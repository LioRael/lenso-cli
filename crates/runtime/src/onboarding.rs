use std::collections::BTreeMap;
use std::path::Path;

use lenso_cli::{
    Argument, Cli, CommandSpec, Context, Error, Outcome, Question, QuestionKind, Status,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::project::{Installation, LoadMode, LoadOptions, atomic_write, read_project};

/// Author-owned composition, independent of any application or resource protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Composition schema, currently 1.
    pub schema_version: u32,
    /// Stable composition identity.
    pub id: String,
    /// Composition version.
    pub version: String,
    /// Packages and proposed grants. Grants require host confirmation.
    pub extensions: Vec<Installation>,
    /// Initialization workflow for new directories, when supported.
    #[serde(default)]
    pub create: Option<String>,
    /// Initialization workflow for existing projects, when supported.
    #[serde(default)]
    pub adopt: Option<String>,
}

/// A reviewed, immutable preset selection and its contributed direct packages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PresetSelection {
    /// Original preset reference.
    pub source: String,
    /// Exact reviewed preset version.
    pub version: String,
    /// Digest of the reviewed composition metadata.
    pub sha256: String,
    /// Direct provider identities contributed by this composition.
    pub extensions: BTreeMap<String, crate::Dependency>,
}

impl PresetSelection {
    pub(crate) fn validate(&self, id: &str) -> Result<(), Error> {
        if id.is_empty() || self.source.is_empty() || self.extensions.len() > 128 {
            return Err(Error::Invalid(
                "invalid preset selection identity or size".into(),
            ));
        }
        semver::Version::parse(&self.version).map_err(|e| Error::Invalid(e.to_string()))?;
        crate::cache::check_digest(&self.sha256)?;
        for (provider, dependency) in &self.extensions {
            if provider.is_empty() || dependency.source.is_empty() {
                return Err(Error::Invalid(
                    "empty preset dependency identity or source".into(),
                ));
            }
            semver::VersionReq::parse(&dependency.version)
                .map_err(|e| Error::Invalid(e.to_string()))?;
        }
        Ok(())
    }
}

/// Extension-owned onboarding metadata. The host only understands grants and workflows.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    /// Proposed Installation fields excluding identity and distribution source.
    #[serde(default)]
    pub defaults: BTreeMap<String, Value>,
    /// Optional workflow that asks questions and returns proposed `installation` fields.
    #[serde(default)]
    pub configure: Option<String>,
}

fn argument(name: &str, positional: bool, help: &str) -> Argument {
    Argument {
        name: name.into(),
        positional,
        help: help.into(),
        kind: Default::default(),
    }
}

pub(crate) fn register(cli: &mut Cli, options: &LoadOptions) -> Result<(), Error> {
    for (path, about, mut args) in [
        (
            vec!["new"],
            "Create a project from an optional preset",
            vec![
                argument("directory", true, "New project directory"),
                argument("preset", false, "Official name or preset manifest URL/path"),
            ],
        ),
        (
            vec!["init"],
            "Configure Lenso in an existing project",
            vec![argument(
                "preset",
                false,
                "Official name or preset manifest URL/path",
            )],
        ),
        (
            vec!["ext", "add"],
            "Add an extension and review its dependency tree and grants",
            vec![
                argument(
                    "extension",
                    true,
                    "Official name, @namespace/name, or explicit source",
                ),
                argument("id", false, "Provider ID for explicit sources"),
                argument("constraint", false, "Semantic version requirement"),
            ],
        ),
        (
            vec!["ext", "configure"],
            "Review configuration and grants for an installed extension",
            vec![argument("extension", true, "Installed provider ID")],
        ),
        (
            vec!["ext", "remove"],
            "Remove a direct extension selection",
            vec![argument("extension", true, "Provider ID")],
        ),
        (
            vec!["ext", "tree"],
            "Show resolved extensions and dependency edges",
            vec![],
        ),
        (
            vec!["preset", "apply"],
            "Apply a versioned extension composition",
            vec![argument(
                "preset",
                true,
                "Official name or preset manifest URL/path",
            )],
        ),
        (
            vec!["preset", "remove"],
            "Remove a preset selection while keeping shared packages",
            vec![argument(
                "preset",
                true,
                "Preset identity or official short name",
            )],
        ),
    ] {
        let operation = path.join(" ");
        if matches!(
            operation.as_str(),
            "new" | "init" | "ext add" | "ext configure" | "preset apply"
        ) {
            args.push(argument(
                "token-env",
                false,
                "Explicit GitHub credential environment variable",
            ));
            args.push(argument(
                "api",
                false,
                "Explicit compatible GitHub API base",
            ));
        }
        let workflow = format!("lenso/{}", path.join("-"));
        let options = options.clone();
        cli.command(
            CommandSpec {
                path: path.into_iter().map(String::from).collect(),
                workflow,
                about: about.into(),
                provider: "lenso".into(),
                arguments: args,
            },
            move |ctx| {
                let options = options.clone();
                let operation = operation.clone();
                async move { run(ctx, options, &operation).await }
            },
        )?;
    }
    Ok(())
}

