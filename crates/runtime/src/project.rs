use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::Cache;
use crate::cache::{check_digest, digest};
use crate::source::Source;
use lenso_cli::Error;
use serde::{Deserialize, Serialize};

use crate::dependencies::resolve;
use crate::{Extension, HandlerSpec, ProcessSpec, project_path};

/// Register explicit update, configuration inspection, migration, and cache discovery.
///
/// # Errors
/// Returns a command or handler registration conflict.
pub fn register_management(cli: &mut lenso_cli::Cli) -> Result<(), Error> {
    register_management_with_options(cli, &LoadOptions::default())
}

/// Register management commands with the host's dependency access policy.
///
/// # Errors
/// Returns a command or handler registration conflict.
pub fn register_management_with_options(
    cli: &mut lenso_cli::Cli,
    options: &LoadOptions,
) -> Result<(), Error> {
    use lenso_cli::{CommandSpec, Outcome};
    crate::onboarding::register(cli, options)?;
    cli.command(
        CommandSpec {
            path: vec!["package".into()],
            workflow: "lenso/package".into(),
            about: "Build release assets from a prebuilt local extension".into(),
            provider: "lenso".into(),
            arguments: vec![
                lenso_cli::Argument {
                    name: "id".into(),
                    help: "Author package ID from lenso.registry.toml".into(),
                    positional: false,
                    kind: Default::default(),
                },
                lenso_cli::Argument {
                    name: "out".into(),
                    help: "Output directory relative to the project (default dist/lenso)".into(),
                    positional: false,
                    kind: Default::default(),
                },
            ],
        },
        |ctx| async move {
            let output = ctx.input["out"].as_str().unwrap_or("dist/lenso");
            let path = crate::project_path(&ctx.project, Path::new(output))?;
            let result = crate::package_extension(&ctx.project, ctx.input["id"].as_str(), &path)?;
            Ok(Outcome::complete(result))
        },
    )?;
    let publishing = options.clone();
    cli.command(
        CommandSpec {
            path: vec!["publish".into()],
            workflow: "lenso/publish".into(),
            about: "Package, verify and publish an extension to GitHub Releases".into(),
            provider: "lenso".into(),
            arguments: vec![
                lenso_cli::Argument {
                    name: "github".into(),
                    help: "Publish through GitHub Releases".into(),
                    positional: false,
                    kind: lenso_cli::ArgumentKind::Flag,
                },
                lenso_cli::Argument {
                    name: "repo".into(),
                    help: "Target owner/repo (defaults to GITHUB_REPOSITORY in Actions)".into(),
                    positional: false,
                    kind: Default::default(),
                },
                lenso_cli::Argument {
                    name: "id".into(),
                    help: "Author package ID from lenso.registry.toml".into(),
                    positional: false,
                    kind: Default::default(),
                },
                lenso_cli::Argument {
                    name: "target".into(),
                    help: "Remote target ref/commit (Actions GITHUB_SHA or HEAD by default)".into(),
                    positional: false,
                    kind: Default::default(),
                },
                lenso_cli::Argument {
                    name: "out".into(),
                    help: "Prepared artifact directory relative to the project".into(),
                    positional: false,
                    kind: Default::default(),
                },
                lenso_cli::Argument {
                    name: "api".into(),
                    help: "Explicit compatible GitHub API base".into(),
                    positional: false,
                    kind: Default::default(),
                },
                lenso_cli::Argument {
                    name: "token-env".into(),
                    help: "Credential environment variable (otherwise GH_TOKEN or GITHUB_TOKEN)"
                        .into(),
                    positional: false,
                    kind: Default::default(),
                },
                lenso_cli::Argument {
                    name: "draft".into(),
                    help: "Upload and verify assets, leaving the release as a draft".into(),
                    positional: false,
                    kind: lenso_cli::ArgumentKind::Flag,
                },
                lenso_cli::Argument {
                    name: "dry-run".into(),
                    help: "Prepare and validate local assets without GitHub requests".into(),
                    positional: false,
                    kind: lenso_cli::ArgumentKind::Flag,
                },
            ],
        },
        move |ctx| {
            let options = publishing.clone();
            async move { crate::publish::run(ctx, options).await }
        },
    )?;
    let resolution = options.clone();
    cli.command(
        CommandSpec {
            path: vec!["update".into()],
            workflow: "lenso/update".into(),
            about: "Refresh dependency pins from declared sources".into(),
            provider: "lenso".into(),
            arguments: Vec::new(),
        },
        move |ctx| {
            let mut options = resolution.clone();
            async move {
                options.mode = LoadMode::Update;
                options.events = ctx.events;
                let count =
                    tokio::task::spawn_blocking(move || prepare_project(&ctx.project, &options))
                        .await
                        .map_err(|e| Error::Operation(e.to_string()))??;
                let message = format!("Updated {count} extension(s) in lenso.lock");
                Ok(Outcome::complete(
                    serde_json::json!({"message":message, "extensions":count}),
                ))
            }
        },
    )?;
    cli.command(
        CommandSpec {
            path: vec!["config".into()],
            workflow: "lenso/config".into(),
            about: "Inspect the root declaration without loading extensions".into(),
            provider: "lenso".into(),
            arguments: Vec::new(),
        },
        |ctx| async move {
            let config = read_project(&ctx.project)?
                .ok_or_else(|| Error::Invalid("no lenso.toml found".into()))?;
            Ok(Outcome::complete(serde_json::json!({
                "message":"Project declaration: lenso.toml (use --json for full configuration)",
                "config":config
            })))
        },
    )?;
    let cache_options = options.clone();
    cli.command(
        CommandSpec {
            path: vec!["cache".into(), "path".into()],
            workflow: "lenso/cache-path".into(),
            about: "Show the per-user shared cache directory".into(),
            provider: "lenso".into(),
            arguments: Vec::new(),
        },
        move |_| {
            let options = cache_options.clone();
            async move {
                let cache = options.cache()?;
                Ok(Outcome::complete(serde_json::json!({
                    "message":cache.root().display().to_string(), "cache_dir":cache.root()
                })))
            }
        },
    )?;
    cli.command(
        CommandSpec {
            path: vec!["migrate".into()],
            workflow: "lenso/migrate".into(),
            about: "Migrate experimental .lenso/project.json installations".into(),
            provider: "lenso".into(),
            arguments: Vec::new(),
        },
        |ctx| async move {
            let count = tokio::task::spawn_blocking(move || crate::migrate_project(&ctx.project))
                .await
                .map_err(|e| Error::Operation(e.to_string()))??;
            let message = format!("Migrated {count} extension(s) to lenso.toml and lenso.lock");
            Ok(Outcome::complete(
                serde_json::json!({"message":message, "extensions":count}),
            ))
        },
    )
}