fn question(
    ctx: &Context,
    id: &str,
    message: String,
    kind: QuestionKind,
) -> Result<Option<Outcome>, Error> {
    let question = Question {
        id: id.into(),
        message,
        kind,
    };
    match ctx.answers.get(id) {
        None => Ok(Some(Outcome::NeedsInput { question })),
        Some(value) => {
            question.validate(value)?;
            if matches!(question.kind, QuestionKind::Confirm) && value == &Value::Bool(false) {
                return Err(Error::Cancelled);
            }
            Ok(None)
        }
    }
}

fn package(
    name: &str,
    id: Option<&str>,
    version: Option<&str>,
    providers: &BTreeMap<String, crate::NativeProvider>,
) -> Result<Installation, Error> {
    let source =
        if name.contains(':') || name.contains('.') || name.contains('/') && !name.starts_with('@')
        {
            name.to_owned()
        } else if providers.contains_key(name.strip_prefix("@lenso/").unwrap_or(name)) {
            format!("builtin:{}", name.strip_prefix("@lenso/").unwrap_or(name))
        } else if name.starts_with('@') {
            format!("registry:{name}")
        } else {
            format!("registry:@lenso/{name}")
        };
    let id = id
        .map(String::from)
        .or_else(|| {
            source
                .strip_prefix("registry:")
                .and_then(|name| name.rsplit('/').next())
                .map(String::from)
        })
        .or_else(|| source.strip_prefix("builtin:").map(String::from))
        .ok_or_else(|| Error::Invalid("explicit extension sources require --id".into()))?;
    Installation::new(id, source, version.map(String::from))
}

fn preset_choices(project: &Path, options: &LoadOptions) -> Option<Vec<String>> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Catalog {
        schema_version: u32,
        presets: Vec<String>,
    }
    let source = crate::source::Source::parse(
        project,
        "https://raw.githubusercontent.com/LioRael/lenso-registry/main/presets/index.json",
    )
    .ok()?;
    let mut discovery = options.clone();
    discovery.mode = LoadMode::Update;
    source
        .manifest(&options.cache().ok()?, &discovery, |bytes| {
            let catalog: Catalog = serde_json::from_slice(bytes)?;
            if catalog.schema_version != 1
                || catalog.presets.len() > 128
                || catalog.presets.iter().any(|name| {
                    name.is_empty()
                        || !name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                })
            {
                return Err(Error::Invalid("invalid preset catalog".into()));
            }
            let mut choices = vec!["empty".to_owned()];
            for name in catalog.presets {
                if !choices.contains(&name) {
                    choices.push(name);
                }
            }
            Ok(choices)
        })
        .ok()
}

fn preset(project: &Path, reference: &str, options: &LoadOptions) -> Result<Preset, Error> {
    let reference = reference.strip_prefix("@lenso/").unwrap_or(reference);
    let source = if reference.contains(':')
        || reference.contains('/')
        || reference.ends_with(".toml")
        || reference.ends_with(".json")
    {
        reference.to_owned()
    } else {
        format!(
            "https://raw.githubusercontent.com/LioRael/lenso-registry/main/presets/{reference}.toml"
        )
    };
    let source = crate::source::Source::parse(project, &source)?;
    source.manifest(&options.cache()?, options, |bytes| {
        let preset: Preset = if source.is_toml() {
            toml::from_str(
                std::str::from_utf8(bytes)
                    .map_err(|_| Error::Invalid("preset must be UTF-8".into()))?,
            )
            .map_err(|e| Error::Invalid(e.to_string()))?
        } else {
            serde_json::from_slice(bytes)?
        };
        if preset.schema_version != 1 || preset.id.is_empty() || preset.extensions.len() > 128 {
            return Err(Error::Invalid(
                "invalid preset schema, identity or size".into(),
            ));
        }
        semver::Version::parse(&preset.version).map_err(|e| Error::Invalid(e.to_string()))?;
        Ok(preset)
    })
}