/// Whether loading executes a command, discovers metadata, or explicitly updates pins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadMode {
    /// Resolve changed declarations and persist the lock before executing.
    #[default]
    Execute,
    /// Resolve metadata for help without writing a project lock or caching Wasm.
    Discover,
    /// Refresh all source metadata while preserving explicit digest requirements.
    Update,
}

/// Host-owned dependency preparation policy; never supplied by an extension.
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// Native implementations available to this host; availability grants no activation.
    pub providers: BTreeMap<String, crate::NativeProvider>,
    /// Disable network access for dependency preparation.
    pub offline: bool,
    /// Reject missing or changed project locks.
    pub locked: bool,
    /// Purpose of this load operation.
    pub mode: LoadMode,
    /// Optional absolute cache root for an embedding application.
    pub cache_dir: Option<PathBuf>,
    /// Native-only progress notifications.
    pub events: lenso_cli::EventSink,
    /// Trusted GitHub transport configuration for source operations.
    pub github: crate::GithubConfig,
}

impl LoadOptions {
    /// Read resolution flags before dynamic command parsing, skipping global values.
    pub fn from_args(args: &[std::ffi::OsString]) -> Self {
        let mut options = Self::default();
        let mut args = args.iter().skip(1);
        let mut command = false;
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            if ["--project", "--input", "--answers", "--color", "--report"]
                .iter()
                .any(|flag| arg == *flag)
            {
                args.next();
                continue;
            }
            if arg == "--offline" {
                options.offline = true;
            }
            if arg == "--locked" {
                options.locked = true;
            }
            if arg == "--help" || arg == "-h" || arg == "--version" || arg == "-V" {
                options.mode = LoadMode::Discover;
            }
            if arg.to_str().is_some_and(|value| !value.starts_with('-')) {
                if !command && arg == "help" {
                    options.mode = LoadMode::Discover;
                }
                command = true;
            }
        }
        if !command {
            options.mode = LoadMode::Discover;
        }
        options
    }

    pub(crate) fn cache(&self) -> Result<Cache, Error> {
        match &self.cache_dir {
            Some(path) => Cache::at(path.clone()),
            None => Cache::from_env(),
        }
    }
}