async fn workflow(ctx: &Context, cli: &mut Cli, id: &str) -> Result<Outcome, Error> {
    let engine = cli.engine_mut();
    let mut session = engine.session(id, &ctx.project, ctx.input.clone(), ctx.answers.clone())?;
    session.observe(ctx.events.clone());
    let report = session.advance(engine).await?;
    if report.status == Status::NeedsInput {
        return Ok(Outcome::NeedsInput {
            question: report
                .question
                .ok_or_else(|| Error::Operation("missing workflow question".into()))?,
        });
    }
    Ok(Outcome::complete(json!(report.outputs)))
}

async fn configure(
    ctx: &Context,
    config: &mut crate::Project,
    lock: &crate::project::ProjectLock,
    options: &LoadOptions,
    added: &[Installation],
) -> Result<Option<Outcome>, Error> {
    for entry in &lock.extensions {
        let Some(id) = entry
            .definition
            .setup
            .as_ref()
            .and_then(|setup| setup.configure.as_deref())
            .filter(|_| added.iter().any(|installed| installed.id == entry.id))
        else {
            continue;
        };
        for phase in 0..4 {
            let before = serde_json::to_value(&config)?;
            let mut execution = Cli::new("lenso");
            crate::register_management_with_options(&mut execution, options)?;
            crate::manifest::register_locked(&mut execution, &ctx.project, config, lock, options)?;
            let mut setup_ctx = ctx.clone();
            setup_ctx.input["_lenso_setup"] = json!({"phase":phase});
            let output = match workflow(&setup_ctx, &mut execution, id).await? {
                outcome @ Outcome::NeedsInput { .. } => return Ok(Some(outcome)),
                Outcome::Complete { output } => output,
            };
            let proposals = output
                .as_object()
                .ok_or_else(|| Error::Invalid("setup outputs must be an object".into()))?
                .values()
                .filter(|step| step.get("installation").is_some())
                .collect::<Vec<_>>();
            if proposals.len() > 1 {
                return Err(Error::Invalid(
                    "setup must return at most one installation proposal".into(),
                ));
            }
            let Some(proposal) = proposals.first() else {
                break;
            };
            let more = proposal
                .get("continue")
                .map(|value| {
                    value
                        .as_bool()
                        .ok_or_else(|| Error::Invalid("setup continue must be a boolean".into()))
                })
                .transpose()?
                .unwrap_or(false);
            let installed = config
                .cli
                .extensions
                .iter_mut()
                .find(|installation| installation.id == entry.id)
                .expect("added entry exists");
            let mut value = serde_json::to_value(&*installed)?;
            for (key, value_new) in proposal["installation"]
                .as_object()
                .ok_or_else(|| Error::Invalid("setup installation must be an object".into()))?
            {
                if matches!(
                    key.as_str(),
                    "id" | "source" | "version" | "enabled" | "direct"
                ) {
                    return Err(Error::Invalid(
                        "setup cannot change package identity".into(),
                    ));
                }
                value[key] = value_new.clone();
            }
            *installed = serde_json::from_value(value)?;
            if serde_json::to_value(&config)? != before {
                let bytes =
                    toml::to_string_pretty(config).map_err(|e| Error::Invalid(e.to_string()))?;
                let digest = crate::cache::digest(&serde_json::to_vec(
                    &json!({"project":ctx.project,"config":config,"lock":lock,"provider":entry.id,"phase":phase}),
                )?);
                if let Some(outcome) = question(
                    ctx,
                    &format!("grants:{digest}"),
                    format!("Proposed configuration and grants:\n{bytes}\nApprove these changes?"),
                    QuestionKind::Confirm,
                )? {
                    return Ok(Some(outcome));
                }
            }
            if !more {
                break;
            }
            if phase == 3 {
                return Err(Error::Invalid(
                    "setup exceeded four permission negotiation rounds".into(),
                ));
            }
        }
    }
    Ok(None)
}

async fn run(
    mut ctx: Context,
    mut options: LoadOptions,
    operation: &str,
) -> Result<Outcome, Error> {
    if options.locked && operation != "ext tree" {
        return Err(Error::Invalid(
            "configuration changes cannot use --locked".into(),
        ));
    }
    options.events = ctx.events.clone();
    options.mode = LoadMode::Execute;
    let creating = operation == "new";
    let mut target = ctx.project.clone();
    if creating {
        let directory = ctx.input["directory"]
            .as_str()
            .or_else(|| ctx.answers.get("directory").and_then(Value::as_str));
        let Some(directory) = directory else {
            return Ok(Outcome::NeedsInput {
                question: Question {
                    id: "directory".into(),
                    message: "New project directory".into(),
                    kind: QuestionKind::Text,
                },
            });
        };
        target = crate::project_path(&ctx.project, Path::new(directory))?;
        if target.exists() && target.read_dir()?.next().is_some() {
            return Err(Error::Invalid("new project directory must be empty".into()));
        }
    }
    let reference = ctx.input["preset"].as_str().map(String::from).or_else(|| {
        ctx.answers
            .get("preset")
            .and_then(Value::as_str)
            .map(String::from)
    });
    if matches!(operation, "new" | "init") && reference.is_none() {
        let catalog_options = options.clone();
        let catalog_root = ctx.project.clone();
        let choices =
            tokio::task::spawn_blocking(move || preset_choices(&catalog_root, &catalog_options))
                .await
                .map_err(|e| Error::Operation(e.to_string()))?;
        return Ok(Outcome::NeedsInput {
            question: Question {
                id: "preset".into(),
                message: "Choose a project preset (empty for no extensions)".into(),
                kind: choices.map_or(QuestionKind::Text, |options| QuestionKind::Select {
                    options,
                }),
            },
        });
    }
    let extension = ctx.input["extension"]
        .as_str()
        .map(String::from)
        .or_else(|| {
            ctx.answers
                .get("extension")
                .and_then(Value::as_str)
                .map(String::from)
        });
    if matches!(operation, "ext add" | "ext remove" | "ext configure") && extension.is_none() {
        return Ok(Outcome::NeedsInput {
            question: Question {
                id: "extension".into(),
                message: "Extension name or source".into(),
                kind: QuestionKind::Text,
            },
        });
    }
    // Resolve against the real project so relative sources keep their original meaning.
    let root = if creating { &ctx.project } else { &target };
    let old = if creating { None } else { read_project(root)? };
    let mut config = old.clone().unwrap_or_default();
    config.cli.schema_version = 1;
    if let Some(name) = ctx.input["token-env"].as_str() {
        if name.is_empty()
            || !name.bytes().enumerate().all(|(index, byte)| {
                byte == b'_' || byte.is_ascii_alphabetic() || index > 0 && byte.is_ascii_digit()
            })
        {
            return Err(Error::Invalid(
                "token-env must name an environment variable".into(),
            ));
        }
        config.cli.github.token_env = Some(name.to_owned());
    }
    if let Some(api) = ctx.input["api"].as_str() {
        let crate::source::Source::Remote(url) = crate::source::Source::parse(root, api)? else {
            return Err(Error::Invalid(
                "GitHub API base requires an HTTPS URL".into(),
            ));
        };
        if !api.ends_with('/') || url.query().is_some() {
            return Err(Error::Invalid(
                "GitHub API base must end in / without a query".into(),
            ));
        }
        config.cli.github.api = api.to_owned();
    }
    let mut recipe = None;
    let mut preset_identity = None;
    let mut selection = None;
    let mut added = Vec::new();
    if matches!(operation, "new" | "init" | "preset apply")
        && let Some(reference) = reference.as_deref().filter(|name| *name != "empty")
    {
        let options = options.clone();
        let root = root.to_owned();
        let reference = reference.to_owned();
        let fetched_reference = reference.clone();
        let composition =
            tokio::task::spawn_blocking(move || preset(&root, &fetched_reference, &options))
                .await
                .map_err(|e| Error::Operation(e.to_string()))??;
        recipe = if creating {
            composition.create.clone()
        } else {
            composition.adopt.clone()
        };
        if recipe.is_none() {
            return Err(Error::Invalid(format!(
                "preset {} does not support {}",
                composition.id,
                if creating {
                    "new projects"
                } else {
                    "existing projects"
                }
            )));
        }
        selection = Some((
            composition.id.clone(),
            PresetSelection {
                source: reference.clone(),
                version: composition.version.clone(),
                sha256: crate::cache::digest(&serde_json::to_vec(&composition)?),
                extensions: composition
                    .extensions
                    .iter()
                    .map(|entry| {
                        (
                            entry.id.clone(),
                            crate::Dependency {
                                source: entry.source.clone(),
                                version: entry.version.clone().unwrap_or("*".into()),
                            },
                        )
                    })
                    .collect(),
            },
        ));
        preset_identity = Some(json!({"id":composition.id,"version":composition.version}));
        added = composition.extensions;
        for installed in &mut added {
            installed.direct = false;
            installed.version = None;
        }
    } else if operation == "preset apply" {
        return Err(Error::Invalid("preset apply requires a preset".into()));
    }
    if operation == "ext add" {
        added.push(package(
            extension.as_deref().expect("extension checked"),
            ctx.input["id"].as_str(),
            ctx.input["constraint"].as_str(),
            &options.providers,
        )?);
    }
    if operation == "ext remove" {
        let id = extension.as_deref().expect("extension checked");
        let selected = config
            .cli
            .extensions
            .iter_mut()
            .find(|entry| entry.id == id && entry.direct)
            .ok_or_else(|| Error::Invalid(format!("{id} is not a direct selection")))?;
        selected.direct = false;
        selected.version = None;
        config.cli.extensions.retain(|entry| {
            entry.direct
                || config
                    .cli
                    .presets
                    .values()
                    .any(|preset| preset.extensions.contains_key(&entry.id))
        });
    }
    if operation == "preset remove" {
        let reference = reference
            .as_deref()
            .ok_or_else(|| Error::Invalid("preset remove requires an identity".into()))?;
        let identity = if reference.starts_with('@') {
            reference.to_owned()
        } else {
            format!("@lenso/{reference}")
        };
        if config.cli.presets.remove(&identity).is_none() {
            return Err(Error::Invalid(format!("preset {identity} is not selected")));
        }
        config.cli.extensions.retain(|entry| {
            entry.direct
                || config
                    .cli
                    .presets
                    .values()
                    .any(|preset| preset.extensions.contains_key(&entry.id))
        });
    }
    let mut new_packages = Vec::new();
    for installed in added {
        if let Some(existing) = config
            .cli
            .extensions
            .iter_mut()
            .find(|entry| entry.id == installed.id)
        {
            if existing.source != installed.source {
                return Err(Error::Invalid(format!(
                    "{} is already selected from a different source",
                    installed.id
                )));
            }
            if installed.direct {
                existing.direct = true;
                existing.version = installed.version;
            }
        } else {
            config.cli.extensions.push(installed.clone());
            new_packages.push(installed);
        }
    }
    let added = if operation == "ext configure" {
        let id = extension.as_deref().expect("extension checked");
        vec![
            config
                .cli
                .extensions
                .iter()
                .find(|installed| installed.id == id)
                .cloned()
                .ok_or_else(|| Error::Invalid(format!("{id} is not installed")))?,
        ]
    } else {
        new_packages
    };
    if let Some((id, selection)) = selection {
        if let Some(previous) = config.cli.presets.get(&id)
            && previous.version == selection.version
            && previous.sha256 != selection.sha256
        {
            return Err(Error::Permission(format!(
                "preset {id} {} changed its composition digest; use a new preset version",
                selection.version
            )));
        }
        config.cli.presets.insert(id, selection);
    }
    let resolve_root = root.to_owned();
    let resolve_config = config.clone();
    let resolve_options = options.clone();
    let mut lock = tokio::task::spawn_blocking(move || {
        crate::dependencies::resolve(&resolve_root, &resolve_config, &resolve_options)
    })
    .await
    .map_err(|e| Error::Operation(e.to_string()))??;
    if creating {
        lock.resources.clear();
    }
    let nodes=lock.extensions.iter().map(|entry|json!({"id":entry.id,"version":entry.definition.version,"source":entry.source,"dependencies":entry.definition.dependencies})).collect::<Vec<_>>();
    let tree = lock
        .extensions
        .iter()
        .map(|entry| {
            let dependencies = entry
                .definition
                .dependencies
                .iter()
                .map(|(id, dependency)| format!("    -> {id} {}", dependency.version))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{} {} ({}){}{}",
                entry.id,
                entry.definition.version,
                entry.source,
                if dependencies.is_empty() { "" } else { "\n" },
                dependencies
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if operation == "ext tree" {
        return Ok(Outcome::complete(
            json!({"message":tree,"extensions":nodes}),
        ));
    }
    for entry in &lock.extensions {
        if operation != "ext configure"
            && added.iter().any(|installation| installation.id == entry.id)
            && let Some(setup) = &entry.definition.setup
        {
            let installed = config
                .cli
                .extensions
                .iter_mut()
                .find(|installation| installation.id == entry.id)
                .expect("added entry exists");
            let mut value = serde_json::to_value(&*installed)?;
            for (key, default) in &setup.defaults {
                if matches!(
                    key.as_str(),
                    "id" | "source" | "version" | "enabled" | "direct"
                ) {
                    return Err(Error::Invalid(
                        "setup defaults cannot change package identity".into(),
                    ));
                }
                value[key] = default.clone();
            }
            *installed = serde_json::from_value(value)?;
        }
    }
    if let Some(recipe) = &recipe
        && !lock.extensions.iter().any(|entry| {
            entry
                .definition
                .workflows
                .iter()
                .any(|workflow| workflow.id == *recipe)
        })
    {
        return Err(Error::Invalid(format!(
            "preset recipe {recipe} is not provided by its extensions"
        )));
    }
    if operation == "ext configure"
        && !lock.extensions.iter().any(|entry| {
            Some(entry.id.as_str()) == extension.as_deref()
                && entry
                    .definition
                    .setup
                    .as_ref()
                    .is_some_and(|setup| setup.configure.is_some())
        })
    {
        return Err(Error::Invalid(
            "installed extension has no configuration workflow".into(),
        ));
    }
    let mut check = Cli::new("lenso");
    crate::register_management_with_options(&mut check, &options)?;
    crate::manifest::register_locked(&mut check, root, &config, &lock, &options)?;
    let bytes = toml::to_string_pretty(&config).map_err(|e| Error::Invalid(e.to_string()))?;
    let approval = crate::cache::digest(&serde_json::to_vec(
        &json!({"target":target,"previous":old,"config":config,"lock":lock,"preset":preset_identity}),
    )?);
    if operation != "ext configure"
        && let Some(outcome) = question(
            &ctx,
            &format!("configure:{approval}"),
            format!(
                "Project: {}\n{}\nExtensions:\n{}\nApprove this configuration and its explicit grants?",
                target.display(),
                bytes,
                tree
            ),
            QuestionKind::Confirm,
        )?
    {
        return Ok(outcome);
    }
    if creating {
        if lock
            .extensions
            .iter()
            .any(|entry| !entry.source.contains(':'))
        {
            return Err(Error::Invalid(
                "new project presets must use remotely distributed or native extensions".into(),
            ));
        }
        std::fs::create_dir_all(&target)?;
        target = target.canonicalize()?;
        ctx.project = target.clone();
    }
    if let Some(outcome) = configure(&ctx, &mut config, &lock, &options, &added).await? {
        return Ok(outcome);
    }
    let bytes = toml::to_string_pretty(&config).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut execution = Cli::new("lenso");
    crate::register_management_with_options(&mut execution, &options)?;
    crate::manifest::register_locked(&mut execution, &target, &config, &lock, &options)?;
    if let Some(recipe) = recipe
        && let outcome @ Outcome::NeedsInput { .. } =
            workflow(&ctx, &mut execution, &recipe).await?
    {
        return Ok(outcome);
    }
    let _guard = crate::project::resolution_guard(&target, &config, &options)?;
    let current = read_project(&target)?;
    if serde_json::to_value(&current)? != serde_json::to_value(&old)? {
        return Err(Error::Operation(
            "project configuration changed during review; retry".into(),
        ));
    }
    let config_path = crate::project_path(&target, Path::new("lenso.toml"))?;
    let previous = std::fs::read(&config_path).ok();
    atomic_write(&config_path, bytes.as_bytes())?;
    if let Err(error) = crate::project::save_lock(&target, &lock, &options) {
        if let Some(previous) = previous {
            atomic_write(&config_path, &previous)?;
        } else {
            std::fs::remove_file(&config_path)?;
        }
        return Err(error);
    }
    Ok(Outcome::complete(
        json!({"message":format!("Configured {} with {} extension(s)",target.display(),nodes.len()),"project":target,"extensions":nodes,"preset":preset_identity,"config":"lenso.toml","lock":"lenso.lock"}),
    ))
}