/// Human-authored project declaration stored in the root `lenso.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// CLI-specific declarations, separate from local cache state.
    pub cli: ProjectCli,
}

/// Explicit extension sources and project-owned policy.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCli {
    /// Declaration schema version; currently 1.
    pub schema_version: u32,
    /// Explicit local or remote extension packages.
    #[serde(default)]
    pub extensions: Vec<Installation>,
    /// Exact reviewed preset compositions; package grants remain visible above.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub presets: BTreeMap<String, crate::PresetSelection>,
    /// Explicit namespace-to-base-URL routes for extension distribution.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub registries: BTreeMap<String, String>,
    /// GitHub API authority and optional credential binding.
    #[serde(default, skip_serializing_if = "crate::github::is_default")]
    pub github: crate::GithubConfig,
}

/// A declared package source and its independently reviewable grants.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Installation {
    /// Expected extension identity.
    pub id: String,
    /// Project-relative manifest outside `.lenso/`, or an explicit HTTPS manifest URL.
    pub source: String,
    /// Explicit repository source grants (github:owner/repo).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    /// Opaque configuration interpreted only by the owning extension.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, serde_json::Value>,
    /// Optional semantic version requirement for the declared source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Whether the user directly selected this package, independently of presets.
    #[serde(default = "enabled", skip_serializing_if = "is_true")]
    pub direct: bool,
    /// Whether commands from this declaration are enabled.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Exact authorized process invocations.
    #[serde(default)]
    pub processes: Vec<ProcessSpec>,
    /// Exact project-relative file destinations.
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// Exact authorized project reads.
    #[serde(default)]
    pub reads: Vec<PathBuf>,
    /// Project-relative directories whose descendant files may be changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub write_roots: Vec<PathBuf>,
    /// Project-relative directories whose descendant files may be read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_roots: Vec<PathBuf>,
    /// Environment variable names usable only by bound HTTP credentials.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    /// Allow bounded project file-name discovery without exposing file contents.
    #[serde(
        default,
        alias = "workspace",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub discover: bool,
    /// Host-only authentication bindings, never serialized into Wasm contexts.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub credentials: BTreeMap<String, crate::Credential>,
    /// Authorized HTTPS URL prefixes.
    #[serde(default)]
    pub http: Vec<String>,
    /// Disabled command aliases, expressed as space-separated paths.
    #[serde(default)]
    pub disabled_commands: Vec<String>,
    /// Explicit command alias replacements.
    #[serde(default)]
    pub aliases: BTreeMap<String, Vec<String>>,
}

impl Installation {
    /// Declare a package without granting any external capabilities.
    pub fn new(id: String, source: String, version: Option<String>) -> Result<Self, Error> {
        serde_json::from_value(serde_json::json!({"id":id,"source":source,"version":version}))
            .map_err(Error::from)
    }
}

fn is_true(value: &bool) -> bool {
    *value
}

fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectLock {
    pub(crate) schema_version: u32,
    pub extensions: Vec<LockedExtension>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub presets: BTreeMap<String, crate::PresetSelection>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<String, crate::resources::ResourcePin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LockedExtension {
    pub id: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) requirement: Option<String>,
    pub(crate) source_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) registry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) github: Option<crate::package::ReleasePin>,
    pub(crate) definition_sha256: String,
    pub definition: Extension,
    pub(crate) modules: BTreeMap<String, LockedModule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LockedModule {
    source: String,
    sha256: String,
}

/// Read the root declaration without reading cache files or executing extensions.
///
/// # Errors
/// Rejects invalid TOML, unsupported schemas, duplicate identities, and legacy layout.
pub fn read_project(project: &Path) -> Result<Option<Project>, Error> {
    let path = project_path(project, Path::new("lenso.toml"))?;
    let bytes = match read_bounded(&path, 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            if project.join(".lenso/project.json").exists() {
                return Err(Error::Invalid(
                    "legacy .lenso/project.json found; run lenso migrate".into(),
                ));
            }
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| Error::Invalid("lenso.toml must be UTF-8".into()))?;
    let config: Project =
        toml::from_str(text).map_err(|e| Error::Invalid(format!("lenso.toml: {e}")))?;
    if config.cli.schema_version != 1 {
        return Err(
            lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::InvalidInput)
                .with_details(serde_json::json!({"field":"cli.schema_version","supported":[1]}))
                .with_retryable(false)
                .into(),
        );
    }
    for (namespace, url) in &config.cli.registries {
        crate::registry::reference(&format!("registry:{namespace}/check"))?;
        crate::registry::validate_base(project, url)?;
    }
    for (id, preset) in &config.cli.presets {
        preset.validate(id)?;
        for (provider, dependency) in &preset.extensions {
            if !config.cli.extensions.iter().any(|entry| {
                entry.enabled && entry.id == *provider && entry.source == dependency.source
            }) {
                return Err(Error::Invalid(format!(
                    "preset {id} requires an enabled {provider} declaration from {}",
                    dependency.source
                )));
            }
        }
    }
    let mut ids = BTreeSet::new();
    for installed in &config.cli.extensions {
        if installed.id.is_empty() || !ids.insert(&installed.id) {
            return Err(Error::Invalid(
                "duplicate or empty declared extension identity".into(),
            ));
        }
        crate::package::reference(&installed.source)?;
        for source in &installed.sources {
            crate::github::Repo::parse(source.strip_prefix("github:").ok_or_else(|| {
                Error::Invalid("source grants require github:owner/repo".into())
            })?)?;
        }
        if !installed.source.starts_with("builtin:")
            && crate::package::reference(&installed.source)?.is_none()
            && crate::registry::route(&installed.source, &config.cli.registries)?.is_none()
        {
            Source::parse(project, &installed.source)?;
        }
        if let Some(version) = &installed.version {
            semver::VersionReq::parse(version)
                .map_err(|e| Error::Invalid(format!("version requirement: {e}")))?;
        }
    }
    Ok(Some(config))
}

pub(crate) fn read_lock(project: &Path) -> Result<Option<ProjectLock>, Error> {
    let path = project_path(project, Path::new("lenso.lock"))?;
    let bytes = match read_bounded(&path, 8 * 1024 * 1024) {
        Ok(bytes) => bytes,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let lock: ProjectLock = serde_json::from_slice(&bytes)?;
    if lock.schema_version != 1 {
        return Err(Error::Invalid("unsupported lock schema_version".into()));
    }
    for (id, preset) in &lock.presets {
        preset.validate(id)?;
    }
    if lock.resources.len() > 128 {
        return Err(Error::Invalid(
            "repository source pin limit exceeded".into(),
        ));
    }
    for (key, pin) in &lock.resources {
        pin.validate()?;
        if &pin.key() != key {
            return Err(Error::Invalid("invalid resource lock key".into()));
        }
    }
    let mut ids = BTreeSet::new();
    for entry in &lock.extensions {
        check_digest(&entry.source_sha256)?;
        if let Some(pin) = &entry.github {
            pin.validate()?;
            if crate::package::reference(entry.resolved_source.as_deref().unwrap_or(&entry.source))?
                .is_none()
                || pin.metadata.id != entry.id
                || pin.metadata.version != entry.definition.version
            {
                return Err(Error::Invalid("invalid locked GitHub package".into()));
            }
        }
        match (&entry.registry, &entry.resolved_source) {
            (Some(base), Some(source)) => {
                crate::registry::reference(&entry.source)?
                    .ok_or_else(|| Error::Invalid("locked Registry reference is missing".into()))?;
                crate::registry::validate_base(project, base)?;
                if crate::package::reference(source)?.is_none()
                    && !matches!(Source::parse(project, source)?, Source::Remote(_))
                {
                    return Err(Error::Invalid(
                        "locked Registry manifest must be remote".into(),
                    ));
                }
            }
            (None, None) => {
                if entry.github.is_none() && !entry.source.starts_with("builtin:") {
                    Source::parse(project, &entry.source)?;
                }
            }
            _ => {
                return Err(Error::Invalid(
                    "incomplete locked Registry resolution".into(),
                ));
            }
        }
        for module in entry.modules.values() {
            check_digest(&module.sha256)?;
            Source::parse(project, &module.source)?;
        }
        if !ids.insert(&entry.id)
            || entry.id != entry.definition.id
            || digest(&serde_json::to_vec(&entry.definition)?) != entry.definition_sha256
        {
            return Err(Error::Invalid(
                "invalid or changed locked extension definition; run lenso update".into(),
            ));
        }
        for handler in entry.definition.handlers.values() {
            if let HandlerSpec::Wasm(spec) = handler
                && entry
                    .modules
                    .get(&spec.module)
                    .is_none_or(|module| module.sha256 != spec.sha256)
            {
                return Err(Error::Invalid(
                    "locked module metadata does not match its handler".into(),
                ));
            }
        }
    }
    Ok(Some(lock))
}

/// Resolve declared sources and save metadata without executing extension code.
///
/// Modules are fetched lazily by actual handlers. Embedders normally use
/// [`crate::load_project_with_options`] so definitions are also checked against their host.
///
/// # Errors
/// Rejects invalid definitions, changed locked declarations, or unavailable sources.
pub fn prepare_project(project: &Path, options: &LoadOptions) -> Result<usize, Error> {
    let project = project.canonicalize()?;
    let config =
        read_project(&project)?.ok_or_else(|| Error::Invalid("no lenso.toml found".into()))?;
    let _guard = resolution_guard(&project, &config, options)?;
    let next = resolve(&project, &config, options)?;
    validate(&project, &config, &next, options)?;
    save_lock(&project, &next, options)?;
    Ok(next.extensions.len())
}

pub(crate) fn validate(
    project: &Path,
    config: &Project,
    next: &ProjectLock,
    options: &LoadOptions,
) -> Result<(), Error> {
    // Validate all definitions together before replacing the lock or touching cache.
    let mut check = lenso_cli::Cli::new("validation");
    register_management(&mut check)?;
    let namespaces: Vec<_> = next
        .extensions
        .iter()
        .map(|entry| format!("{}/", entry.id))
        .collect();
    let defined: BTreeSet<_> = next
        .extensions
        .iter()
        .flat_map(|entry| entry.definition.handlers.keys())
        .collect();
    for entry in &next.extensions {
        for step in entry
            .definition
            .workflows
            .iter()
            .flat_map(|workflow| &workflow.steps)
        {
            for id in std::iter::once(&step.handler)
                .chain(&step.hooks.prepare)
                .chain(&step.hooks.before)
                .chain(&step.hooks.validate)
                .chain(&step.hooks.after)
            {
                // Host-native dependencies are checked against the real application at load time.
                if !defined.contains(id)
                    && !namespaces.iter().any(|namespace| id.starts_with(namespace))
                    && !options
                        .providers
                        .keys()
                        .any(|provider| id.starts_with(&format!("{provider}/")))
                    && !check.engine_mut().contains_handler(id)
                {
                    check.engine_mut().handler(id, |_| async {
                        Err(Error::Operation("validation-only native reference".into()))
                    })?;
                }
            }
        }
    }
    crate::manifest::register_locked(&mut check, project, config, next, options)?;
    Ok(())
}

pub(crate) fn resolution_guard(
    project: &Path,
    _config: &Project,
    options: &LoadOptions,
) -> Result<Option<std::fs::File>, Error> {
    if options.mode == LoadMode::Discover || options.locked {
        return Ok(None);
    }
    Ok(Some(
        options
            .cache()?
            .lock(&format!("project:{}", project.display()))?,
    ))
}

pub(crate) fn declarations_match(lock: &ProjectLock, config: &Project) -> bool {
    lock.presets == config.cli.presets
        && lock.extensions.iter().all(|entry| {
            entry.registry.as_deref()
                == crate::registry::route(&entry.source, &config.cli.registries)
                    .ok()
                    .flatten()
                && entry
                    .github
                    .as_ref()
                    .is_none_or(|pin| pin.api == config.cli.github.api)
        })
        && config
            .cli
            .extensions
            .iter()
            .filter(|entry| entry.enabled)
            .all(|installed| {
                lock.extensions.iter().any(|entry| {
                    entry.id == installed.id
                        && entry.source == installed.source
                        && entry.requirement == installed.version
                        && entry
                            .github
                            .as_ref()
                            .is_none_or(|pin| pin.api == config.cli.github.api)
                        && entry.registry.as_deref()
                            == crate::registry::route(&installed.source, &config.cli.registries)
                                .ok()
                                .flatten()
                })
            })
}

pub(crate) fn resolve_flat(
    project: &Path,
    config: &Project,
    options: &LoadOptions,
) -> Result<ProjectLock, Error> {
    if options.locked && options.mode == LoadMode::Update {
        return Err(Error::Invalid(
            "update cannot be combined with --locked".into(),
        ));
    }
    let previous = read_lock(project)?;
    let matches = |locked: &LockedExtension, installed: &Installation| {
        locked.id == installed.id
            && locked.source == installed.source
            && locked.requirement == installed.version
            && locked
                .github
                .as_ref()
                .is_none_or(|pin| pin.api == config.cli.github.api)
            && locked.registry.as_deref()
                == crate::registry::route(&installed.source, &config.cli.registries)
                    .ok()
                    .flatten()
    };
    let cache = options.cache()?;
    let mut entries = Vec::new();
    for installed in config.cli.extensions.iter().filter(|entry| entry.enabled) {
        if let Some(id) = installed.source.strip_prefix("builtin:") {
            let provider = options.providers.get(id).ok_or_else(|| {
                lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::DependencyUnavailable)
                    .with_details(
                        serde_json::json!({"provider":id,"reason":"native_provider_unavailable"}),
                    )
                    .with_retryable(false)
                    .with_recovery(
                        "select_host",
                        "Use a Host that explicitly makes this native provider available.",
                    )
            })?;
            let definition: Extension = serde_json::from_value(
                serde_json::json!({"schema_version":1,"id":id,"version":provider.version,"handlers":{},"workflows":[],"commands":[]}),
            )?;
            if installed.id != id {
                return Err(Error::Invalid("native provider identity mismatch".into()));
            }
            let version = semver::Version::parse(&provider.version)
                .map_err(|e| Error::Invalid(e.to_string()))?;
            if let Some(req) = &installed.version
                && !semver::VersionReq::parse(req)
                    .map_err(|e| Error::Invalid(e.to_string()))?
                    .matches(&version)
            {
                return Err(Error::Invalid(format!(
                    "native provider {id} version {version} does not satisfy {req}; upgrade CLI"
                )));
            }
            let hash = digest(&serde_json::to_vec(&definition)?);
            entries.push(LockedExtension {
                id: id.into(),
                source: installed.source.clone(),
                requirement: installed.version.clone(),
                source_sha256: hash.clone(),
                definition_sha256: hash,
                registry: None,
                resolved_source: None,
                github: None,
                definition,
                modules: BTreeMap::new(),
            });
            continue;
        }
        if options.mode != LoadMode::Update
            && let Some(entry) = previous.as_ref().and_then(|lock| {
                lock.extensions
                    .iter()
                    .find(|entry| matches(entry, installed))
            })
        {
            entries.push(entry.clone());
            continue;
        }
        options
            .events
            .preparing(format!("Resolving {}", installed.id));
        let mut source_options = options.clone();
        if previous
            .as_ref()
            .is_some_and(|lock| lock.extensions.iter().any(|entry| entry.id == installed.id))
        {
            source_options.mode = LoadMode::Update;
        }
        let registry = crate::registry::route(&installed.source, &config.cli.registries)?;
        let package = registry
            .map(|base| {
                crate::registry::resolve(
                    project,
                    &installed.source,
                    base,
                    installed.version.as_deref(),
                    &cache,
                    &source_options,
                )
            })
            .transpose()?;
        let release_source = package
            .as_ref()
            .map_or(installed.source.as_str(), |package| {
                package.manifest.as_str()
            });
        if crate::package::reference(release_source)?.is_some() {
            let github = crate::github::Github::new(&config.cli.github)?;
            let (pin, bytes) = crate::package::resolve(
                release_source,
                &installed.id,
                installed.version.as_deref(),
                package.as_ref().map(|package| &package.version),
                &github,
                &cache,
                &source_options,
            )?;
            let virtual_source = Source::parse(
                project,
                &format!(
                    "https://github.com/{}/releases/download/{}/{}",
                    pin.repository.name(),
                    pin.tag,
                    pin.metadata.manifest.name
                ),
            )?;
            let mut entry =
                resolve_extension(project, installed, &virtual_source, &bytes, options)?;
            if entry.definition.version != pin.metadata.version {
                return Err(Error::Invalid("release manifest version mismatch".into()));
            }
            if let Some(package) = &package
                && (digest(&bytes) != package.sha256
                    || entry.definition.version != package.version.to_string())
            {
                return Err(Error::Permission(
                    "GitHub package does not match Registry digest/version".into(),
                ));
            }
            entry.registry = registry.map(str::to_owned);
            entry.resolved_source = package.map(|package| package.manifest);
            entry.github = Some(pin);
            entries.push(entry);
            continue;
        }
        let declaration = package
            .as_ref()
            .map_or(installed.source.as_str(), |package| {
                package.manifest.as_str()
            });
        let source = Source::parse(project, declaration)?;
        let mut entry = source.manifest(&cache, &source_options, |bytes| {
            if let Some(package) = &package
                && digest(bytes) != package.sha256
            {
                return Err(Error::Permission(
                    "extension manifest digest does not match Registry metadata".into(),
                ));
            }
            let entry = resolve_extension(project, installed, &source, bytes, options)?;
            if let Some(package) = &package
                && entry.definition.version != package.version.to_string()
            {
                return Err(Error::Invalid(
                    "extension manifest version does not match Registry release".into(),
                ));
            }
            Ok(entry)
        })?;
        entry.registry = registry.map(str::to_owned);
        entry.resolved_source = package.map(|package| package.manifest);
        entries.push(entry);
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(ProjectLock {
        schema_version: 1,
        extensions: entries,
        presets: config.cli.presets.clone(),
        resources: previous.map(|p| p.resources).unwrap_or_default(),
    })
}

fn resolve_extension(
    project: &Path,
    installed: &Installation,
    source: &Source,
    bytes: &[u8],
    options: &LoadOptions,
) -> Result<LockedExtension, Error> {
    let mut definition: Extension = if source.is_toml() {
        toml::from_str(
            std::str::from_utf8(bytes)
                .map_err(|_| Error::Invalid("manifest must be UTF-8".into()))?,
        )
        .map_err(|e| Error::Invalid(format!("manifest TOML: {e}")))?
    } else {
        serde_json::from_slice(bytes)?
    };
    if definition.id != installed.id {
        return Err(Error::Invalid(format!(
            "source identity does not match {}",
            installed.id
        )));
    }
    if let Some(requirement) = &installed.version {
        let requirement =
            semver::VersionReq::parse(requirement).map_err(|e| Error::Invalid(e.to_string()))?;
        let version = semver::Version::parse(&definition.version)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if !requirement.matches(&version) {
            return Err(Error::Invalid(format!(
                "{} version {} does not satisfy {requirement}",
                installed.id, definition.version
            )));
        }
    }
    let mut modules = BTreeMap::new();
    for handler in definition.handlers.values_mut() {
        if let HandlerSpec::Wasm(spec) = handler {
            let module_source = source.module(project, &installed.source, &spec.module)?;
            if spec.sha256 == "auto" {
                if matches!(source, Source::Remote(_)) {
                    return Err(Error::Invalid(
                        "remote Wasm manifests must declare exact SHA-256 digests".into(),
                    ));
                }
                spec.sha256 = digest(
                    &Source::parse(project, &module_source)?.bytes(16 * 1024 * 1024, options)?,
                );
            }
            check_digest(&spec.sha256)?;
            if modules
                .get(&spec.module)
                .is_some_and(|module: &LockedModule| module.sha256 != spec.sha256)
            {
                return Err(Error::Invalid(
                    "conflicting SHA-256 pins for the same module".into(),
                ));
            }
            modules.insert(
                spec.module.clone(),
                LockedModule {
                    source: module_source,
                    sha256: spec.sha256.clone(),
                },
            );
        }
    }
    Ok(LockedExtension {
        id: installed.id.clone(),
        source: installed.source.clone(),
        requirement: installed.version.clone(),
        source_sha256: digest(bytes),
        registry: None,
        resolved_source: None,
        github: None,
        definition_sha256: digest(&serde_json::to_vec(&definition)?),
        definition,
        modules,
    })
}

pub(crate) fn save_lock(
    project: &Path,
    lock: &ProjectLock,
    options: &LoadOptions,
) -> Result<(), Error> {
    write_lock(project, lock, options, LockWrite::Extensions)
}
pub(crate) fn save_resource_lock(
    project: &Path,
    lock: &ProjectLock,
    options: &LoadOptions,
) -> Result<(), Error> {
    write_lock(project, lock, options, LockWrite::Resources)
}
enum LockWrite {
    Extensions,
    Resources,
}
fn write_lock(
    project: &Path,
    lock: &ProjectLock,
    options: &LoadOptions,
    scope: LockWrite,
) -> Result<(), Error> {
    if options.mode == LoadMode::Discover || options.locked {
        return Ok(());
    }
    let _guard = options
        .cache()?
        .lock(&format!("lock-write:{}", project.display()))?;
    let mut next = lock.clone();
    if let Some(current) = read_lock(project)? {
        match scope {
            LockWrite::Extensions => next.resources = current.resources,
            LockWrite::Resources => {
                next.extensions = current.extensions;
                next.presets = current.presets;
            }
        }
    }
    let path = project_path(project, Path::new("lenso.lock"))?;
    let bytes = serde_json::to_vec_pretty(&next)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(Error::Invalid("project lock exceeds 8 MiB".into()));
    }
    match std::fs::read(&path) {
        Ok(previous) if previous == bytes => return Ok(()),
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    atomic_write(&path, &bytes)
}

impl LockedExtension {
    pub fn module(
        &self,
        project: &Path,
        name: &str,
        options: &LoadOptions,
    ) -> Result<PathBuf, Error> {
        let module = self
            .modules
            .get(name)
            .ok_or_else(|| Error::Invalid("module is absent from lenso.lock".into()))?;
        if let Some(pin) = &self.github {
            return crate::package::module(
                pin,
                &self.definition,
                name,
                &crate::github::Github::new(&options.github)?,
                &options.cache()?,
                options,
            );
        }
        let source = Source::parse(project, &module.source)?;
        options
            .cache()?
            .ensure(&module.sha256, 16 * 1024 * 1024, || {
                options.events.preparing(format!("Preparing {}", self.id));
                let bytes = source.bytes(16 * 1024 * 1024, options)?;
                if digest(&bytes) != module.sha256 {
                    return Err(lenso_cli::Diagnostic::new(
                        lenso_cli::DiagnosticCode::IntegrityMismatch,
                    )
                    .with_details(serde_json::json!({"artifact":"wasm"}))
                    .with_retryable(false)
                    .with_recovery(
                        "restore_pinned",
                        "Restore the pinned bytes, or explicitly review and update the dependency.",
                    )
                    .into());
                }
                Ok(bytes)
            })
    }
}

pub(crate) fn read_bounded(path: &Path, max: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(Error::Invalid(format!(
            "file exceeds size limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| Error::Invalid("file has no parent".into()))?,
    )?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
