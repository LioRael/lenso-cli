use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone)]
pub struct RemoteModuleInstallOptions {
    pub allow_incompatible: bool,
    pub base_url: Option<String>,
    pub console_plan: bool,
    pub dry_run: bool,
    pub env_file: Option<PathBuf>,
    pub install_profiles: Vec<String>,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub run_install_commands: bool,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ModuleReleaseInspectOptions {
    pub base_url: Option<String>,
    pub check: bool,
    pub json: bool,
    pub repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct LinkedInstallProfileEffects {
    env: Vec<(String, String)>,
    runtime_config_defaults: Vec<RuntimeConfigDefault>,
}

#[derive(Debug, Clone, PartialEq)]
struct RuntimeConfigDefault {
    service: String,
    key: String,
    value: Value,
}

#[derive(Debug, Clone)]
pub struct RemoteModuleUninstallOptions {
    pub dry_run: bool,
    pub env_file: Option<PathBuf>,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleUpdateOptions {
    pub allow_incompatible: bool,
    pub base_url: Option<String>,
    pub console_plan: bool,
    pub dry_run: bool,
    pub env_file: Option<PathBuf>,
    pub install_profiles: Vec<String>,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub run_install_commands: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleDoctorOptions {
    pub env_file: Option<PathBuf>,
    pub json: bool,
    pub module_name: Option<String>,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ServiceManifestCheckOptions {
    pub cwd: Option<PathBuf>,
    pub json: bool,
    pub manifest_url: Option<String>,
    pub operation: Option<String>,
    pub ready_timeout_ms: u64,
    pub ready_url: Option<String>,
    pub sample_input: Option<PathBuf>,
    pub serve_command: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ServiceDiffOptions {
    pub json: bool,
    pub manifest_reference: String,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ServiceUpgradeOptions {
    pub allow_incompatible: bool,
    pub base_url: Option<String>,
    pub dry_run: bool,
    pub env_file: Option<PathBuf>,
    pub manifest_reference: String,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ServiceRollbackOptions {
    pub dry_run: bool,
    pub env_file: Option<PathBuf>,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ModuleServiceListOptions {
    pub json: bool,
    pub module_name: Option<String>,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ModuleServiceExportOptions {
    pub format: String,
    pub module_name: String,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ModuleServiceStatusOptions {
    pub json: bool,
    pub module_name: String,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ModuleServiceLogsOptions {
    pub module_name: String,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
    pub tail: usize,
}

#[derive(Debug, Clone)]
pub struct ModuleServiceStartOptions {
    pub module_name: String,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ModuleServiceStopOptions {
    pub module_name: String,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ModuleCatalogAddOptions {
    pub base_url: Option<String>,
    pub catalog_file: Option<PathBuf>,
    pub dry_run: bool,
    pub repo_root: Option<PathBuf>,
    pub summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ModuleCreateOptions {
    pub area: Option<String>,
    pub capability: Option<String>,
    pub dry_run: bool,
    pub icon: Option<String>,
    pub label: Option<String>,
    pub module_id: String,
    pub output_dir: Option<PathBuf>,
    pub package_name: Option<String>,
    pub package_root: Option<String>,
    pub package_scope: Option<String>,
    pub package_slug: Option<String>,
    pub remote: bool,
    pub repo_root: Option<PathBuf>,
    pub route: Option<String>,
    pub runtime_console_root: Option<PathBuf>,
    pub source: Option<String>,
    pub surface_name: Option<String>,
    pub with_console: bool,
}

#[derive(Debug, Clone)]
pub struct ConsolePackageCreateOptions {
    pub area: Option<String>,
    pub capability: Option<String>,
    pub dry_run: bool,
    pub icon: Option<String>,
    pub label: Option<String>,
    pub module_id: String,
    pub package_name: Option<String>,
    pub package_scope: Option<String>,
    pub package_slug: Option<String>,
    pub route: Option<String>,
    pub runtime_console_root: Option<PathBuf>,
    pub source: Option<String>,
    pub surface_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConsolePackageApplyPlanOptions {
    pub dependency_version: Option<String>,
    pub dry_run: bool,
    pub install_plan_file: Option<PathBuf>,
    pub log_next_steps: bool,
    pub repo_root: Option<PathBuf>,
    pub runtime_console_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct AppliedConsolePlan;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleSource {
    Linked,
    Remote,
}

#[derive(Debug, Clone)]
struct ConsolePackageContext {
    area: String,
    capability: String,
    component_name: String,
    icon: String,
    label: String,
    manifest_name: String,
    module_id: String,
    module_name: String,
    package_dir: PathBuf,
    package_name: String,
    package_private: bool,
    package_slug: String,
    registry_source: String,
    route: String,
    runtime_console_api_version: String,
    surface_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallCommandSpec {
    command: String,
    cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteModuleServiceInstallSpec {
    name: String,
    command: String,
    cwd: Option<String>,
    ready_url: String,
    ready_timeout_ms: u64,
    auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteModuleServiceState {
    module_name: String,
    services: Vec<RemoteModuleServiceInstallSpec>,
}

struct RemoteUninstallTarget {
    provider_name: String,
    module_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteModuleServiceDoctorStatus {
    Ready,
    Disabled,
    ManualNotReady,
    NotConfigured,
    NotReady,
    StaleState,
}

impl RemoteModuleServiceDoctorStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Disabled => "disabled",
            Self::ManualNotReady => "service_not_ready",
            Self::NotConfigured => "source_not_configured",
            Self::NotReady => "service_not_ready",
            Self::StaleState => "stale_lock_or_pid",
        }
    }

    fn is_issue(self) -> bool {
        matches!(
            self,
            Self::ManualNotReady | Self::NotConfigured | Self::NotReady | Self::StaleState
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleDoctorReport {
    issue_count: usize,
    sources_checked: usize,
    services_checked: usize,
    sources: Vec<ModuleDoctorSourceReport>,
    services: Vec<ModuleDoctorServiceReport>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleDoctorSourceReport {
    module_name: String,
    installed: bool,
    configured: bool,
    enabled: bool,
    base_url: Option<String>,
    manifest_url: Option<String>,
    manifest_status: ModuleDoctorManifestStatus,
    fix: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ModuleDoctorManifestStatus {
    Reachable,
    Unreachable,
    Skipped,
    NotConfigured,
}

impl ModuleDoctorManifestStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Reachable => "reachable",
            Self::Unreachable => "unreachable",
            Self::Skipped => "skipped",
            Self::NotConfigured => "not_configured",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleDoctorServiceReport {
    module_name: String,
    service_name: String,
    status: String,
    ready_url: String,
    process: String,
    command: Option<String>,
    lock_file: Option<String>,
    pid_file: Option<String>,
    fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleServiceListReport {
    services: Vec<ModuleServiceListItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleServiceListItem {
    module_name: String,
    service_name: String,
    auto_start: bool,
    command: String,
    ready_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleServiceStatusReport {
    module_name: String,
    service_name: String,
    status: String,
    ready: bool,
    ready_url: String,
    auto_start: bool,
    lock_file: Option<String>,
    pid_file: Option<String>,
}

#[derive(Debug)]
struct RepoPaths {
    lenso_bootstrap_cargo_toml_path: PathBuf,
    lenso_bootstrap_lib_path: PathBuf,
    cargo_toml_path: PathBuf,
}

type PendingWrites = BTreeMap<PathBuf, String>;

const MODULE_CATALOG_PATH: &str = ".lenso/module-catalog.json";
const MODULE_INSTALL_LEDGER_PATH: &str = ".lenso/module-installs.json";
const CONSOLE_EXTENSION_REGISTRY_PATH: &str = ".lenso/console/extensions/registry.json";
const RUNTIME_CONFIG_DEFAULTS_PATH: &str = ".lenso/runtime-config-defaults.json";
const CONSOLE_EXTENSION_ROUTE_PREFIX: &str = "/console/extensions";
const CONSOLE_BUNDLE_HOST_API: &str = "1";
const REMOTE_PROTOCOL_VERSION: &str = "1";
const SUPPORTED_SERVICE_MODULE_FEATURES: &[&str] = &[
    "console.package-api.1",
    "service.lifecycle",
    "service.status",
];

pub async fn create_module(options: ModuleCreateOptions) -> Result<()> {
    if options.remote {
        return create_remote_module(options).await;
    }

    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let module_id = slugify(&options.module_id);
    if module_id.is_empty() {
        bail!("Module id is required");
    }
    let module_crate = snake_case(&module_id);
    let host_layout = is_starter_host_root(&repo_root);
    let module_dir = if host_layout {
        repo_root.join("src/modules").join(&module_crate)
    } else {
        repo_root.join("modules").join(&module_id)
    };
    if module_dir.exists() {
        let module_path = display_relative(&repo_root, &module_dir);
        bail!("Module directory already exists: {module_path}");
    }

    let runtime_console_root = options
        .runtime_console_root
        .clone()
        .unwrap_or_else(|| repo_root.join("apps/runtime-console"));
    let runtime_console_root = absolutize(&runtime_console_root)?;
    let console_surface = if options.with_console {
        let context = build_console_package_context(
            ConsolePackageBuildInput::from_module_options(&options),
            &runtime_console_root,
        )?;
        if context.package_dir.exists() {
            bail!(
                "Console package directory already exists: {}",
                display_relative(&runtime_console_root, &context.package_dir)
            );
        }
        Some(context)
    } else {
        None
    };

    let mut pending_writes = PendingWrites::new();
    if host_layout {
        queue_host_module_files(
            &mut pending_writes,
            &module_dir,
            &module_id,
            console_surface.as_ref(),
        )?;
        update_host_modules_mod(
            &mut pending_writes,
            &repo_root.join("src/modules/mod.rs"),
            &module_crate,
        )?;
        update_host_lib_for_created_module(
            &mut pending_writes,
            &repo_root.join("src/lib.rs"),
            &module_crate,
        )?;
    } else if is_framework_workspace_root(&repo_root) {
        let paths = repo_paths(&repo_root);
        queue_module_files(
            &mut pending_writes,
            &module_dir,
            &module_id,
            console_surface.as_ref(),
        )?;
        update_workspace_cargo_toml(&mut pending_writes, &paths.cargo_toml_path, &module_id)?;
        update_lenso_bootstrap_cargo_toml(
            &mut pending_writes,
            &paths.lenso_bootstrap_cargo_toml_path,
            &module_id,
        )?;
        update_lenso_bootstrap_lib(
            &mut pending_writes,
            &paths.lenso_bootstrap_lib_path,
            &module_crate,
            &module_id,
        )?;
    } else {
        bail!("Could not find a Lenso framework workspace or starter host root");
    }

    if let Some(console_surface) = console_surface.as_ref() {
        queue_console_package(
            &mut pending_writes,
            &runtime_console_root,
            console_surface,
            true,
        )?;
    }

    if options.dry_run {
        println!("Module dry run:");
        for file_path in pending_writes.keys() {
            println!("- {}", display_relative(&repo_root, file_path));
        }
        return Ok(());
    }

    write_pending_files(&pending_writes)?;

    println!("Created module {module_id}.");
    if let Some(console_surface) = console_surface {
        println!("Created {}.", console_surface.package_name);
    }
    println!("Next steps:");
    if host_layout {
        println!("- cargo test --locked");
        println!("- cargo run --bin migrate");
    } else {
        println!("- cargo test --locked -p {module_crate}");
        println!("- just rust-check");
        println!("- just arch-check");
    }

    Ok(())
}

pub async fn create_console_package(options: ConsolePackageCreateOptions) -> Result<()> {
    let runtime_console_root = options
        .runtime_console_root
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir().context("resolve current directory")?);
    let runtime_console_root = absolutize(&runtime_console_root)?;
    let context = build_console_package_context(
        ConsolePackageBuildInput::from_console_package_options(&options),
        &runtime_console_root,
    )?;

    if context.package_dir.exists() {
        bail!(
            "Console package directory already exists: {}",
            display_relative(&runtime_console_root, &context.package_dir)
        );
    }

    let mut pending_writes = PendingWrites::new();
    queue_console_package(&mut pending_writes, &runtime_console_root, &context, true)?;

    if options.dry_run {
        println!("Console package dry run:");
        for file_path in pending_writes.keys() {
            println!("- {}", display_relative(&runtime_console_root, file_path));
        }
        return Ok(());
    }

    write_pending_files(&pending_writes)?;

    println!("Created {}.", context.package_name);
    println!("Next steps:");
    println!(
        "- Copy {}/console-surface.rs into the Rust module manifest",
        context.package_slug
    );
    println!(
        "- Keep navigation.workspace.id=\"{}\" so the module owns its workspace",
        context.module_id
    );
    println!("- Omit navigation only for host System surfaces");
    println!("- pnpm install --lockfile-only");
    println!("- pnpm check:console-packages");
    println!("- pnpm check");

    Ok(())
}

pub async fn install_module(
    module_reference: &str,
    options: RemoteModuleInstallOptions,
) -> Result<()> {
    let source = parse_module_source(&options.source)?;
    if let Some(descriptor) = read_install_descriptor(module_reference).await? {
        if is_module_release_descriptor(&descriptor) {
            return install_module_descriptor(&descriptor, module_reference, options).await;
        }
        if should_resolve_service_catalog_entry(source)
            && let Some(manifest_reference) = catalog_service_manifest_reference(&descriptor)
        {
            return add_remote_module(manifest_reference, options).await;
        }
        return install_module_descriptor(&descriptor, module_reference, options).await;
    }

    if should_resolve_service_catalog_entry(source)
        && !looks_like_json_reference(module_reference)
        && let Some((descriptor_reference, descriptor)) =
            local_catalog_module_release_descriptor(module_reference, options.repo_root.as_deref())?
    {
        return install_module_descriptor(&descriptor, &descriptor_reference, options).await;
    }

    if should_resolve_service_catalog_entry(source)
        && !looks_like_json_reference(module_reference)
        && let Some(manifest_reference) = local_catalog_service_manifest_reference(
            module_reference,
            options.repo_root.as_deref(),
        )?
    {
        return add_remote_module(&manifest_reference, options).await;
    }

    match source {
        ModuleSource::Remote => add_remote_module(module_reference, options).await,
        ModuleSource::Linked => install_linked_module(module_reference, options),
    }
}

fn should_resolve_service_catalog_entry(source: ModuleSource) -> bool {
    matches!(source, ModuleSource::Remote)
}

fn catalog_service_manifest_reference(entry: &Value) -> Option<&str> {
    if catalog_entry_is_module_release(entry) {
        return catalog_module_release_service_reference(entry);
    }
    entry
        .get("serviceManifest")
        .or_else(|| entry.get("service_manifest"))
        .or_else(|| {
            catalog_entry_is_service(entry)
                .then(|| entry.get("manifestReference"))
                .flatten()
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn catalog_service_manifest_reference_for_module<'a>(
    entry: &'a Value,
    module_name: &str,
) -> Option<&'a str> {
    let entry_name_matches = entry.get("name").and_then(Value::as_str) == Some(module_name);
    let provided_module_matches = catalog_entry_is_service(entry)
        && entry
            .get("modules")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|module| module.get("name").and_then(Value::as_str) == Some(module_name));

    (entry_name_matches || provided_module_matches)
        .then(|| catalog_service_manifest_reference(entry))
        .flatten()
}

fn catalog_entry_is_service(entry: &Value) -> bool {
    entry
        .get("source")
        .and_then(Value::as_str)
        .is_some_and(|source| source.eq_ignore_ascii_case("service"))
}

fn catalog_entry_is_module_release(entry: &Value) -> bool {
    entry.get("protocol").and_then(Value::as_str) == Some("lenso.module-release.v1")
}

fn catalog_module_release_service_reference(entry: &Value) -> Option<&str> {
    let provider = entry.get("provider").unwrap_or(entry);
    provider
        .get("servicePackage")
        .or_else(|| provider.get("service_package"))
        .or_else(|| provider.get("serviceManifest"))
        .or_else(|| provider.get("service_manifest"))
        .or_else(|| provider.get("manifestReference"))
        .or_else(|| provider.get("manifest_reference"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn local_catalog_module_release_descriptor(
    module_name: &str,
    repo_root: Option<&Path>,
) -> Result<Option<(String, Value)>> {
    let repo_root = resolve_repo_root(repo_root)?;
    let Some(catalog) = read_json_if_exists(&repo_root.join(MODULE_CATALOG_PATH))? else {
        return Ok(None);
    };
    let modules = catalog
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Module catalog modules must be an array"))?;
    Ok(modules
        .iter()
        .find(|entry| {
            catalog_entry_is_module_release(entry)
                && entry.get("name").and_then(Value::as_str) == Some(module_name)
        })
        .map(|entry| {
            let reference = entry
                .get("manifestReference")
                .and_then(Value::as_str)
                .unwrap_or(module_name)
                .to_owned();
            (reference, entry.clone())
        }))
}

fn local_catalog_service_manifest_reference(
    module_name: &str,
    repo_root: Option<&Path>,
) -> Result<Option<String>> {
    let repo_root = resolve_repo_root(repo_root)?;
    let Some(catalog) = read_json_if_exists(&repo_root.join(MODULE_CATALOG_PATH))? else {
        return Ok(None);
    };
    let modules = catalog
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Module catalog modules must be an array"))?;
    Ok(modules
        .iter()
        .find_map(|entry| catalog_service_manifest_reference_for_module(entry, module_name))
        .map(ToOwned::to_owned))
}

async fn install_module_descriptor(
    descriptor: &Value,
    descriptor_reference: &str,
    options: RemoteModuleInstallOptions,
) -> Result<()> {
    if is_module_release_descriptor(descriptor) {
        return install_module_release_descriptor(descriptor, descriptor_reference, options).await;
    }
    match parse_module_source(string_field(descriptor, "source")?)? {
        ModuleSource::Remote => {
            let manifest_reference = descriptor
                .get("remote")
                .and_then(|remote| {
                    remote
                        .get("manifest_url")
                        .or_else(|| remote.get("manifestUrl"))
                })
                .and_then(Value::as_str)
                .unwrap_or(descriptor_reference);
            add_remote_module(manifest_reference, options).await
        }
        ModuleSource::Linked => {
            install_linked_module_descriptor(descriptor, descriptor_reference, options).await
        }
    }
}

async fn install_module_release_descriptor(
    descriptor: &Value,
    descriptor_reference: &str,
    mut options: RemoteModuleInstallOptions,
) -> Result<()> {
    let release = validate_module_release_descriptor(descriptor.clone())?;
    let source = module_release_source(&release)?;
    if source == "linked" || source == "bundled" {
        if release.get("linked").is_some() {
            return install_linked_module_descriptor(&release, descriptor_reference, options).await;
        }
        let module_name = string_field(&release, "name")?.trim().to_owned();
        return install_linked_module(&module_name, options);
    }
    if options.base_url.is_none() {
        options.base_url = descriptor
            .get("baseUrl")
            .or_else(|| descriptor.get("base_url"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
    }
    let service_reference = module_release_service_reference(descriptor_reference, &release)?;
    let release_context = ModuleReleaseInstallContext {
        manifest: release,
        reference: descriptor_reference.to_owned(),
    };
    add_remote_module_with_context(&service_reference, options, Some(&release_context)).await
}

pub async fn inspect_module_release(
    release_reference: &str,
    options: ModuleReleaseInspectOptions,
) -> Result<()> {
    let (descriptor_reference, descriptor) =
        read_module_release_descriptor_for_inspect(release_reference, options.repo_root.as_deref())
            .await?;
    let release = validate_module_release_descriptor(descriptor)?;
    let source = module_release_source(&release)?;
    let provider = release.get("provider").and_then(Value::as_object);
    let provider_name = provider.and_then(|provider| {
        provider
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    });
    let service_package = provider.and_then(|provider| {
        optional_provider_string(provider, "servicePackage", "service_package")
    });
    let service_manifest = provider.and_then(|provider| {
        optional_provider_string(provider, "serviceManifest", "service_manifest")
    });
    let service_reference = if source == "service" {
        Some(module_release_service_reference(
            &descriptor_reference,
            &release,
        )?)
    } else {
        None
    };
    let base_url = options
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            release
                .get("baseUrl")
                .or_else(|| release.get("base_url"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        });
    let issues = module_release_inspect_issues(service_reference.as_deref(), base_url.as_deref());
    let status = if issues.is_empty() {
        "ready"
    } else {
        "needs_attention"
    };
    let install_command =
        module_release_install_command(&descriptor_reference, base_url.as_deref());
    let catalog_command =
        module_release_catalog_command(&descriptor_reference, base_url.as_deref());

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "baseUrl": base_url,
                "catalogCommand": catalog_command,
                "installCommand": install_command,
                "issues": &issues,
                "name": string_field(&release, "name")?.trim(),
                "source": source,
                "provider": {
                    "name": provider_name,
                    "serviceManifest": service_manifest,
                    "servicePackage": service_package,
                },
                "releaseReference": descriptor_reference,
                "serviceReference": service_reference,
                "status": status,
                "version": string_field(&release, "version")?.trim(),
            }))
            .context("serialize module release inspect report")?
        );
    } else {
        println!(
            "Module release {}@{}",
            string_field(&release, "name")?.trim(),
            string_field(&release, "version")?.trim()
        );
        println!("- status: {status}");
        println!("- release: {descriptor_reference}");
        println!("- source: {source}");
        if let Some(provider_name) = provider_name {
            println!("- provider: {provider_name}");
        }
        if let Some(service_package) = service_package {
            println!("- service package: {service_package}");
        }
        if let Some(service_manifest) = service_manifest {
            println!("- service manifest: {service_manifest}");
        }
        if let Some(service_reference) = service_reference {
            println!("- service reference: {service_reference}");
            println!(
                "- base URL: {}",
                base_url
                    .as_deref()
                    .unwrap_or("<required for local artifacts>")
            );
        }
        println!("- install: {install_command}");
        println!("- catalog: {catalog_command}");
        if !issues.is_empty() {
            println!("Issues:");
            for issue in &issues {
                println!("- {issue}");
            }
        }
    }

    if options.check && !issues.is_empty() {
        bail!("Module release check failed: {}", issues.join("; "));
    }
    Ok(())
}

async fn read_module_release_descriptor_for_inspect(
    release_reference: &str,
    repo_root: Option<&Path>,
) -> Result<(String, Value)> {
    if let Some(descriptor) = read_install_descriptor(release_reference).await? {
        return Ok((release_reference.to_owned(), descriptor));
    }
    if !looks_like_json_reference(release_reference)
        && let Some((descriptor_reference, descriptor)) =
            local_catalog_module_release_descriptor(release_reference, repo_root)?
    {
        return Ok((descriptor_reference, descriptor));
    }
    bail!(
        "Module release `{release_reference}` was not found as a file, URL, or local catalog entry"
    );
}

fn module_release_inspect_issues(
    service_reference: Option<&str>,
    base_url: Option<&str>,
) -> Vec<String> {
    let mut issues = Vec::new();
    if let Some(service_reference) = service_reference
        && base_url.is_none()
        && !http_manifest_reference(service_reference)
    {
        issues.push(
            "installing this release needs --base-url because its service reference is not an HTTP /manifest URL"
                .to_owned(),
        );
    }
    issues
}

fn module_release_install_command(release_reference: &str, base_url: Option<&str>) -> String {
    format!(
        "lenso module install {release_reference}{}",
        base_url
            .map(|base_url| format!(" --base-url {base_url}"))
            .unwrap_or_default()
    )
}

fn module_release_catalog_command(release_reference: &str, base_url: Option<&str>) -> String {
    format!(
        "lenso module catalog add {release_reference}{}",
        base_url
            .map(|base_url| format!(" --base-url {base_url}"))
            .unwrap_or_default()
    )
}

fn http_manifest_reference(reference: &str) -> bool {
    (reference.starts_with("http://") || reference.starts_with("https://"))
        && reference.trim_end_matches('/').ends_with("/manifest")
}

fn optional_provider_string<'a>(
    provider: &'a Map<String, Value>,
    camel_field: &str,
    snake_field: &str,
) -> Option<&'a str> {
    provider
        .get(camel_field)
        .or_else(|| provider.get(snake_field))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

pub async fn update_module(module_name: &str, options: ModuleUpdateOptions) -> Result<()> {
    let module_name = module_name.trim();
    if module_name.is_empty() {
        bail!("Module name is required");
    }
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let receipt = module_install_ledger_entry(&ledger_path, module_name)?
        .ok_or_else(|| anyhow!("Module `{module_name}` is not installed locally"))?;
    let manifest_reference = receipt
        .get("manifestReference")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Module install receipt manifestReference is required"))?;
    let source = receipt
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Module install receipt source is required"))
        .and_then(parse_module_source)?;

    match source {
        ModuleSource::Remote => {
            update_remote_module_from_receipt(
                module_name,
                manifest_reference,
                &receipt,
                options,
                repo_root,
            )
            .await
        }
        ModuleSource::Linked => {
            update_linked_module_from_receipt(module_name, manifest_reference, options, repo_root)
                .await
        }
    }
}

async fn update_remote_module_from_receipt(
    module_name: &str,
    manifest_reference: &str,
    receipt: &Value,
    options: ModuleUpdateOptions,
    repo_root: PathBuf,
) -> Result<()> {
    let manifest = read_json_reference(manifest_reference).await?;
    if is_service_manifest(&manifest) {
        let manifest = validate_service_manifest(manifest)?;
        let manifest_service_name = string_field(&manifest, "name")?.trim();
        let receipt_service_name = receipt
            .get("service")
            .and_then(|service| service.get("name"))
            .and_then(Value::as_str)
            .unwrap_or(manifest_service_name);
        if manifest_service_name != receipt_service_name {
            bail!(
                "Installed module `{module_name}` update resolved service `{manifest_service_name}`"
            );
        }
        return add_remote_module(
            manifest_reference,
            RemoteModuleInstallOptions {
                allow_incompatible: options.allow_incompatible,
                base_url: options
                    .base_url
                    .clone()
                    .or_else(|| service_receipt_base_url(receipt)),
                console_plan: options.console_plan,
                dry_run: options.dry_run,
                env_file: options.env_file,
                install_profiles: options.install_profiles,
                module_services_file: options.module_services_file,
                repo_root: Some(repo_root),
                run_install_commands: options.run_install_commands,
                source: "remote".to_owned(),
            },
        )
        .await;
    }
    let manifest = validate_remote_module_manifest(manifest)?;
    let manifest_name = string_field(&manifest, "name")?.trim();
    if manifest_name != module_name {
        bail!("Installed module `{module_name}` update resolved manifest for `{manifest_name}`");
    }

    let cleanup =
        remove_stale_module_console_artifacts(&repo_root, module_name, true, options.dry_run)?;
    if options.dry_run && !cleanup.is_empty() {
        println!("Remote module update dry run:");
        for path in cleanup {
            println!("- {}", display_relative(&repo_root, &path));
        }
    }

    add_remote_module(
        manifest_reference,
        RemoteModuleInstallOptions {
            allow_incompatible: options.allow_incompatible,
            base_url: options.base_url.clone().or_else(|| {
                receipt
                    .get("baseUrl")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            }),
            console_plan: options.console_plan,
            dry_run: options.dry_run,
            env_file: options.env_file,
            install_profiles: options.install_profiles,
            module_services_file: options.module_services_file,
            repo_root: Some(repo_root),
            run_install_commands: options.run_install_commands,
            source: "remote".to_owned(),
        },
    )
    .await
}

async fn update_linked_module_from_receipt(
    module_name: &str,
    manifest_reference: &str,
    options: ModuleUpdateOptions,
    repo_root: PathBuf,
) -> Result<()> {
    if options.base_url.is_some() {
        bail!("--base-url only applies to remote module updates");
    }

    let cleanup =
        remove_stale_module_console_artifacts(&repo_root, module_name, false, options.dry_run)?;
    if options.dry_run && !cleanup.is_empty() {
        println!("Linked module update dry run:");
        for path in cleanup {
            println!("- {}", display_relative(&repo_root, &path));
        }
    }

    install_module(
        module_update_reference(manifest_reference),
        RemoteModuleInstallOptions {
            allow_incompatible: options.allow_incompatible,
            base_url: None,
            console_plan: options.console_plan,
            dry_run: options.dry_run,
            env_file: options.env_file,
            install_profiles: options.install_profiles,
            module_services_file: options.module_services_file,
            repo_root: Some(repo_root),
            run_install_commands: options.run_install_commands,
            source: "linked".to_owned(),
        },
    )
    .await
}

pub async fn add_remote_module(
    manifest_reference: &str,
    options: RemoteModuleInstallOptions,
) -> Result<()> {
    add_remote_module_with_context(manifest_reference, options, None).await
}

async fn add_remote_module_with_context(
    manifest_reference: &str,
    options: RemoteModuleInstallOptions,
    module_release_context: Option<&ModuleReleaseInstallContext>,
) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        options
            .env_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".env")),
    );
    let console_extension_registry_path = repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH);
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let module_services_path = resolve_path(
        &repo_root,
        options
            .module_services_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    );
    let manifest = read_json_reference(manifest_reference).await?;
    if is_service_package_manifest(&manifest) {
        let package = validate_service_package_manifest(manifest)?;
        let service_manifest_reference =
            service_package_manifest_reference(manifest_reference, &package)?;
        let service_manifest =
            validate_service_manifest(read_json_reference(&service_manifest_reference).await?)?;
        ensure_service_package_matches_manifest(&package, &service_manifest)?;
        let package_context = ServicePackageInstallContext {
            manifest: package,
            reference: manifest_reference.to_owned(),
        };
        return add_service_manifest_with_paths(
            &service_manifest_reference,
            service_manifest,
            &options,
            &repo_root,
            &env_file_path,
            &console_extension_registry_path,
            &install_ledger_path,
            &module_services_path,
            Some(&package_context),
            module_release_context,
        )
        .await;
    }
    if is_service_manifest(&manifest) {
        return add_service_manifest_with_paths(
            manifest_reference,
            validate_service_manifest(manifest)?,
            &options,
            &repo_root,
            &env_file_path,
            &console_extension_registry_path,
            &install_ledger_path,
            &module_services_path,
            None,
            module_release_context,
        )
        .await;
    }
    let manifest = validate_remote_module_manifest(manifest)?;
    if let Some(issue) = remote_module_manifest_compatibility_issue(&manifest)
        && !options.allow_incompatible
    {
        bail!("{issue}; rerun with --allow-incompatible to record an operator override");
    }
    let module_name = string_field(&manifest, "name")?.trim().to_owned();
    let base_url = derive_remote_base_url(options.base_url.as_deref(), manifest_reference)?;
    let install_env = remote_module_install_env(&manifest)?;
    let install_commands = remote_module_install_commands(&manifest)?;
    let install_services = remote_module_install_services(&manifest, &module_name, &base_url)?;
    let env_file = apply_manifest_install_env(
        update_remote_modules_env(&env_file_path, &module_name, &base_url)?,
        &install_env,
    );
    let console_bundle_install = install_runtime_console_bundles(
        &repo_root,
        &console_extension_registry_path,
        &manifest,
        &base_url,
        options.console_plan,
        options.dry_run,
    )
    .await?;
    let module_services =
        update_remote_module_services_file(&module_services_path, &module_name, &install_services)?;
    let install_ledger = update_module_install_ledger(
        &install_ledger_path,
        remote_module_install_ledger_entry(
            &module_name,
            manifest_reference,
            &base_url,
            &manifest,
            remote_module_install_writes(
                &repo_root,
                &env_file_path,
                console_bundle_install
                    .registry_changed
                    .then_some(console_extension_registry_path.as_path()),
                module_services
                    .as_ref()
                    .map(|_| module_services_path.as_path()),
            ),
            &install_env,
            &install_commands,
            &install_services,
            console_bundle_install.bundle_count,
        ),
    )?;

    if options.dry_run {
        println!("Module install dry run:");
        println!("- {}", display_relative(&repo_root, &env_file_path));
        if console_bundle_install.registry_changed {
            println!(
                "- {}",
                display_relative(&repo_root, &console_extension_registry_path)
            );
            for file_path in &console_bundle_install.bundle_files {
                println!("- {}", display_relative(&repo_root, file_path));
            }
        }
        println!("- {}", display_relative(&repo_root, &install_ledger_path));
        if module_services.is_some() {
            println!("- {}", display_relative(&repo_root, &module_services_path));
        }
        println!("- {module_name}={base_url}");
        println!("- install env vars: {}", install_env.len());
        println!("- install commands: {}", install_commands.len());
        println!("- install services: {}", install_services.len());
        println!("- console bundles: {}", console_bundle_install.bundle_count);
        return Ok(());
    }

    write_file(&env_file_path, env_file.as_bytes())?;
    write_json(&install_ledger_path, &install_ledger)?;
    if let Some(module_services) = &module_services {
        write_json(&module_services_path, module_services)?;
    }

    println!("Installed module {module_name}.");
    println!("Updated:");
    println!("- {}", display_relative(&repo_root, &env_file_path));
    if console_bundle_install.registry_changed {
        println!(
            "- {}",
            display_relative(&repo_root, &console_extension_registry_path)
        );
        for file_path in &console_bundle_install.bundle_files {
            println!("- {}", display_relative(&repo_root, file_path));
        }
    }
    println!("- {}", display_relative(&repo_root, &install_ledger_path));
    if module_services.is_some() {
        println!("- {}", display_relative(&repo_root, &module_services_path));
    }
    println!("REMOTE_MODULES: {module_name}={base_url}");
    println!("Install env vars: {}", install_env.len());
    println!("Install commands: {}", install_commands.len());
    println!("Install services: {}", install_services.len());
    println!("Console bundles: {}", console_bundle_install.bundle_count);

    let install_commands_ran = if !install_commands.is_empty() && options.run_install_commands {
        run_install_commands(&repo_root, &install_commands)?;
        true
    } else {
        false
    };

    println!("Next steps:");
    if !install_commands.is_empty() && !install_commands_ran {
        println!("- rerun with --run-install-commands to execute manifest install commands");
    }
    println!("- restart the API and worker");

    Ok(())
}

async fn add_service_manifest_with_options(
    manifest_reference: &str,
    manifest: Value,
    options: &RemoteModuleInstallOptions,
    package_context: Option<&ServicePackageInstallContext>,
    module_release_context: Option<&ModuleReleaseInstallContext>,
) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        options
            .env_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".env")),
    );
    let console_extension_registry_path = repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH);
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let module_services_path = resolve_path(
        &repo_root,
        options
            .module_services_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    );
    add_service_manifest_with_paths(
        manifest_reference,
        manifest,
        options,
        &repo_root,
        &env_file_path,
        &console_extension_registry_path,
        &install_ledger_path,
        &module_services_path,
        package_context,
        module_release_context,
    )
    .await
}

async fn read_service_or_package_manifest(
    reference: &str,
) -> Result<(String, Value, Option<ServicePackageInstallContext>)> {
    let manifest = read_json_reference(reference).await?;
    if is_service_package_manifest(&manifest) {
        let package = validate_service_package_manifest(manifest)?;
        let service_manifest_reference = service_package_manifest_reference(reference, &package)?;
        let service_manifest =
            validate_service_manifest(read_json_reference(&service_manifest_reference).await?)?;
        ensure_service_package_matches_manifest(&package, &service_manifest)?;
        return Ok((
            service_manifest_reference,
            service_manifest,
            Some(ServicePackageInstallContext {
                manifest: package,
                reference: reference.to_owned(),
            }),
        ));
    }
    Ok((
        reference.to_owned(),
        validate_service_manifest(manifest)?,
        None,
    ))
}

#[derive(Debug)]
struct ServicePackageInstallContext {
    manifest: Value,
    reference: String,
}

#[derive(Debug)]
struct ModuleReleaseInstallContext {
    manifest: Value,
    reference: String,
}

async fn add_service_manifest_with_paths(
    manifest_reference: &str,
    manifest: Value,
    options: &RemoteModuleInstallOptions,
    repo_root: &Path,
    env_file_path: &Path,
    console_extension_registry_path: &Path,
    install_ledger_path: &Path,
    module_services_path: &Path,
    package_context: Option<&ServicePackageInstallContext>,
    module_release_context: Option<&ModuleReleaseInstallContext>,
) -> Result<()> {
    if let Some(issue) = remote_module_manifest_compatibility_issue(&manifest)
        && !options.allow_incompatible
    {
        bail!("{issue}; rerun with --allow-incompatible to record an operator override");
    }

    let service_name = string_field(&manifest, "name")?.trim().to_owned();
    let base_url = derive_remote_base_url(options.base_url.as_deref(), manifest_reference)?;
    let install_env = remote_module_install_env(&manifest)?;
    let install_commands = remote_module_install_commands(&manifest)?;
    let install_services = service_manifest_install_services(&manifest, &service_name, &base_url)?;
    let module_manifests =
        service_module_install_manifests(&manifest, manifest_reference, &base_url)?;
    if let Some(module_release_context) = module_release_context {
        ensure_module_release_matches_service_manifest(
            &module_release_context.manifest,
            &manifest,
        )?;
    }
    let env_file = apply_manifest_install_env(
        update_remote_modules_env(env_file_path, &service_name, &base_url)?,
        &install_env,
    );
    let module_services =
        update_remote_module_services_file(module_services_path, &service_name, &install_services)?;

    let mut console_bundle_files = Vec::new();
    let mut console_bundle_count = 0;
    let mut console_registry_changed = false;
    let mut install_ledger = read_json_if_exists(install_ledger_path)?
        .unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
    let mut module_names = Vec::new();

    for module_manifest in &module_manifests {
        let module_name = string_field(module_manifest, "name")?.trim().to_owned();
        let module_base_url = service_module_base_url(&base_url, &module_name);
        let previous_manifest_snapshot =
            module_install_ledger_entry_value(&install_ledger, &module_name)
                .and_then(|entry| entry.get("serviceManifestSnapshot").cloned());
        let console_bundle_install = install_runtime_console_bundles(
            repo_root,
            console_extension_registry_path,
            module_manifest,
            &module_base_url,
            options.console_plan,
            options.dry_run,
        )
        .await?;
        console_registry_changed |= console_bundle_install.registry_changed;
        console_bundle_count += console_bundle_install.bundle_count;
        console_bundle_files.extend(console_bundle_install.bundle_files);

        let mut entry = remote_module_install_ledger_entry(
            &module_name,
            manifest_reference,
            &module_base_url,
            module_manifest,
            remote_module_install_writes(
                repo_root,
                env_file_path,
                console_registry_changed.then_some(console_extension_registry_path),
                module_services.as_ref().map(|_| module_services_path),
            ),
            &install_env,
            &install_commands,
            &install_services,
            console_bundle_install.bundle_count,
        );
        entry["serviceManifestSnapshot"] = manifest.clone();
        if let Some(previous_manifest_snapshot) = previous_manifest_snapshot {
            entry["previousServiceManifestSnapshot"] = previous_manifest_snapshot;
        }
        if let Some(service) = entry.get_mut("service").and_then(Value::as_object_mut) {
            service.insert("baseUrl".to_owned(), json!(base_url.clone()));
            service.insert(
                "manifestReference".to_owned(),
                json!(manifest_reference.to_owned()),
            );
        }
        if let Some(package_context) = package_context {
            entry["servicePackage"] = json!({
                "manifestReference": package_context.reference.clone(),
                "manifestSnapshot": package_context.manifest.clone(),
            });
        }
        if let Some(module_release_context) = module_release_context
            && module_release_context
                .manifest
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| name == module_name)
        {
            entry["moduleRelease"] = json!({
                "manifestReference": module_release_context.reference.clone(),
                "manifestSnapshot": module_release_context.manifest.clone(),
            });
        }
        install_ledger = upsert_module_install_ledger_entry(install_ledger, entry)?;
        module_names.push(module_name);
    }

    if options.dry_run {
        println!("Service install dry run:");
        println!("- {}", display_relative(repo_root, env_file_path));
        if console_registry_changed {
            println!(
                "- {}",
                display_relative(repo_root, console_extension_registry_path)
            );
            for file_path in &console_bundle_files {
                println!("- {}", display_relative(repo_root, file_path));
            }
        }
        println!("- {}", display_relative(repo_root, install_ledger_path));
        if module_services.is_some() {
            println!("- {}", display_relative(repo_root, module_services_path));
        }
        println!("- {service_name}={base_url}");
        println!("- provided modules: {}", module_names.join(", "));
        println!("- install env vars: {}", install_env.len());
        println!("- install commands: {}", install_commands.len());
        println!("- install services: {}", install_services.len());
        println!("- console bundles: {console_bundle_count}");
        return Ok(());
    }

    write_file(env_file_path, env_file.as_bytes())?;
    write_json(install_ledger_path, &install_ledger)?;
    if let Some(module_services) = &module_services {
        write_json(module_services_path, module_services)?;
    }

    println!("Installed service {service_name}.");
    println!("Updated:");
    println!("- {}", display_relative(repo_root, env_file_path));
    if console_registry_changed {
        println!(
            "- {}",
            display_relative(repo_root, console_extension_registry_path)
        );
        for file_path in &console_bundle_files {
            println!("- {}", display_relative(repo_root, file_path));
        }
    }
    println!("- {}", display_relative(repo_root, install_ledger_path));
    if module_services.is_some() {
        println!("- {}", display_relative(repo_root, module_services_path));
    }
    println!("REMOTE_MODULES: {service_name}={base_url}");
    println!("Provided modules: {}", module_names.join(", "));
    println!("Install env vars: {}", install_env.len());
    println!("Install commands: {}", install_commands.len());
    println!("Install services: {}", install_services.len());
    println!("Console bundles: {console_bundle_count}");

    let install_commands_ran = if !install_commands.is_empty() && options.run_install_commands {
        run_install_commands(repo_root, &install_commands)?;
        true
    } else {
        false
    };

    println!("Next steps:");
    if !install_commands.is_empty() && !install_commands_ran {
        println!("- rerun with --run-install-commands to execute service install commands");
    }
    println!("- start the service process if it is not already running");
    println!("- restart the API and worker");

    Ok(())
}

fn install_linked_module(module_name: &str, options: RemoteModuleInstallOptions) -> Result<()> {
    set_linked_module_enabled(
        module_name,
        true,
        options.env_file,
        options.repo_root,
        options.dry_run,
    )
}

async fn install_linked_module_descriptor(
    descriptor: &Value,
    descriptor_reference: &str,
    options: RemoteModuleInstallOptions,
) -> Result<()> {
    let module_name = string_field(descriptor, "name")?.trim().to_owned();
    if module_name.is_empty() {
        bail!("Linked module descriptor name is required");
    }
    let (descriptor, install_profile_effects) =
        apply_linked_install_profiles(descriptor, &options.install_profiles)?;
    let linked = descriptor
        .get("linked")
        .ok_or_else(|| anyhow!("Linked module descriptor linked section is required"))?;
    let call = string_field(linked, "call")?.trim().to_owned();
    if call.is_empty() {
        bail!("Linked module descriptor linked.call is required");
    }

    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        options
            .env_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".env")),
    );
    let cargo_toml_path = repo_root.join("Cargo.toml");
    let console_extension_registry_path = repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH);
    let host_lib_path = repo_root.join("src/lib.rs");
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let runtime_config_defaults_path = repo_root.join(RUNTIME_CONFIG_DEFAULTS_PATH);

    let dependencies = descriptor
        .get("dependencies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    let mut env_file =
        set_linked_module_enabled_env(&read_text_if_exists(&env_file_path)?, &module_name, true);
    for dependency in &dependencies {
        env_file = set_linked_module_enabled_env(&env_file, dependency, true);
    }
    env_file = apply_manifest_install_env(env_file, &install_profile_effects.env);

    let runtime_config_defaults = if install_profile_effects.runtime_config_defaults.is_empty() {
        None
    } else {
        Some(update_runtime_config_defaults(
            read_json_if_exists(&runtime_config_defaults_path)?,
            &install_profile_effects.runtime_config_defaults,
        )?)
    };

    let dependency_descriptors = dependencies
        .iter()
        .filter_map(|dependency| {
            builtin_linked_module_descriptor(dependency)
                .map(|descriptor| (dependency.clone(), descriptor))
        })
        .collect::<Vec<_>>();

    let mut cargo_toml = read_text_if_exists(&cargo_toml_path)?;
    let mut cargo_toml_changed = false;
    let mut host_lib = read_text(&host_lib_path)?;
    for (_, dependency_descriptor) in &dependency_descriptors {
        let dependency_linked = dependency_descriptor
            .get("linked")
            .ok_or_else(|| anyhow!("Linked dependency descriptor linked section is required"))?;
        if let Some(updated) = update_host_cargo_toml_for_linked_descriptor(
            &cargo_toml,
            dependency_linked.get("cargo"),
        )? {
            cargo_toml = updated;
            cargo_toml_changed = true;
        }
        host_lib = update_host_lib_for_linked_descriptor(
            &host_lib,
            dependency_linked.get("use").and_then(Value::as_str),
            string_field(dependency_linked, "call")?,
        )?;
    }
    if let Some(updated) =
        update_host_cargo_toml_for_linked_descriptor(&cargo_toml, linked.get("cargo"))?
    {
        cargo_toml = updated;
        cargo_toml_changed = true;
    }
    host_lib = update_host_lib_for_linked_descriptor(
        &host_lib,
        linked.get("use").and_then(Value::as_str),
        &call,
    )?;
    let mut console_manifests = dependency_descriptors
        .iter()
        .map(|(_, descriptor)| descriptor)
        .collect::<Vec<_>>();
    console_manifests.push(&descriptor);
    let console_bundle_install = install_runtime_console_bundles_for_manifests(
        &repo_root,
        &console_extension_registry_path,
        &console_manifests,
        options.console_plan,
        options.dry_run,
    )
    .await?;
    let install_ledger = update_module_install_ledger(
        &install_ledger_path,
        linked_module_install_ledger_entry(
            &module_name,
            descriptor_reference,
            &call,
            &dependencies,
            linked_module_install_writes(
                &repo_root,
                &env_file_path,
                if cargo_toml_changed {
                    Some(cargo_toml_path.as_path())
                } else {
                    None
                },
                &host_lib_path,
                runtime_config_defaults
                    .as_ref()
                    .map(|_| runtime_config_defaults_path.as_path()),
                console_bundle_install
                    .registry_changed
                    .then_some(console_extension_registry_path.as_path()),
            ),
            cargo_toml_changed,
        ),
    )?;

    if options.dry_run {
        println!("Linked module install dry run:");
        println!("- {}", display_relative(&repo_root, &env_file_path));
        if cargo_toml_changed {
            println!("- {}", display_relative(&repo_root, &cargo_toml_path));
        }
        println!("- {}", display_relative(&repo_root, &host_lib_path));
        if console_bundle_install.registry_changed {
            println!(
                "- {}",
                display_relative(&repo_root, &console_extension_registry_path)
            );
            for file_path in &console_bundle_install.bundle_files {
                println!("- {}", display_relative(&repo_root, file_path));
            }
        }
        if runtime_config_defaults.is_some() {
            println!(
                "- {}",
                display_relative(&repo_root, &runtime_config_defaults_path)
            );
        }
        println!("- {}", display_relative(&repo_root, &install_ledger_path));
        println!("- {module_name}");
        println!("- console bundles: {}", console_bundle_install.bundle_count);
        return Ok(());
    }

    write_file(&env_file_path, env_file.as_bytes())?;
    if cargo_toml_changed {
        write_file(&cargo_toml_path, cargo_toml.as_bytes())?;
    }
    write_file(&host_lib_path, host_lib.as_bytes())?;
    if let Some(runtime_config_defaults) = &runtime_config_defaults {
        write_json(&runtime_config_defaults_path, runtime_config_defaults)?;
    }
    write_json(&install_ledger_path, &install_ledger)?;

    println!("Installed linked module {module_name}.");
    println!("Updated:");
    println!("- {}", display_relative(&repo_root, &env_file_path));
    if cargo_toml_changed {
        println!("- {}", display_relative(&repo_root, &cargo_toml_path));
    }
    println!("- {}", display_relative(&repo_root, &host_lib_path));
    if console_bundle_install.registry_changed {
        println!(
            "- {}",
            display_relative(&repo_root, &console_extension_registry_path)
        );
        for file_path in &console_bundle_install.bundle_files {
            println!("- {}", display_relative(&repo_root, file_path));
        }
    }
    if runtime_config_defaults.is_some() {
        println!(
            "- {}",
            display_relative(&repo_root, &runtime_config_defaults_path)
        );
    }
    println!("- {}", display_relative(&repo_root, &install_ledger_path));
    println!("Console bundles: {}", console_bundle_install.bundle_count);
    println!("Next steps:");
    println!("- cargo run --bin migrate");
    println!("- restart the API and worker");

    Ok(())
}

pub async fn uninstall_module(
    module_name: &str,
    options: RemoteModuleUninstallOptions,
) -> Result<()> {
    match uninstall_module_source(module_name, &options)? {
        ModuleSource::Remote => uninstall_remote_module(module_name, options).await,
        ModuleSource::Linked => uninstall_linked_module(module_name, options),
    }
}

fn uninstall_module_source(
    module_name: &str,
    options: &RemoteModuleUninstallOptions,
) -> Result<ModuleSource> {
    if let Some(source) = options.source.as_deref() {
        return parse_module_source(source);
    }

    let module_name = module_name.trim();
    if module_name.is_empty() {
        bail!("Module name is required");
    }

    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        options
            .env_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".env")),
    );
    let install_plan_path = resolve_path(
        &repo_root,
        Path::new(".lenso/console-package-install-plan.json"),
    );
    let console_extension_registry_path = repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH);
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let module_services_path = resolve_path(
        &repo_root,
        options
            .module_services_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    );
    if let Some(source) = module_install_ledger_source(&install_ledger_path, module_name)? {
        return Ok(source);
    }

    infer_uninstall_module_source(
        module_name,
        &read_text_if_exists(&env_file_path)?,
        remote_module_install_state_exists(
            module_name,
            &env_file_path,
            &install_plan_path,
            &console_extension_registry_path,
            &module_services_path,
        )?,
    )
}

pub async fn uninstall_remote_module(
    module_name: &str,
    options: RemoteModuleUninstallOptions,
) -> Result<()> {
    let module_name = module_name.trim();
    if module_name.is_empty() {
        bail!("Module name is required");
    }
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        options
            .env_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".env")),
    );
    let install_plan_path = resolve_path(
        &repo_root,
        Path::new(".lenso/console-package-install-plan.json"),
    );
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let module_services_path = resolve_path(
        &repo_root,
        options
            .module_services_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    );
    let console_extension_registry_path = repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH);
    let target = remote_uninstall_target(&install_ledger_path, module_name)?;
    for warning in remote_uninstall_dependency_warnings(&install_ledger_path, &target)? {
        eprintln!("warning: {warning}");
    }
    let console_extension_module_dirs = target
        .module_names
        .iter()
        .map(|module_name| {
            repo_root
                .join(".lenso/console/extensions")
                .join(slugify(module_name))
        })
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    let env_file = remove_remote_module_from_env(&env_file_path, &target.provider_name)?;
    let install_plan =
        remove_console_package_install_plan_modules(&install_plan_path, &target.module_names)?;
    let install_ledger =
        remove_module_install_ledger_modules(&install_ledger_path, &target.module_names)?;
    let module_services =
        remove_remote_module_services_file_module(&module_services_path, &target.provider_name)?;
    let console_registry = remove_runtime_console_bundle_registry_modules(
        &console_extension_registry_path,
        &target.module_names,
    )?;

    if options.dry_run {
        println!("Service uninstall dry run:");
        if env_file.is_some() {
            println!("- {}", display_relative(&repo_root, &env_file_path));
        }
        if install_plan.is_some() {
            println!("- {}", display_relative(&repo_root, &install_plan_path));
        }
        if install_ledger.is_some() {
            println!("- {}", display_relative(&repo_root, &install_ledger_path));
        }
        if module_services.is_some() {
            println!("- {}", display_relative(&repo_root, &module_services_path));
        }
        if console_registry.is_some() {
            println!(
                "- {}",
                display_relative(&repo_root, &console_extension_registry_path)
            );
        }
        for path in &console_extension_module_dirs {
            println!("- {}", display_relative(&repo_root, path));
        }
        if env_file.is_none()
            && install_plan.is_none()
            && install_ledger.is_none()
            && module_services.is_none()
            && console_registry.is_none()
            && console_extension_module_dirs.is_empty()
        {
            println!("- no local install state found");
        }
        return Ok(());
    }

    let changed = env_file.is_some()
        || install_plan.is_some()
        || install_ledger.is_some()
        || module_services.is_some()
        || console_registry.is_some()
        || !console_extension_module_dirs.is_empty();
    if let Some(env_file) = env_file {
        write_file(&env_file_path, env_file.as_bytes())?;
    }
    if let Some(install_plan) = install_plan {
        write_json(&install_plan_path, &install_plan)?;
    }
    if let Some(install_ledger) = install_ledger {
        write_json(&install_ledger_path, &install_ledger)?;
    }
    if let Some(module_services) = module_services {
        write_json(&module_services_path, &module_services)?;
    }
    if let Some(console_registry) = console_registry {
        write_json(&console_extension_registry_path, &console_registry)?;
    }
    for path in console_extension_module_dirs {
        fs::remove_dir_all(&path)
            .with_context(|| format!("remove console extension directory {}", path.display()))?;
    }

    if !changed {
        println!("Service {module_name} is not installed locally.");
        return Ok(());
    }

    if target.provider_name == module_name && target.module_names.len() == 1 {
        println!("Uninstalled service {module_name}.");
    } else {
        println!(
            "Uninstalled service {} and modules: {}.",
            target.provider_name,
            target.module_names.join(", ")
        );
    }
    println!("Next steps:");
    println!("- restart the API and worker");

    Ok(())
}

pub async fn doctor_module(options: ModuleDoctorOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        options
            .env_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".env")),
    );
    let module_services_path = resolve_path(
        &repo_root,
        options
            .module_services_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    );
    let requested_module = options
        .module_name
        .as_deref()
        .map(str::trim)
        .filter(|module_name| !module_name.is_empty());
    let report = build_module_doctor_report(
        &repo_root,
        &env_file_path,
        &module_services_path,
        requested_module,
    )
    .await?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_module_doctor_report(&repo_root, &env_file_path, &module_services_path, &report);
    }

    if report.issue_count > 0 {
        bail!("Service doctor found {} issue(s)", report.issue_count);
    }

    Ok(())
}

async fn build_module_doctor_report(
    repo_root: &Path,
    env_file_path: &Path,
    module_services_path: &Path,
    requested_module: Option<&str>,
) -> Result<ModuleDoctorReport> {
    let env_source = read_text_if_exists(&env_file_path)?;
    let remote_modules = remote_module_entries_from_env_source(&env_source);
    let service_states = read_remote_module_service_states(&module_services_path)?;
    let services_state_dir = module_services_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .context("build module doctor HTTP client")?;
    let mut issue_count = 0usize;
    let mut sources = Vec::new();
    let mut services = Vec::new();

    for (module_name, base_url) in remote_modules.iter().filter(|(module_name, _)| {
        requested_module.is_none_or(|requested| module_name == requested)
    }) {
        let enabled = module_enabled_from_env_source(&env_source, module_name);
        let installed =
            module_install_ledger_entry(&repo_root.join(MODULE_INSTALL_LEDGER_PATH), module_name)?
                .is_some();
        let mut fix = None;
        let mut manifest_url = None;
        let mut manifest_status = ModuleDoctorManifestStatus::Skipped;
        if !enabled {
            fix = Some("enable the service if it should load".to_owned());
        } else if let Some(url) = remote_module_manifest_url(base_url) {
            let manifest_ready = remote_service_ready_url(&client, &url).await;
            manifest_status = if manifest_ready {
                ModuleDoctorManifestStatus::Reachable
            } else {
                issue_count += 1;
                fix = Some(
                    "start the service or fix REMOTE_MODULES for this manifest URL".to_owned(),
                );
                ModuleDoctorManifestStatus::Unreachable
            };
            manifest_url = Some(url);
        }
        sources.push(ModuleDoctorSourceReport {
            module_name: module_name.to_owned(),
            installed,
            configured: true,
            enabled,
            base_url: Some(base_url.to_owned()),
            manifest_url,
            manifest_status,
            fix,
        });
    }

    if let Some(module_name) = requested_module {
        let has_source = remote_modules.iter().any(|(name, _)| name == module_name);
        let has_service_state = service_states
            .iter()
            .any(|state| state.module_name == module_name);
        if !has_source && !has_service_state {
            issue_count += 1;
            sources.push(ModuleDoctorSourceReport {
                module_name: module_name.to_owned(),
                installed: false,
                configured: false,
                enabled: false,
                base_url: None,
                manifest_url: None,
                manifest_status: ModuleDoctorManifestStatus::NotConfigured,
                fix: Some("install the service or add it to REMOTE_MODULES".to_owned()),
            });
        }
    }

    for state in service_states
        .iter()
        .filter(|state| requested_module.is_none_or(|module_name| state.module_name == module_name))
    {
        let configured = remote_modules
            .iter()
            .any(|(module_name, _)| module_name == &state.module_name);
        let enabled = module_enabled_from_env_source(&env_source, &state.module_name);

        for service in &state.services {
            let ready = remote_service_ready_url(&client, &service.ready_url).await;
            let lock_file_path = remote_module_service_state_path(
                services_state_dir,
                &state.module_name,
                service,
                "lock",
            );
            let pid_file_path = remote_module_service_state_path(
                services_state_dir,
                &state.module_name,
                service,
                "pid",
            );
            let lock_exists = lock_file_path.exists();
            let pid_exists = pid_file_path.exists();
            let status = remote_module_service_doctor_status(
                configured,
                enabled,
                service.auto_start,
                ready,
                lock_exists,
                pid_exists,
            );
            if status.is_issue() {
                issue_count += 1;
            }
            services.push(ModuleDoctorServiceReport {
                module_name: state.module_name.clone(),
                service_name: service.name.clone(),
                status: status.label().to_owned(),
                ready_url: service.ready_url.clone(),
                process: if service.auto_start {
                    "host-started".to_owned()
                } else {
                    "manual".to_owned()
                },
                command: (!ready).then(|| service.command.clone()),
                lock_file: lock_exists.then(|| display_relative(repo_root, &lock_file_path)),
                pid_file: pid_exists.then(|| display_relative(repo_root, &pid_file_path)),
                fix: remote_module_service_doctor_fix(status).map(ToOwned::to_owned),
            });
        }
    }

    Ok(ModuleDoctorReport {
        issue_count,
        sources_checked: sources.len(),
        services_checked: services.len(),
        sources,
        services,
    })
}

fn print_module_doctor_report(
    repo_root: &Path,
    env_file_path: &Path,
    module_services_path: &Path,
    report: &ModuleDoctorReport,
) {
    println!("Module doctor:");
    println!("- env: {}", display_relative(repo_root, env_file_path));
    println!(
        "- services: {}",
        display_relative(repo_root, module_services_path)
    );
    println!("- services: {}", report.sources.len());
    println!("Sources:");
    for source in &report.sources {
        println!(
            "- {}: {}",
            source.module_name,
            if source.configured {
                "configured"
            } else {
                "source_not_configured"
            }
        );
        println!(
            "  installed: {}",
            if source.installed { "yes" } else { "no" }
        );
        if let Some(base_url) = &source.base_url {
            println!("  baseUrl: {base_url}");
        }
        if let Some(manifest_url) = &source.manifest_url {
            println!("  manifest: {manifest_url}");
        }
        println!("  manifestStatus: {}", source.manifest_status.label());
        if let Some(fix) = &source.fix {
            println!("  fix: {fix}");
        }
    }

    println!("Services:");
    for service in &report.services {
        println!(
            "- {}/{}: {}",
            service.module_name, service.service_name, service.status
        );
        println!("  readyUrl: {}", service.ready_url);
        println!("  process: {}", service.process);
        if let Some(command) = &service.command {
            println!("  command: {command}");
        }
        if service.lock_file.is_some() || service.pid_file.is_some() {
            println!(
                "  state: lock={} pid={}",
                service.lock_file.as_deref().unwrap_or("-"),
                service.pid_file.as_deref().unwrap_or("-")
            );
        }
        if let Some(fix) = &service.fix {
            println!("  fix: {fix}");
        }
    }
    println!("- services checked: {}", report.services_checked);
    println!("- sources checked: {}", report.sources_checked);
}

pub async fn list_module_services(options: ModuleServiceListOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let module_services_path =
        resolve_module_services_file_path(&repo_root, options.module_services_file.as_deref());
    let states = read_remote_module_service_states(&module_services_path)?;
    let services = module_service_list_items(&states, options.module_name.as_deref());
    let report = ModuleServiceListReport { services };

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("MODULE\tSERVICE\tPROCESS\tREADY URL");
    for service in &report.services {
        println!(
            "{}\t{}\t{}\t{}",
            service.module_name,
            service.service_name,
            service_process_label(service.auto_start),
            service.ready_url
        );
    }
    Ok(())
}

pub async fn export_module_services(options: ModuleServiceExportOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let module_services_path =
        resolve_module_services_file_path(&repo_root, options.module_services_file.as_deref());
    let states = read_remote_module_service_states(&module_services_path)?;
    let state = states
        .iter()
        .find(|state| state.module_name == options.module_name)
        .ok_or_else(|| anyhow!("Service module not found: {}", options.module_name))?;
    let receipt = installed_service_receipt(&repo_root, &options.module_name).ok();
    match options.format.trim() {
        "compose" => print!("{}", compose_service_export_source(state)),
        "systemd" => print!("{}", systemd_service_export_source(state)),
        "dockerfile" => print!("{}", dockerfile_service_export_source(state)),
        "env" => print!("{}", env_service_export_source(state, receipt.as_ref())),
        other => bail!(
            "Unsupported service export format `{other}`; expected compose, systemd, dockerfile, or env"
        ),
    }
    Ok(())
}

fn compose_service_export_source(state: &RemoteModuleServiceState) -> String {
    let mut source = "services:\n".to_owned();
    for service in &state.services {
        source.push_str(&compose_service_source(state, service));
    }
    source
}

fn compose_service_source(
    state: &RemoteModuleServiceState,
    service: &RemoteModuleServiceInstallSpec,
) -> String {
    let service_key = format!("{}-{}", slugify(&state.module_name), slugify(&service.name));
    format!(
        "  {service_key}:\n    command: >-\n      {}\n    working_dir: {}\n    restart: unless-stopped\n    labels:\n      lenso.module: {}\n      lenso.service: {}\n      lenso.ready_url: {}\n",
        service.command,
        service.cwd.as_deref().unwrap_or("."),
        state.module_name,
        service.name,
        service.ready_url
    )
}

fn systemd_service_export_source(state: &RemoteModuleServiceState) -> String {
    let mut source = String::new();
    for service in &state.services {
        let unit_name = format!(
            "lenso-{}-{}",
            slugify(&state.module_name),
            slugify(&service.name)
        );
        source.push_str(&format!(
            "# {unit_name}.service\n[Unit]\nDescription=Lenso service {} / {}\nAfter=network.target\n\n[Service]\nWorkingDirectory={}\nExecStart=/bin/sh -lc '{}'\nRestart=always\nEnvironment=LENSO_READY_URL={}\n\n[Install]\nWantedBy=multi-user.target\n\n",
            state.module_name,
            service.name,
            shell_single_quote(service.cwd.as_deref().unwrap_or(".")),
            shell_single_quote(&service.command),
            service.ready_url
        ));
    }
    source
}

fn dockerfile_service_export_source(state: &RemoteModuleServiceState) -> String {
    let Some(service) = state.services.first() else {
        return "# no services declared\n".to_owned();
    };
    format!(
        "# Generated for Lenso service {} / {}\nFROM node:22-slim\nWORKDIR /app\nCOPY . .\nEXPOSE 4100\nCMD [\"sh\", \"-lc\", \"{}\"]\n",
        state.module_name,
        service.name,
        json_escaped_string(&service.command)
    )
}

fn env_service_export_source(state: &RemoteModuleServiceState, receipt: Option<&Value>) -> String {
    let mut source = format!("# Lenso service env for {}\n", state.module_name);
    let manifest = receipt.and_then(|receipt| receipt.get("serviceManifestSnapshot"));
    for key in manifest.map(service_env_set).unwrap_or_default() {
        source.push_str(&format!("{key}=\n"));
    }
    for service in &state.services {
        source.push_str(&format!(
            "LENSO_{}_READY_URL={}\n",
            snake_case(&service.name).to_ascii_uppercase(),
            service.ready_url
        ));
    }
    source
}

fn shell_single_quote(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn json_escaped_string(value: &str) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "\"\"".to_owned())
        .trim_matches('"')
        .to_owned()
}

pub async fn status_module_service(options: ModuleServiceStatusOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let module_services_path =
        resolve_module_services_file_path(&repo_root, options.module_services_file.as_deref());
    let states = read_remote_module_service_states(&module_services_path)?;
    let (module_name, service) =
        find_module_service(&states, &options.module_name, &options.service_name)?;
    let report = module_service_status_report(
        &repo_root,
        module_services_path
            .parent()
            .unwrap_or_else(|| Path::new(".")),
        &module_name,
        &service,
    )
    .await?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!(
        "{}/{}: {}",
        report.module_name, report.service_name, report.status
    );
    println!("readyUrl: {}", report.ready_url);
    println!(
        "state: lock={} pid={}",
        report.lock_file.as_deref().unwrap_or("-"),
        report.pid_file.as_deref().unwrap_or("-")
    );
    Ok(())
}

pub async fn logs_module_service(options: ModuleServiceLogsOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let module_services_path =
        resolve_module_services_file_path(&repo_root, options.module_services_file.as_deref());
    let states = read_remote_module_service_states(&module_services_path)?;
    let (module_name, service) =
        find_module_service(&states, &options.module_name, &options.service_name)?;
    let log_file_path = module_service_log_path(&repo_root, &module_name, &service.name);
    if !log_file_path.exists() {
        bail!(
            "No local log file for {}/{}; start it with `lenso service start {} {}`",
            module_name,
            service.name,
            module_name,
            service.name
        );
    }

    // ponytail: local dev logs are read whole; stream from EOF if these get large.
    let contents = read_text(&log_file_path)
        .with_context(|| format!("read service log {}", log_file_path.display()))?;
    for line in tail_lines(&contents, options.tail) {
        println!("{line}");
    }
    Ok(())
}

pub async fn start_module_service(options: ModuleServiceStartOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let module_services_path =
        resolve_module_services_file_path(&repo_root, options.module_services_file.as_deref());
    let states = read_remote_module_service_states(&module_services_path)?;
    let (module_name, service) =
        find_module_service(&states, &options.module_name, &options.service_name)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .context("build module service HTTP client")?;
    if remote_service_ready_url(&client, &service.ready_url).await {
        println!("{}/{} already ready", module_name, service.name);
        return Ok(());
    }

    let services_state_dir = module_services_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let lock_file_path =
        remote_module_service_state_path(services_state_dir, &module_name, &service, "lock");
    let pid_file_path =
        remote_module_service_state_path(services_state_dir, &module_name, &service, "pid");
    if lock_file_path.exists() || pid_file_path.exists() {
        bail!(
            "{}/{} already has local state; run `lenso service stop {} {}` first",
            module_name,
            service.name,
            module_name,
            service.name
        );
    }

    let cwd = service
        .cwd
        .as_deref()
        .map(|cwd| resolve_path(&repo_root, Path::new(cwd)))
        .unwrap_or_else(|| repo_root.clone());
    let log_file_path = module_service_log_path(&repo_root, &module_name, &service.name);
    if let Some(parent) = log_file_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file_path)
        .with_context(|| format!("open service log {}", log_file_path.display()))?;
    let stderr_log = log_file
        .try_clone()
        .with_context(|| format!("clone service log {}", log_file_path.display()))?;
    // ponytail: local dev process control; a real supervisor belongs in deployment tooling.
    let mut child = shell_command(&service.command)
        .current_dir(cwd)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(stderr_log))
        .spawn()
        .with_context(|| format!("start service {}/{}", module_name, service.name))?;
    write_service_lock(&lock_file_path)?;
    write_file(&pid_file_path, format!("{}\n", child.id()).as_bytes())?;
    println!(
        "Started service {}/{} with pid {}. Logs: {}",
        module_name,
        service.name,
        child.id(),
        display_relative(&repo_root, &log_file_path)
    );
    wait_for_started_module_service_ready(
        &client,
        &mut child,
        &module_name,
        &service,
        &lock_file_path,
        &pid_file_path,
    )
    .await?;
    Ok(())
}

pub async fn start_declared_module_services(
    repo_root: Option<&Path>,
    module_services_file: Option<&Path>,
) -> Result<()> {
    let repo_root = repo_root.unwrap_or_else(|| Path::new("."));
    let module_services_path = resolve_module_services_file_path(repo_root, module_services_file);
    let states = read_remote_module_service_states(&module_services_path)?;
    for state in states {
        for service in state.services {
            if service.auto_start {
                start_module_service(ModuleServiceStartOptions {
                    module_name: state.module_name.clone(),
                    service_name: service.name.clone(),
                    module_services_file: Some(module_services_path.clone()),
                    repo_root: Some(repo_root.to_path_buf()),
                })
                .await?;
            }
        }
    }
    Ok(())
}

pub async fn stop_module_service(options: ModuleServiceStopOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let module_services_path =
        resolve_module_services_file_path(&repo_root, options.module_services_file.as_deref());
    let states = read_remote_module_service_states(&module_services_path)?;
    let (module_name, service) =
        find_module_service(&states, &options.module_name, &options.service_name)?;
    let services_state_dir = module_services_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let lock_file_path =
        remote_module_service_state_path(services_state_dir, &module_name, &service, "lock");
    let pid_file_path =
        remote_module_service_state_path(services_state_dir, &module_name, &service, "pid");
    if !pid_file_path.exists() {
        println!("{}/{} not running", module_name, service.name);
        return Ok(());
    }
    let pid = read_text(&pid_file_path)?.trim().to_owned();
    let status = Command::new("kill")
        .arg(&pid)
        .status()
        .with_context(|| format!("stop service {}/{}", module_name, service.name))?;
    if !status.success() {
        bail!("kill failed for pid {pid}");
    }
    let _ = fs::remove_file(&pid_file_path);
    let _ = fs::remove_file(&lock_file_path);
    println!("Stopped service {}/{}.", module_name, service.name);
    Ok(())
}

fn resolve_module_services_file_path(
    repo_root: &Path,
    module_services_file: Option<&Path>,
) -> PathBuf {
    resolve_path(
        repo_root,
        module_services_file.unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    )
}

fn module_service_list_items(
    states: &[RemoteModuleServiceState],
    requested_module: Option<&str>,
) -> Vec<ModuleServiceListItem> {
    states
        .iter()
        .filter(|state| requested_module.is_none_or(|module_name| state.module_name == module_name))
        .flat_map(|state| {
            state.services.iter().map(|service| ModuleServiceListItem {
                module_name: state.module_name.clone(),
                service_name: service.name.clone(),
                auto_start: service.auto_start,
                command: service.command.clone(),
                ready_url: service.ready_url.clone(),
            })
        })
        .collect()
}

fn find_module_service(
    states: &[RemoteModuleServiceState],
    module_name: &str,
    service_name: &str,
) -> Result<(String, RemoteModuleServiceInstallSpec)> {
    states
        .iter()
        .find(|state| state.module_name == module_name)
        .and_then(|state| {
            state
                .services
                .iter()
                .find(|service| service.name == service_name)
                .cloned()
                .map(|service| (state.module_name.clone(), service))
        })
        .ok_or_else(|| anyhow!("Service not found: {module_name}/{service_name}"))
}

async fn module_service_status_report(
    repo_root: &Path,
    services_state_dir: &Path,
    module_name: &str,
    service: &RemoteModuleServiceInstallSpec,
) -> Result<ModuleServiceStatusReport> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .context("build module service HTTP client")?;
    let ready = remote_service_ready_url(&client, &service.ready_url).await;
    let lock_file_path =
        remote_module_service_state_path(services_state_dir, module_name, service, "lock");
    let pid_file_path =
        remote_module_service_state_path(services_state_dir, module_name, service, "pid");
    let lock_exists = lock_file_path.exists();
    let pid_exists = pid_file_path.exists();
    let status = if ready {
        "ready"
    } else if lock_exists || pid_exists {
        "stale_lock_or_pid"
    } else {
        "service_not_ready"
    };
    Ok(ModuleServiceStatusReport {
        module_name: module_name.to_owned(),
        service_name: service.name.clone(),
        status: status.to_owned(),
        ready,
        ready_url: service.ready_url.clone(),
        auto_start: service.auto_start,
        lock_file: lock_exists.then(|| display_relative(repo_root, &lock_file_path)),
        pid_file: pid_exists.then(|| display_relative(repo_root, &pid_file_path)),
    })
}

fn service_process_label(auto_start: bool) -> &'static str {
    if auto_start { "host-started" } else { "manual" }
}

fn write_service_lock(lock_file_path: &Path) -> Result<()> {
    if let Some(parent) = lock_file_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(lock_file_path)
        .with_context(|| format!("create {}", lock_file_path.display()))?;
    write_file(
        lock_file_path,
        format!("owner_pid={}\n", std::process::id()).as_bytes(),
    )
}

pub async fn add_module_catalog_entry(
    manifest_reference: &str,
    options: ModuleCatalogAddOptions,
) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let catalog_file_path = resolve_path(
        &repo_root,
        options
            .catalog_file
            .as_deref()
            .unwrap_or_else(|| Path::new(MODULE_CATALOG_PATH)),
    );
    let manifest = read_json_reference(manifest_reference).await?;
    if is_module_release_descriptor(&manifest) {
        let manifest = validate_module_release_descriptor(manifest)?;
        let module_name = string_field(&manifest, "name")?.trim().to_owned();
        let version = string_field(&manifest, "version")?.trim().to_owned();
        let mut catalog = read_json_if_exists(&catalog_file_path)?
            .unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
        let modules = catalog
            .get_mut("modules")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Module catalog modules must be an array"))?;
        modules.retain(|entry| {
            entry.get("name").and_then(Value::as_str) != Some(module_name.as_str())
        });
        modules.push(module_release_catalog_entry_from_manifest(
            &manifest,
            manifest_reference,
            options.base_url.as_deref(),
            options.summary.as_deref(),
        )?);

        if options.dry_run {
            println!("Module catalog dry run:");
            println!("- {}", display_relative(&repo_root, &catalog_file_path));
            println!("- {module_name} {version}");
            return Ok(());
        }

        write_json(&catalog_file_path, &catalog)?;
        println!("Added module release {module_name} to module catalog.");
        println!("Updated:");
        println!("- {}", display_relative(&repo_root, &catalog_file_path));
        println!("Install:");
        println!("- lenso module install {module_name}");
        return Ok(());
    }
    let service_package = if is_service_package_manifest(&manifest) {
        let package = validate_service_package_manifest(manifest.clone())?;
        let service_manifest_reference =
            service_package_manifest_reference(manifest_reference, &package)?;
        Some((
            ServicePackageInstallContext {
                manifest: package,
                reference: manifest_reference.to_owned(),
            },
            service_manifest_reference,
        ))
    } else {
        None
    };
    let (manifest_reference, manifest) =
        if let Some((_, service_manifest_reference)) = service_package.as_ref() {
            (
                service_manifest_reference.as_str(),
                read_json_reference(service_manifest_reference).await?,
            )
        } else {
            (manifest_reference, manifest)
        };
    let is_service = is_service_manifest(&manifest);
    let manifest = if is_service {
        validate_service_manifest(manifest)?
    } else {
        validate_remote_module_manifest(manifest)?
    };
    let module_name = string_field(&manifest, "name")?.trim().to_owned();
    let version = string_field(&manifest, "version")?.trim().to_owned();
    let base_url = derive_remote_base_url(options.base_url.as_deref(), manifest_reference)?;
    let mut catalog = read_json_if_exists(&catalog_file_path)?
        .unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
    let modules = catalog
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Module catalog modules must be an array"))?;
    modules.retain(|entry| entry.get("name").and_then(Value::as_str) != Some(module_name.as_str()));
    let mut entry = if is_service {
        service_catalog_entry_from_manifest(
            &manifest,
            manifest_reference,
            &base_url,
            options.summary.as_deref(),
        )?
    } else {
        module_catalog_entry_from_manifest(
            &manifest,
            manifest_reference,
            &base_url,
            options.summary.as_deref(),
        )?
    };
    if let Some((package_context, _)) = &service_package {
        entry["servicePackage"] = json!({
            "manifestReference": package_context.reference.clone(),
            "manifestSnapshot": package_context.manifest.clone(),
        });
    }
    modules.push(entry);

    if options.dry_run {
        println!("Module catalog dry run:");
        println!("- {}", display_relative(&repo_root, &catalog_file_path));
        println!("- {module_name} {version}");
        return Ok(());
    }

    write_json(&catalog_file_path, &catalog)?;
    if is_service {
        println!("Added service {module_name} to module catalog.");
    } else {
        println!("Added {module_name} to module catalog.");
    }
    println!("Updated:");
    println!("- {}", display_relative(&repo_root, &catalog_file_path));
    println!("Install:");
    if is_service {
        println!("- lenso service install {manifest_reference}");
    } else {
        println!("- lenso module install {manifest_reference}");
    }

    Ok(())
}

pub async fn check_service_manifest_reference(
    manifest_reference: &str,
    options: ServiceManifestCheckOptions,
) -> Result<()> {
    let defer_manifest_fetch = (manifest_reference.starts_with("http://")
        || manifest_reference.starts_with("https://"))
        && options.serve_command.is_some();
    let (manifest_reference, initial_manifest) = if defer_manifest_fetch {
        (manifest_reference.to_owned(), None)
    } else {
        let (manifest_reference, manifest, _) =
            read_service_or_package_manifest(manifest_reference).await?;
        (manifest_reference, Some(manifest))
    };
    let manifest_url = service_check_manifest_url(
        &manifest_reference,
        initial_manifest.as_ref(),
        options.manifest_url.as_deref(),
    );
    let ready_url = service_check_ready_url(
        initial_manifest.as_ref(),
        manifest_url.as_deref(),
        options.ready_url.as_deref(),
    );
    let mut process = if let Some(command) = options.serve_command.as_deref() {
        let ready_url = ready_url.as_deref().ok_or_else(|| {
            anyhow!(
                "Service check needs --ready-url or a manifest health/install ready URL when using --serve-command"
            )
        })?;
        let manifest_url = manifest_url.as_deref().ok_or_else(|| {
            anyhow!(
                "Service check needs --manifest-url or an inferable manifest URL when using --serve-command"
            )
        })?;
        let mut process = ManagedCheckProcess::spawn(command, options.cwd.as_deref())?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(800))
            .build()
            .context("build service check HTTP client")?;
        wait_for_service_check_ready(
            &client,
            &mut process.child,
            ready_url,
            options.ready_timeout_ms,
        )
        .await?;
        let fetched_manifest = client
            .get(manifest_url)
            .send()
            .await
            .with_context(|| format!("fetch service manifest {manifest_url}"))?
            .error_for_status()
            .with_context(|| format!("fetch service manifest {manifest_url}"))?
            .json::<Value>()
            .await
            .context("parse service manifest JSON")?;
        Some((process, validate_service_manifest(fetched_manifest)?))
    } else {
        None
    };
    let manifest = if let Some((_, manifest)) = process.as_ref() {
        manifest.clone()
    } else if let Some(manifest) = initial_manifest {
        manifest
    } else {
        read_service_or_package_manifest(&manifest_reference)
            .await?
            .1
    };
    let name = string_field(&manifest, "name")?.trim();
    let version = string_field(&manifest, "version")?.trim();
    let modules = manifest
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Service manifest modules must be an array"))?;
    let module_names = modules
        .iter()
        .filter_map(|module| module.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    let operations = service_manifest_operations(&manifest, options.operation.as_deref());
    if let Some(operation) = options.operation.as_deref()
        && operations.is_empty()
    {
        bail!("Service operation `{operation}` was not found in manifest");
    }
    let probes = if let Some(manifest_url) = manifest_url.as_deref() {
        service_check_operation_probe_summary(
            &operations,
            manifest_url,
            options.sample_input.as_deref(),
        )
        .await?
    } else {
        Vec::new()
    };
    let declarations = service_check_declaration_summary(&manifest);
    if let Some(failed_probe) = probes
        .iter()
        .find(|probe| probe.get("status").and_then(Value::as_str) == Some("failed"))
    {
        bail!(
            "Service probe failed: {} {} {}",
            failed_probe
                .get("operationId")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            failed_probe
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("-"),
            failed_probe
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("-")
        );
    }

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "declarations": declarations,
                "manifestReference": manifest_reference,
                "manifestUrl": manifest_url,
                "modules": module_names,
                "operations": operations,
                "probes": probes,
                "readyUrl": ready_url,
                "service": name,
                "status": "ok",
                "version": version,
            }))
            .context("serialize service manifest check")?
        );
    } else {
        println!("Service manifest ok: {name} {version}");
        println!("Provided modules: {}", module_names.join(", "));
        println!(
            "Declared operations: routes={} actions={} runtime={} events={}",
            declarations["routes"],
            declarations["actions"],
            declarations["runtimeFunctions"],
            declarations["eventHandlers"]
        );
        println!("Service operations: {}", operations.len());
        if let Some(ready_url) = ready_url {
            println!("Ready URL: {ready_url}");
        }
        if let Some(manifest_url) = manifest_url {
            println!("Manifest URL: {manifest_url}");
        }
        if !probes.is_empty() {
            println!("Probes:");
            for probe in probes {
                println!(
                    "- {} {} {} {}",
                    probe
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("skip"),
                    probe
                        .get("operationId")
                        .and_then(Value::as_str)
                        .unwrap_or("-"),
                    probe.get("method").and_then(Value::as_str).unwrap_or("-"),
                    probe
                        .get("url")
                        .or_else(|| probe.get("reason"))
                        .and_then(Value::as_str)
                        .unwrap_or("-")
                );
            }
        }
    }
    drop(process.take());
    Ok(())
}

struct ManagedCheckProcess {
    child: Child,
}

impl ManagedCheckProcess {
    fn spawn(command: &str, cwd: Option<&Path>) -> Result<Self> {
        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        if let Some(cwd) = cwd {
            process.current_dir(cwd);
        }
        let child = process
            .spawn()
            .with_context(|| format!("start service check command `{command}`"))?;
        Ok(Self { child })
    }
}

impl Drop for ManagedCheckProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

async fn wait_for_service_check_ready(
    client: &reqwest::Client,
    child: &mut Child,
    ready_url: &str,
    ready_timeout_ms: u64,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(ready_timeout_ms);
    loop {
        if remote_service_ready_url(client, ready_url).await {
            return Ok(());
        }
        if let Some(status) = child.try_wait().context("check service command status")? {
            bail!("service command exited before ready: {status}");
        }
        if Instant::now() >= deadline {
            bail!("service did not become ready at {ready_url} within {ready_timeout_ms}ms");
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn service_check_manifest_url(
    manifest_reference: &str,
    manifest: Option<&Value>,
    explicit_manifest_url: Option<&str>,
) -> Option<String> {
    explicit_manifest_url
        .map(ToOwned::to_owned)
        .or_else(|| {
            manifest
                .and_then(|manifest| manifest.get("health"))
                .and_then(|health| {
                    health
                        .get("manifestUrl")
                        .or_else(|| health.get("manifest_url"))
                })
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            manifest
                .and_then(service_check_first_ready_url)
                .and_then(|url| infer_manifest_url_from_ready_url(&url))
        })
        .or_else(|| {
            (manifest_reference.starts_with("http://")
                || manifest_reference.starts_with("https://"))
            .then(|| manifest_reference.to_owned())
        })
}

fn service_check_ready_url(
    manifest: Option<&Value>,
    manifest_url: Option<&str>,
    explicit_ready_url: Option<&str>,
) -> Option<String> {
    explicit_ready_url
        .map(ToOwned::to_owned)
        .or_else(|| manifest.and_then(service_check_first_ready_url))
        .or_else(|| manifest_url.and_then(infer_ready_url_from_manifest_url))
}

fn service_check_first_ready_url(manifest: &Value) -> Option<String> {
    manifest
        .get("health")
        .and_then(|health| {
            health
                .get("readyUrl")
                .or_else(|| health.get("ready_url"))
                .or_else(|| health.get("statusUrl"))
                .or_else(|| health.get("status_url"))
        })
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            manifest
                .get("install")
                .and_then(|install| install.get("services"))
                .and_then(Value::as_array)
                .and_then(|services| services.first())
                .and_then(|service| service.get("readyUrl").or_else(|| service.get("ready_url")))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

fn infer_manifest_url_from_ready_url(ready_url: &str) -> Option<String> {
    ready_url
        .strip_suffix("/status")
        .or_else(|| ready_url.strip_suffix("/ready"))
        .map(|base| format!("{base}/manifest"))
}

fn infer_ready_url_from_manifest_url(manifest_url: &str) -> Option<String> {
    manifest_url
        .strip_suffix("/manifest")
        .map(|base| format!("{base}/status"))
}

fn service_manifest_operations(manifest: &Value, filter: Option<&str>) -> Vec<Value> {
    let mut operations = Vec::new();
    for module in manifest
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(module_name) = module.get("name").and_then(Value::as_str) else {
            continue;
        };
        for route in module
            .get("http_routes")
            .or_else(|| module.get("httpRoutes"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let method = route
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_ascii_uppercase();
            let path = route.get("path").and_then(Value::as_str).unwrap_or("");
            let operation = route.get("operation");
            let operation_id = operation_id(operation)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{module_name}/http/{method}:{path}"));
            let safe_probe_spec = operation_safe_probe(operation).cloned();
            let legacy_safe_probe = operation.is_none()
                && method == "GET"
                && !path.contains('{')
                && !path.contains(':');
            push_manifest_operation(
                &mut operations,
                filter,
                json!({
                    "capability": route.get("capability").and_then(Value::as_str),
                    "kind": "http_route",
                    "method": method,
                    "module": module_name,
                    "operationId": operation_id,
                    "path": path,
                    "safeProbe": safe_probe_spec.is_some() || legacy_safe_probe,
                    "safeProbeSpec": safe_probe_spec,
                }),
            );
        }
        for function in module
            .get("runtime")
            .and_then(|runtime| runtime.get("functions"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = function.get("name").and_then(Value::as_str).unwrap_or("");
            let operation = function.get("operation");
            let operation_id = operation_id(operation)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{module_name}/runtime/{name}"));
            let safe_probe_spec = operation_safe_probe(operation).cloned();
            push_manifest_operation(
                &mut operations,
                filter,
                json!({
                    "kind": "runtime_function",
                    "module": module_name,
                    "name": name,
                    "operationId": operation_id,
                    "safeProbe": safe_probe_spec.is_some(),
                    "safeProbeSpec": safe_probe_spec,
                }),
            );
        }
        for handler in module
            .get("events")
            .and_then(|events| events.get("handlers"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = handler
                .get("name")
                .or_else(|| handler.get("event"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let operation = handler.get("operation");
            let operation_id = operation_id(operation)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("{module_name}/event/{name}"));
            let safe_probe_spec = operation_safe_probe(operation).cloned();
            push_manifest_operation(
                &mut operations,
                filter,
                json!({
                    "eventName": handler.get("eventName").or_else(|| handler.get("event_name")).and_then(Value::as_str),
                    "kind": "event_handler",
                    "module": module_name,
                    "name": name,
                    "operationId": operation_id,
                    "safeProbe": safe_probe_spec.is_some(),
                    "safeProbeSpec": safe_probe_spec,
                }),
            );
        }
        let admin = module.get("admin");
        let include_admin_actions = admin
            .and_then(|admin| admin.get("kind"))
            .and_then(Value::as_str)
            .is_none_or(|kind| kind == "declarative_custom");
        if include_admin_actions {
            for action in admin
                .and_then(|admin| admin.get("actions"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let name = action.get("name").and_then(Value::as_str).unwrap_or("");
                let operation = action.get("operation");
                let operation_id = operation_id(operation)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("{module_name}/action/{name}"));
                let safe_probe_spec = operation_safe_probe(operation).cloned();
                push_manifest_operation(
                    &mut operations,
                    filter,
                    json!({
                        "capability": action.get("capability").and_then(Value::as_str),
                        "kind": "admin_action",
                        "module": module_name,
                        "name": name,
                        "operationId": operation_id,
                        "safeProbe": safe_probe_spec.is_some(),
                        "safeProbeSpec": safe_probe_spec,
                    }),
                );
            }
        }
    }
    operations.sort_by(|left, right| {
        left.get("operationId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .cmp(
                right
                    .get("operationId")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            )
    });
    operations
}

fn operation_id(operation: Option<&Value>) -> Option<&str> {
    operation
        .and_then(|operation| {
            operation
                .get("operationId")
                .or_else(|| operation.get("operation_id"))
        })
        .and_then(Value::as_str)
}

fn operation_safe_probe(operation: Option<&Value>) -> Option<&Value> {
    let probe = operation.and_then(|operation| {
        operation
            .get("safeProbe")
            .or_else(|| operation.get("safe_probe"))
    })?;
    match probe {
        Value::Bool(true) | Value::Object(_) => Some(probe),
        _ => None,
    }
}

fn push_manifest_operation(operations: &mut Vec<Value>, filter: Option<&str>, operation: Value) {
    let operation_id = operation.get("operationId").and_then(Value::as_str);
    if filter.is_none_or(|filter| operation_id == Some(filter)) {
        operations.push(operation);
    }
}

async fn service_check_operation_probe_summary(
    operations: &[Value],
    manifest_url: &str,
    _sample_input: Option<&Path>,
) -> Result<Vec<Value>> {
    let service_base_url = manifest_url
        .strip_suffix("/manifest")
        .unwrap_or(manifest_url)
        .to_owned();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .context("build service probe HTTP client")?;
    let mut probes = Vec::new();
    for operation in operations {
        let kind = operation.get("kind").and_then(Value::as_str).unwrap_or("");
        let operation_id = operation
            .get("operationId")
            .and_then(Value::as_str)
            .unwrap_or("-");
        if kind != "http_route" {
            probes.push(json!({
                "kind": kind,
                "operationId": operation_id,
                "reason": "operation kind is not probed",
                "status": "skipped",
            }));
            continue;
        }
        if operation.get("safeProbe").and_then(Value::as_bool) != Some(true) {
            probes.push(json!({
                "kind": kind,
                "operationId": operation_id,
                "reason": "safeProbe not declared",
                "status": "skipped",
            }));
            continue;
        }
        let probe = operation.get("safeProbeSpec");
        let method = probe
            .and_then(|probe| probe.get("method"))
            .and_then(Value::as_str)
            .or_else(|| operation.get("method").and_then(Value::as_str))
            .unwrap_or("")
            .to_ascii_uppercase();
        let path = probe
            .and_then(|probe| probe.get("path"))
            .and_then(Value::as_str)
            .or_else(|| operation.get("path").and_then(Value::as_str))
            .unwrap_or("");
        let module_name = operation
            .get("module")
            .and_then(Value::as_str)
            .unwrap_or("");
        if method != "GET" || path.contains('{') || path.contains(':') {
            probes.push(json!({
                "kind": kind,
                "method": method,
                "operationId": operation_id,
                "path": path,
                "reason": "only literal HTTP GET safe probes are supported",
                "status": "skipped",
            }));
            continue;
        }
        let url = join_url_path(
            &service_base_url,
            &format!("modules/{module_name}/{}", path.trim_start_matches('/')),
        );
        let status = if remote_service_ready_url(&client, &url).await {
            "ok"
        } else {
            "failed"
        };
        probes.push(json!({
            "kind": kind,
            "method": method,
            "module": module_name,
            "operationId": operation_id,
            "path": path,
            "status": status,
            "url": url,
        }));
    }
    Ok(probes)
}

fn service_check_declaration_summary(manifest: &Value) -> Value {
    let mut routes = 0usize;
    let mut actions = 0usize;
    let mut runtime_functions = 0usize;
    let mut event_handlers = 0usize;
    for module in manifest
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        routes += module
            .get("http_routes")
            .or_else(|| module.get("httpRoutes"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        actions += module
            .get("admin")
            .and_then(|admin| admin.get("actions"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        runtime_functions += module
            .get("runtime")
            .and_then(|runtime| runtime.get("functions"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        event_handlers += module
            .get("events")
            .and_then(|events| events.get("handlers"))
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
    }
    json!({
        "actions": actions,
        "eventHandlers": event_handlers,
        "routes": routes,
        "runtimeFunctions": runtime_functions,
    })
}

pub async fn diff_service(options: ServiceDiffOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let receipt = installed_service_receipt(&repo_root, &options.service_name)?;
    let current = receipt
        .get("serviceManifestSnapshot")
        .ok_or_else(|| {
            anyhow!(
                "Service `{}` has no manifest snapshot; reinstall or upgrade it once before diff",
                options.service_name
            )
        })?
        .clone();
    let (_, candidate, _) = read_service_or_package_manifest(&options.manifest_reference).await?;
    ensure_service_name_matches(&candidate, &options.service_name)?;
    let diff = service_manifest_diff(&current, &candidate);

    if options.json {
        println!("{}", serde_json::to_string_pretty(&diff)?);
    } else {
        print_service_manifest_diff(&options.service_name, &diff);
    }
    Ok(())
}

pub async fn upgrade_service(options: ServiceUpgradeOptions) -> Result<()> {
    let (manifest_reference, candidate, package_context) =
        read_service_or_package_manifest(&options.manifest_reference).await?;
    ensure_service_name_matches(&candidate, &options.service_name)?;
    let install_options = RemoteModuleInstallOptions {
        allow_incompatible: options.allow_incompatible,
        base_url: options.base_url,
        console_plan: false,
        dry_run: options.dry_run,
        env_file: options.env_file,
        install_profiles: Vec::new(),
        module_services_file: options.module_services_file,
        repo_root: options.repo_root,
        run_install_commands: false,
        source: "remote".to_owned(),
    };
    add_service_manifest_with_options(
        &manifest_reference,
        candidate,
        &install_options,
        package_context.as_ref(),
        None,
    )
    .await
}

pub async fn rollback_service(options: ServiceRollbackOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let receipt = installed_service_receipt(&repo_root, &options.service_name)?;
    let previous = receipt
        .get("previousServiceManifestSnapshot")
        .ok_or_else(|| {
            anyhow!(
                "Service `{}` has no previous manifest snapshot to roll back to",
                options.service_name
            )
        })?
        .clone();
    let previous = validate_service_manifest(previous)?;
    let manifest_reference = receipt
        .get("service")
        .and_then(|service| service.get("manifestReference"))
        .or_else(|| receipt.get("manifestReference"))
        .and_then(Value::as_str)
        .unwrap_or("rollback:lenso.service.json")
        .to_owned();
    let install_options = RemoteModuleInstallOptions {
        allow_incompatible: true,
        base_url: service_receipt_base_url(&receipt),
        console_plan: false,
        dry_run: options.dry_run,
        env_file: options.env_file,
        install_profiles: Vec::new(),
        module_services_file: options.module_services_file,
        repo_root: options.repo_root,
        run_install_commands: false,
        source: "remote".to_owned(),
    };
    add_service_manifest_with_options(&manifest_reference, previous, &install_options, None, None)
        .await
}

fn installed_service_receipt(repo_root: &Path, service_name: &str) -> Result<Value> {
    let ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let ledger = read_json_if_exists(&ledger_path)?
        .ok_or_else(|| anyhow!("Module install ledger not found: {}", ledger_path.display()))?;
    let modules = ledger
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Module install ledger modules must be an array"))?;
    modules
        .iter()
        .find(|entry| {
            entry.get("moduleName").and_then(Value::as_str) == Some(service_name)
                || service_receipt_name(entry) == Some(service_name)
        })
        .cloned()
        .ok_or_else(|| anyhow!("Installed service not found: {service_name}"))
}

fn ensure_service_name_matches(manifest: &Value, expected: &str) -> Result<()> {
    let actual = string_field(manifest, "name")?.trim();
    if actual != expected {
        bail!("Service manifest is for `{actual}`, expected `{expected}`");
    }
    Ok(())
}

fn service_manifest_diff(current: &Value, candidate: &Value) -> Value {
    let current_modules = service_module_name_set(current);
    let candidate_modules = service_module_name_set(candidate);
    let all_modules = current_modules
        .union(&candidate_modules)
        .cloned()
        .collect::<BTreeSet<_>>();
    let capability_changes = all_modules
        .iter()
        .filter_map(|module| {
            let current = service_module_string_set(current, module, "capabilities");
            let candidate = service_module_string_set(candidate, module, "capabilities");
            let added = set_added(&current, &candidate);
            let removed = set_removed(&current, &candidate);
            (!added.is_empty() || !removed.is_empty()).then(|| {
                json!({
                    "added": added,
                    "module": module,
                    "removed": removed,
                })
            })
        })
        .collect::<Vec<_>>();
    let operation_changes = all_modules
        .iter()
        .filter_map(|module| {
            let current = service_module_operation_set(current, module);
            let candidate = service_module_operation_set(candidate, module);
            let added = set_added(&current, &candidate);
            let removed = set_removed(&current, &candidate);
            (!added.is_empty() || !removed.is_empty()).then(|| {
                json!({
                    "added": added,
                    "module": module,
                    "removed": removed,
                })
            })
        })
        .collect::<Vec<_>>();
    let current_env = service_env_set(current);
    let candidate_env = service_env_set(candidate);
    let current_config = service_config_set(current);
    let candidate_config = service_config_set(candidate);

    json!({
        "capabilities": capability_changes,
        "compatibilityChanged": current.get("compatibility") != candidate.get("compatibility"),
        "config": {
            "added": set_added(&current_config, &candidate_config),
            "removed": set_removed(&current_config, &candidate_config),
        },
        "env": {
            "added": set_added(&current_env, &candidate_env),
            "removed": set_removed(&current_env, &candidate_env),
        },
        "modules": {
            "added": set_added(&current_modules, &candidate_modules),
            "removed": set_removed(&current_modules, &candidate_modules),
        },
        "operations": operation_changes,
    })
}

fn print_service_manifest_diff(service_name: &str, diff: &Value) {
    println!("Service diff: {service_name}");
    print_diff_group("modules added", &diff["modules"]["added"]);
    print_diff_group("modules removed", &diff["modules"]["removed"]);
    print_diff_group("env added", &diff["env"]["added"]);
    print_diff_group("env removed", &diff["env"]["removed"]);
    print_diff_group("config added", &diff["config"]["added"]);
    print_diff_group("config removed", &diff["config"]["removed"]);
    println!(
        "compatibility changed: {}",
        diff.get("compatibilityChanged")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );
    for change in diff
        .get("capabilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        println!(
            "capabilities {}: +{} -{}",
            change.get("module").and_then(Value::as_str).unwrap_or("-"),
            json_string_list(&change["added"]).join(", "),
            json_string_list(&change["removed"]).join(", ")
        );
    }
    for change in diff
        .get("operations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        println!(
            "operations {}: +{} -{}",
            change.get("module").and_then(Value::as_str).unwrap_or("-"),
            json_string_list(&change["added"]).join(", "),
            json_string_list(&change["removed"]).join(", ")
        );
    }
}

fn print_diff_group(label: &str, value: &Value) {
    let items = json_string_list(value);
    if !items.is_empty() {
        println!("{label}: {}", items.join(", "));
    }
}

fn json_string_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn service_module_name_set(manifest: &Value) -> BTreeSet<String> {
    manifest
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|module| module.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn service_module<'a>(manifest: &'a Value, module_name: &str) -> Option<&'a Value> {
    manifest
        .get("modules")
        .and_then(Value::as_array)?
        .iter()
        .find(|module| module.get("name").and_then(Value::as_str) == Some(module_name))
}

fn service_module_string_set(manifest: &Value, module_name: &str, key: &str) -> BTreeSet<String> {
    service_module(manifest, module_name)
        .and_then(|module| module.get(key))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn service_module_operation_set(manifest: &Value, module_name: &str) -> BTreeSet<String> {
    let Some(module) = service_module(manifest, module_name) else {
        return BTreeSet::new();
    };
    let mut operations = BTreeSet::new();
    for route in module
        .get("http_routes")
        .or_else(|| module.get("httpRoutes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(method), Some(path)) = (
            route.get("method").and_then(Value::as_str),
            route.get("path").and_then(Value::as_str),
        ) {
            operations.insert(format!("route:{method} {path}"));
        }
    }
    for function in module
        .get("runtime")
        .and_then(|runtime| runtime.get("functions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = function.get("name").and_then(Value::as_str) {
            operations.insert(format!("runtime:{name}"));
        }
    }
    for handler in module
        .get("events")
        .and_then(|events| events.get("handlers"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = handler
            .get("event")
            .or_else(|| handler.get("name"))
            .and_then(Value::as_str)
        {
            operations.insert(format!("event:{name}"));
        }
    }
    for action in module
        .get("admin")
        .and_then(|admin| admin.get("actions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(name) = action.get("name").and_then(Value::as_str) {
            operations.insert(format!("action:{name}"));
        }
    }
    operations
}

fn service_env_set(manifest: &Value) -> BTreeSet<String> {
    let mut values = manifest
        .get("requiredEnv")
        .or_else(|| manifest.get("required_env"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    values.extend(
        manifest
            .get("env")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|field| field.get("name").and_then(Value::as_str))
            .map(ToOwned::to_owned),
    );
    values
}

fn service_config_set(manifest: &Value) -> BTreeSet<String> {
    manifest
        .get("config")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|field| field.get("key").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn set_added(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    right.difference(left).cloned().collect()
}

fn set_removed(left: &BTreeSet<String>, right: &BTreeSet<String>) -> Vec<String> {
    left.difference(right).cloned().collect()
}

pub async fn apply_console_package_install_plan(
    options: ConsolePackageApplyPlanOptions,
) -> Result<AppliedConsolePlan> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let runtime_console_root = options
        .runtime_console_root
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or(default_runtime_console_root_for_repo(&repo_root)?);
    let runtime_console_root = absolutize(&runtime_console_root)?;
    let install_plan_path = resolve_path(
        &repo_root,
        options
            .install_plan_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/console-package-install-plan.json")),
    );
    let dependency_version = options
        .dependency_version
        .unwrap_or_else(|| "latest".to_owned());
    let install_plan = read_json(&install_plan_path)?;
    let paths = runtime_console_paths(&runtime_console_root);
    let mut package_json = read_json(&paths.package_json_path)?;
    let mut manifest_exports_source = read_text(&paths.manifest_exports_path)?;
    let mut module_exports_source = read_text(&paths.module_exports_path)?;
    let plan_items = unique_console_package_plan_items(&install_plan);

    for item in &plan_items {
        update_package_json_dependency(&mut package_json, &item.package_name, &dependency_version)?;
        let manifest_name = manifest_name_from_module_export(&item.export_name);
        manifest_exports_source = insert_before_needle(
            &manifest_exports_source,
            &format!(
                "import {{ {manifest_name} }} from \"{}\";\n",
                item.package_name
            ),
            "export const consolePackageManifests",
        )?;
        manifest_exports_source = insert_before_needle(
            &manifest_exports_source,
            &format!("  {manifest_name},\n"),
            "] as const;",
        )?;
        module_exports_source = insert_before_needle(
            &module_exports_source,
            &format!(
                "import {{ {manifest_name}, {} }} from \"{}\";\n",
                item.export_name, item.package_name
            ),
            "import {",
        )?;
        module_exports_source = insert_before_needle(
            &module_exports_source,
            &format!(
                "  [consolePackageKey({manifest_name})]: {},\n",
                item.export_name
            ),
            "} satisfies ConsolePackageModuleExportsByKey;",
        )?;
    }

    if options.dry_run {
        println!("Console package install plan dry run:");
        println!(
            "- {}",
            display_relative(&repo_root, &paths.package_json_path)
        );
        println!(
            "- {}",
            display_relative(&repo_root, &paths.manifest_exports_path)
        );
        println!(
            "- {}",
            display_relative(&repo_root, &paths.module_exports_path)
        );
        return Ok(AppliedConsolePlan);
    }

    write_json(&paths.package_json_path, &package_json)?;
    write_file(
        &paths.manifest_exports_path,
        manifest_exports_source.as_bytes(),
    )?;
    write_file(&paths.module_exports_path, module_exports_source.as_bytes())?;

    println!(
        "Applied {} console package install plan item(s).",
        plan_items.len()
    );
    if options.log_next_steps {
        let console_root = display_relative(&repo_root, &runtime_console_root);
        println!("Next steps:");
        println!("- pnpm --dir {console_root} install");
        println!("- pnpm --dir {console_root} check:console-packages");
        println!("- pnpm check");
    }

    Ok(AppliedConsolePlan)
}

async fn create_remote_module(options: ModuleCreateOptions) -> Result<()> {
    let module_id = slugify(&options.module_id);
    if module_id.is_empty() {
        bail!("Module id is required");
    }
    let output_root = options
        .output_dir
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir().context("resolve current directory")?);
    let output_root = absolutize(&output_root)?;
    let package_root_name = slugify(
        options
            .package_root
            .as_deref()
            .unwrap_or(&format!("lenso-{module_id}")),
    );
    if package_root_name.is_empty() {
        bail!("Remote package root is required");
    }
    let package_root = output_root.join(&package_root_name);
    if package_root.exists() {
        bail!("Service package already exists: {}", package_root.display());
    }

    let mut package_context = build_console_package_context(
        ConsolePackageBuildInput::for_remote_module(&options, &module_id),
        &package_root,
    )?;
    package_context.package_dir = package_root.join("console");

    let mut pending_writes = PendingWrites::new();
    queue_remote_module_files(
        &mut pending_writes,
        &package_root,
        &package_root_name,
        &package_context,
    )?;

    if options.dry_run {
        println!("Service package dry run:");
        for file_path in pending_writes.keys() {
            println!("- {}", display_relative(&output_root, file_path));
        }
        return Ok(());
    }

    write_pending_files(&pending_writes)?;

    println!("Created service package {package_root_name}.");
    println!("Next steps:");
    println!("- pnpm --dir {package_root_name}/backend dev");
    println!("- lenso service install http://127.0.0.1:4100/lenso/service/v1/manifest");
    println!(
        "- lenso module catalog add http://127.0.0.1:4100/lenso/service/v1/manifest # optional discovery"
    );
    println!("- publish or install the console package");
    println!("- pnpm install");

    Ok(())
}

#[derive(Debug, Clone)]
struct ConsolePackageBuildInput {
    area: Option<String>,
    capability: Option<String>,
    icon: Option<String>,
    label: Option<String>,
    module_id: String,
    package_name: Option<String>,
    package_private: bool,
    package_scope: Option<String>,
    package_slug: Option<String>,
    registry_source: Option<String>,
    route: Option<String>,
    runtime_console_api_version: String,
    surface_name: Option<String>,
}

impl ConsolePackageBuildInput {
    fn from_module_options(options: &ModuleCreateOptions) -> Self {
        Self {
            area: options.area.clone(),
            capability: options.capability.clone(),
            icon: options.icon.clone(),
            label: options.label.clone(),
            module_id: options.module_id.clone(),
            package_name: options.package_name.clone(),
            package_private: true,
            package_scope: options.package_scope.clone(),
            package_slug: options.package_slug.clone(),
            registry_source: options.source.clone(),
            route: options.route.clone(),
            runtime_console_api_version: "workspace:*".to_owned(),
            surface_name: options.surface_name.clone(),
        }
    }

    fn from_console_package_options(options: &ConsolePackageCreateOptions) -> Self {
        Self {
            area: options.area.clone(),
            capability: options.capability.clone(),
            icon: options.icon.clone(),
            label: options.label.clone(),
            module_id: options.module_id.clone(),
            package_name: options.package_name.clone(),
            package_private: true,
            package_scope: options.package_scope.clone(),
            package_slug: options.package_slug.clone(),
            registry_source: options.source.clone(),
            route: options.route.clone(),
            runtime_console_api_version: "workspace:*".to_owned(),
            surface_name: options.surface_name.clone(),
        }
    }

    fn for_remote_module(options: &ModuleCreateOptions, module_id: &str) -> Self {
        Self {
            area: options.area.clone(),
            capability: options.capability.clone(),
            icon: options.icon.clone(),
            label: options.label.clone(),
            module_id: module_id.to_owned(),
            package_name: options.package_name.clone().or_else(|| {
                Some(format!(
                    "{}/lenso-{module_id}-console",
                    options.package_scope.as_deref().unwrap_or("@vendor")
                ))
            }),
            package_private: false,
            package_scope: options.package_scope.clone(),
            package_slug: Some(format!("{module_id}-console")),
            registry_source: options
                .source
                .clone()
                .or_else(|| Some("installed".to_owned())),
            route: options.route.clone(),
            runtime_console_api_version: "^0.1.0".to_owned(),
            surface_name: options.surface_name.clone(),
        }
    }
}

fn build_console_package_context(
    input: ConsolePackageBuildInput,
    runtime_console_root: &Path,
) -> Result<ConsolePackageContext> {
    let module_id = slugify(&input.module_id);
    if module_id.is_empty() {
        bail!("Module id is required");
    }
    let package_slug = slugify(
        input
            .package_slug
            .as_deref()
            .unwrap_or(&format!("{module_id}-console")),
    );
    if package_slug.is_empty() {
        bail!("Console package slug is required");
    }
    let package_name = input.package_name.unwrap_or_else(|| {
        format!(
            "{}/{}",
            input.package_scope.as_deref().unwrap_or("@lenso"),
            package_slug
        )
    });
    let area = input.area.unwrap_or_else(|| "data".to_owned());
    rust_console_area(&area)?;
    let label = input.label.unwrap_or_else(|| title_case(&module_id));
    let route = input
        .route
        .unwrap_or_else(|| format!("/{area}/{module_id}"));
    let registry_source = input
        .registry_source
        .unwrap_or_else(|| "installed".to_owned());
    let icon = input.icon.unwrap_or_else(|| default_icon(&area).to_owned());
    let capability = input
        .capability
        .unwrap_or_else(|| format!("{module_id}.read"));
    let surface_name = input.surface_name.unwrap_or_else(|| module_id.clone());
    let export_stem = export_stem_from_package_slug(&package_slug);
    let manifest_name = format!("{export_stem}Manifest");
    let module_name = format!("{export_stem}Module");
    let component_name = format!("{}ConsolePage", pascal_case(&module_id));
    let package_dir = runtime_console_root.join("packages").join(&package_slug);

    Ok(ConsolePackageContext {
        area,
        capability,
        component_name,
        icon,
        label,
        manifest_name,
        module_id,
        module_name,
        package_dir,
        package_name,
        package_private: input.package_private,
        package_slug,
        registry_source,
        route,
        runtime_console_api_version: input.runtime_console_api_version,
        surface_name,
    })
}

fn queue_module_files(
    pending_writes: &mut PendingWrites,
    module_dir: &Path,
    module_id: &str,
    console_surface: Option<&ConsolePackageContext>,
) -> Result<()> {
    queue_write(
        pending_writes,
        module_dir.join("Cargo.toml"),
        module_cargo_toml(module_id),
    );
    queue_write(
        pending_writes,
        module_dir.join("src/lib.rs"),
        "pub mod module;\n".to_owned(),
    );
    queue_write(
        pending_writes,
        module_dir.join("src/module.rs"),
        module_manifest(module_id, console_surface)?,
    );
    Ok(())
}

fn module_cargo_toml(module_id: &str) -> String {
    format!(
        r#"[package]
name = "{module_id}"
version = "0.1.0"
edition.workspace = true
license.workspace = true
publish.workspace = true
rust-version.workspace = true

[dependencies]
platform-core.workspace = true
platform-module.workspace = true

[lints]
workspace = true
"#
    )
}

fn module_manifest(
    module_id: &str,
    console_surface: Option<&ConsolePackageContext>,
) -> Result<String> {
    let imports = if console_surface.is_some() {
        "use platform_module::{ConsoleArea, ConsolePackage, ConsoleSurface, LinkedBinding, Module, ModuleManifest};"
    } else {
        "use platform_module::{LinkedBinding, Module, ModuleManifest};"
    };
    let manifest_builder = if let Some(console_surface) = console_surface {
        format!(
            r#"ModuleManifest::builder({})
        .capabilities(vec![{}.to_owned()])
        .console(vec![ConsoleSurface {{
            name: {}.to_owned(),
            label: {}.to_owned(),
            area: ConsoleArea::{},
            route: {}.to_owned(),
            package: ConsolePackage {{
                name: {}.to_owned(),
                export: {}.to_owned(),
            }},
            icon: Some({}.to_owned()),
            required_capabilities: vec![{}.to_owned()],
            navigation: Some(platform_module::ConsoleNavigation {{
                workspace: platform_module::ConsoleWorkspaceRef {{
                    id: {}.to_owned(),
                    label: {}.to_owned(),
                    icon: Some({}.to_owned()),
                }},
                group: None,
                order: Some(10),
            }}),
        }}])
        .build()"#,
            rust_string_literal(module_id),
            rust_string_literal(&console_surface.capability),
            rust_string_literal(&console_surface.surface_name),
            rust_string_literal(&console_surface.label),
            rust_console_area(&console_surface.area)?,
            rust_string_literal(&console_surface.route),
            rust_string_literal(&console_surface.package_name),
            rust_string_literal(&console_surface.module_name),
            rust_string_literal(&console_surface.icon),
            rust_string_literal(&console_surface.capability),
            rust_string_literal(module_id),
            rust_string_literal(&console_surface.label),
            rust_string_literal(&console_surface.icon),
        )
    } else {
        format!(
            "ModuleManifest::builder({}).build()",
            rust_string_literal(module_id)
        )
    };

    Ok(format!(
        r#"use platform_core::AppContext;
{imports}

/// Context-free manifest: serializable metadata only.
pub fn manifest() -> ModuleManifest {{
    {manifest_builder}
}}

/// The loaded module: manifest + linked behavior.
pub fn module(_ctx: &AppContext) -> Module {{
    Module::linked(manifest(), LinkedBinding::builder().build())
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn manifest_uses_module_name() {{
        assert_eq!(manifest().name, {});
    }}
}}
"#,
        rust_string_literal(module_id)
    ))
}

fn queue_host_module_files(
    pending_writes: &mut PendingWrites,
    module_dir: &Path,
    module_id: &str,
    console_surface: Option<&ConsolePackageContext>,
) -> Result<()> {
    queue_write(
        pending_writes,
        module_dir.join("mod.rs"),
        host_module_manifest(module_id, console_surface)?,
    );
    Ok(())
}

fn host_module_manifest(
    module_id: &str,
    console_surface: Option<&ConsolePackageContext>,
) -> Result<String> {
    let console_imports = if console_surface.is_some() {
        "use lenso::{ConsoleArea, ConsoleNavigation, ConsolePackage, ConsoleSurface, ConsoleWorkspaceRef};\n"
    } else {
        ""
    };
    let manifest_builder = if let Some(console_surface) = console_surface {
        format!(
            r#"ModuleManifest::builder(MODULE_NAME)
        .capabilities(vec![{}.to_owned()])
        .console(vec![ConsoleSurface {{
            name: {}.to_owned(),
            label: {}.to_owned(),
            area: ConsoleArea::{},
            route: {}.to_owned(),
            package: ConsolePackage {{
                name: {}.to_owned(),
                export: {}.to_owned(),
            }},
            icon: Some({}.to_owned()),
            required_capabilities: vec![{}.to_owned()],
            navigation: Some(ConsoleNavigation {{
                workspace: ConsoleWorkspaceRef {{
                    id: MODULE_NAME.to_owned(),
                    label: {}.to_owned(),
                    icon: Some({}.to_owned()),
                }},
                group: None,
                order: Some(10),
            }}),
        }}])
        .build()"#,
            rust_string_literal(&console_surface.capability),
            rust_string_literal(&console_surface.surface_name),
            rust_string_literal(&console_surface.label),
            rust_console_area(&console_surface.area)?,
            rust_string_literal(&console_surface.route),
            rust_string_literal(&console_surface.package_name),
            rust_string_literal(&console_surface.module_name),
            rust_string_literal(&console_surface.icon),
            rust_string_literal(&console_surface.capability),
            rust_string_literal(&console_surface.label),
            rust_string_literal(&console_surface.icon),
        )
    } else {
        "ModuleManifest::builder(MODULE_NAME).build()".to_owned()
    };

    Ok(format!(
        r#"use lenso::host::prelude::*;
{console_imports}
pub const MODULE_NAME: &str = {};

const MIGRATIONS: &[Migration] = &[];

pub fn linked_module() -> HostLinkedModule {{
    HostLinkedModule::manifest_only(MODULE_NAME, manifest, MIGRATIONS)
}}

fn manifest() -> ModuleManifest {{
    {manifest_builder}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    #[test]
    fn linked_module_exposes_manifest() {{
        let module = linked_module();
        let manifest = (module.manifest)();

        assert_eq!(module.module_name, MODULE_NAME);
        assert_eq!(manifest.name, MODULE_NAME);
        assert!(module.migrations.is_empty());
    }}
}}
"#,
        rust_string_literal(module_id)
    ))
}

fn update_host_modules_mod(
    pending_writes: &mut PendingWrites,
    modules_mod_path: &Path,
    module_name: &str,
) -> Result<()> {
    let file_source = read_text(modules_mod_path)?;
    queue_write(
        pending_writes,
        modules_mod_path.to_path_buf(),
        insert_before_first_needle(
            &file_source,
            &format!("pub mod {module_name};\n"),
            &["pub mod app;"],
        )?,
    );
    Ok(())
}

fn update_host_lib_for_created_module(
    pending_writes: &mut PendingWrites,
    host_lib_path: &Path,
    module_name: &str,
) -> Result<()> {
    let file_source = read_text(host_lib_path)?;
    let call = format!("modules::{module_name}::linked_module()");
    queue_write(
        pending_writes,
        host_lib_path.to_path_buf(),
        update_host_lib_for_linked_descriptor(&file_source, None, &call)?,
    );
    Ok(())
}

fn update_workspace_cargo_toml(
    pending_writes: &mut PendingWrites,
    cargo_toml_path: &Path,
    module_id: &str,
) -> Result<()> {
    let mut file_source = read_text(cargo_toml_path)?;
    file_source = insert_before_first_needle(
        &file_source,
        &format!("    \"modules/{module_id}\",\n"),
        &["    \"tools/", "]\n\n[workspace.package]"],
    )?;
    file_source = insert_before_first_needle(
        &file_source,
        &format!("{module_id} = {{ path = \"modules/{module_id}\" }}\n"),
        &[
            "generate-contracts =",
            "arch-check =",
            "remote-module-example =",
        ],
    )?;
    queue_write(pending_writes, cargo_toml_path.to_path_buf(), file_source);
    Ok(())
}

fn update_lenso_bootstrap_cargo_toml(
    pending_writes: &mut PendingWrites,
    cargo_toml_path: &Path,
    module_id: &str,
) -> Result<()> {
    let file_source = read_text(cargo_toml_path)?;
    queue_write(
        pending_writes,
        cargo_toml_path.to_path_buf(),
        insert_before_first_needle(
            &file_source,
            &format!("{module_id}.workspace = true\n"),
            &[
                "serde_json.workspace",
                "tracing.workspace",
                "\n[dev-dependencies]",
            ],
        )?,
    );
    Ok(())
}

fn update_lenso_bootstrap_lib(
    pending_writes: &mut PendingWrites,
    lenso_bootstrap_lib_path: &Path,
    module_crate: &str,
    module_id: &str,
) -> Result<()> {
    let file_source = read_text(lenso_bootstrap_lib_path)?;
    let entry = format!(
        r#"    LinkedModuleEntry {{
        module_name: "{module_id}",
        manifest: {module_crate}::module::manifest,
        load: {module_crate}::module::module,
        http_binding: None,
    }},
"#
    );
    queue_write(
        pending_writes,
        lenso_bootstrap_lib_path.to_path_buf(),
        insert_into_demo_linked_module_entries(&file_source, &entry)?,
    );
    Ok(())
}

fn queue_console_package(
    pending_writes: &mut PendingWrites,
    runtime_console_root: &Path,
    context: &ConsolePackageContext,
    update_host: bool,
) -> Result<()> {
    queue_console_package_files(pending_writes, context)?;
    if update_host {
        let paths = runtime_console_paths(runtime_console_root);
        update_runtime_console_package_json(pending_writes, &paths, context)?;
        update_tsconfig(pending_writes, &paths, &context.package_slug)?;
        update_oxlint_config(pending_writes, &paths, &context.package_slug)?;
        update_manifest_exports(pending_writes, &paths, context)?;
        update_module_exports(pending_writes, &paths, context)?;
    }
    Ok(())
}

fn queue_console_package_files(
    pending_writes: &mut PendingWrites,
    context: &ConsolePackageContext,
) -> Result<()> {
    queue_write(
        pending_writes,
        context.package_dir.join("package.json"),
        console_package_package_json(context)?,
    );
    queue_write(
        pending_writes,
        context.package_dir.join("console-surface.json"),
        console_surface_json(context)?,
    );
    queue_write(
        pending_writes,
        context.package_dir.join("console-surface.rs"),
        console_surface_rust(context)?,
    );
    queue_write(
        pending_writes,
        context.package_dir.join("src/manifest.ts"),
        console_package_manifest_ts(context)?,
    );
    queue_write(
        pending_writes,
        context.package_dir.join("src/page.tsx"),
        console_package_page_tsx(context),
    );
    queue_write(
        pending_writes,
        context.package_dir.join("src/index.tsx"),
        console_package_index_tsx(context),
    );
    queue_write(
        pending_writes,
        context.package_dir.join("src/index.test.tsx"),
        console_package_test_tsx(context),
    );
    Ok(())
}

fn console_package_package_json(context: &ConsolePackageContext) -> Result<String> {
    json_string_pretty(&json!({
        "exports": {
            ".": "./src/index.tsx",
        },
        "name": context.package_name,
        "peerDependencies": {
            "@lenso/runtime-console-api": context.runtime_console_api_version,
            "react": "^19.1.0",
            "react-dom": "^19.1.0",
        },
        "private": context.package_private,
        "scripts": {
            "check": "pnpm test && pnpm typecheck",
            "test": "echo \"console package smoke passed\"",
            "typecheck": "echo \"console package typecheck placeholder\"",
        },
        "type": "module",
        "version": "0.1.0",
    }))
}

fn console_surface_json(context: &ConsolePackageContext) -> Result<String> {
    json_string_pretty(&json!({
        "exportName": context.module_name,
        "id": context.module_id,
        "packageName": context.package_name,
        "source": context.registry_source,
        "surfaces": [
            {
                "area": context.area,
                "icon": context.icon,
                "label": context.label,
                "navigation": {
                    "order": 10,
                    "workspace": {
                        "icon": context.icon,
                        "id": context.module_id,
                        "label": context.label,
                    },
                },
                "requiredCapabilities": [context.capability],
                "route": context.route,
                "surfaceName": context.surface_name,
            },
        ],
        "version": "workspace",
    }))
}

fn console_surface_rust(context: &ConsolePackageContext) -> Result<String> {
    Ok(format!(
        r#"use platform_module::{{ConsoleArea, ConsolePackage, ConsoleSurface}};

ConsoleSurface {{
    name: {}.to_owned(),
    label: {}.to_owned(),
    area: ConsoleArea::{},
    route: {}.to_owned(),
    package: ConsolePackage {{
        name: {}.to_owned(),
        export: {}.to_owned(),
    }},
    icon: Some({}.to_owned()),
    required_capabilities: vec![{}.to_owned()],
    navigation: Some(platform_module::ConsoleNavigation {{
        workspace: platform_module::ConsoleWorkspaceRef {{
            id: {}.to_owned(),
            label: {}.to_owned(),
            icon: Some({}.to_owned()),
        }},
        group: None,
        order: Some(10),
    }}),
}}
"#,
        rust_string_literal(&context.surface_name),
        rust_string_literal(&context.label),
        rust_console_area(&context.area)?,
        rust_string_literal(&context.route),
        rust_string_literal(&context.package_name),
        rust_string_literal(&context.module_name),
        rust_string_literal(&context.icon),
        rust_string_literal(&context.capability),
        rust_string_literal(&context.module_id),
        rust_string_literal(&context.label),
        rust_string_literal(&context.icon),
    ))
}

fn console_package_manifest_ts(context: &ConsolePackageContext) -> Result<String> {
    Ok(format!(
        r#"import {{ defineConsolePackageManifest }} from "@lenso/runtime-console-api";

import consoleSurface from "../console-surface.json";

const consoleSurfaceContract = consoleSurface as unknown as {{
  readonly exportName: {};
  readonly id: {};
  readonly packageName: {};
  readonly source: {};
  readonly surfaces: readonly [
    {{
      readonly area: {};
      readonly icon: {};
      readonly label: {};
      readonly navigation: {{
        readonly order: 10;
        readonly workspace: {{
          readonly icon: {};
          readonly id: {};
          readonly label: {};
        }};
      }};
      readonly requiredCapabilities: readonly [{}];
      readonly route: {};
      readonly surfaceName: {};
    }},
  ];
  readonly version: "workspace";
}};

export const {} = defineConsolePackageManifest(
  consoleSurfaceContract
);
"#,
        ts_string_literal(&context.module_name)?,
        ts_string_literal(&context.module_id)?,
        ts_string_literal(&context.package_name)?,
        ts_string_literal(&context.registry_source)?,
        ts_string_literal(&context.area)?,
        ts_string_literal(&context.icon)?,
        ts_string_literal(&context.label)?,
        ts_string_literal(&context.icon)?,
        ts_string_literal(&context.module_id)?,
        ts_string_literal(&context.label)?,
        ts_string_literal(&context.capability)?,
        ts_string_literal(&context.route)?,
        ts_string_literal(&context.surface_name)?,
        context.manifest_name,
    ))
}

fn console_package_page_tsx(context: &ConsolePackageContext) -> String {
    format!(
        r#"export function {}() {{
  return (
    <main className="flex min-h-screen flex-col gap-3 px-6 py-5">
      <header>
        <p className="font-medium text-muted-foreground text-xs uppercase tracking-normal">
          {}
        </p>
        <h1 className="font-semibold text-2xl text-foreground">{}</h1>
      </header>
    </main>
  );
}}
"#,
        context.component_name, context.label, context.label
    )
}

fn console_package_index_tsx(context: &ConsolePackageContext) -> String {
    format!(
        r#"import {{ defineConsoleModule }} from "@lenso/runtime-console-api";

import {{ {} }} from "./manifest";
import {{ {} }} from "./page";

const [consoleSurface] = {}.surfaces;

export const {} = defineConsoleModule({{
  id: {}.id,
  surfaces: [
    {{
      area: consoleSurface.area,
      component: {},
      icon: consoleSurface.icon,
      label: consoleSurface.label,
      navigation: consoleSurface.navigation,
      path: consoleSurface.route,
    }},
  ],
}});

export {{ {} }} from "./manifest";
export {{ {} }} from "./page";
"#,
        context.manifest_name,
        context.component_name,
        context.manifest_name,
        context.module_name,
        context.manifest_name,
        context.component_name,
        context.manifest_name,
        context.component_name,
    )
}

fn console_package_test_tsx(context: &ConsolePackageContext) -> String {
    format!(
        r#"import {{ describe, expect, test }} from "vitest";

import {{ {}, {}, {} }} from ".";

const [consoleSurface] = {}.surfaces;

describe({}, () => {{
  test("exports a console module manifest and route", () => {{
    expect({}).toMatchObject({{
      exportName: {},
      id: {},
      packageName: {},
      surfaces: [{{ route: {} }}],
    }});
    expect({}).toMatchObject({{
      id: {}.id,
      surfaces: [
        {{
          area: consoleSurface.area,
          icon: consoleSurface.icon,
          label: consoleSurface.label,
          path: consoleSurface.route,
        }},
      ],
    }});
    expect({}.surfaces[0]?.component).toBe({});
  }});
}});
"#,
        context.component_name,
        context.manifest_name,
        context.module_name,
        context.manifest_name,
        ts_string_literal_lossy(&context.package_name),
        context.manifest_name,
        ts_string_literal_lossy(&context.module_name),
        ts_string_literal_lossy(&context.module_id),
        ts_string_literal_lossy(&context.package_name),
        ts_string_literal_lossy(&context.route),
        context.module_name,
        context.manifest_name,
        context.module_name,
        context.component_name,
    )
}

fn update_runtime_console_package_json(
    pending_writes: &mut PendingWrites,
    paths: &RuntimeConsolePaths,
    context: &ConsolePackageContext,
) -> Result<()> {
    let mut package_json = read_json(&paths.package_json_path)?;
    update_package_json_dependency(&mut package_json, &context.package_name, "workspace:*")?;
    let scripts = package_json
        .as_object_mut()
        .ok_or_else(|| anyhow!("Runtime Console package.json must be a JSON object"))?
        .entry("scripts")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("Runtime Console package.json scripts must be an object"))?;
    let current_test = scripts
        .get("test")
        .and_then(Value::as_str)
        .unwrap_or_default();
    scripts.insert(
        "test".to_owned(),
        Value::String(append_token(
            current_test,
            &format!("packages/{}/src", context.package_slug),
            "packages/console-package-api/src",
        )),
    );
    queue_write(
        pending_writes,
        paths.package_json_path.clone(),
        json_string_pretty(&package_json)?,
    );
    Ok(())
}

fn update_tsconfig(
    pending_writes: &mut PendingWrites,
    paths: &RuntimeConsolePaths,
    package_slug: &str,
) -> Result<()> {
    let mut tsconfig = read_json(&paths.tsconfig_path)?;
    let include = tsconfig
        .as_object_mut()
        .ok_or_else(|| anyhow!("Runtime Console tsconfig.json must be a JSON object"))?
        .entry("include")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| anyhow!("Runtime Console tsconfig include must be an array"))?;
    append_json_string(include, &format!("packages/{package_slug}/src"));
    queue_write(
        pending_writes,
        paths.tsconfig_path.clone(),
        json_string_pretty(&tsconfig)?,
    );
    Ok(())
}

fn update_oxlint_config(
    pending_writes: &mut PendingWrites,
    paths: &RuntimeConsolePaths,
    package_slug: &str,
) -> Result<()> {
    let file_source = read_text(&paths.oxlint_config_path)?;
    queue_write(
        pending_writes,
        paths.oxlint_config_path.clone(),
        insert_before_needle(
            &file_source,
            &format!("        \"packages/{package_slug}/src/**/*.{{ts,tsx}}\",\n"),
            "        \"vite.config.ts\",",
        )?,
    );
    Ok(())
}

fn update_manifest_exports(
    pending_writes: &mut PendingWrites,
    paths: &RuntimeConsolePaths,
    context: &ConsolePackageContext,
) -> Result<()> {
    let mut file_source = read_text(&paths.manifest_exports_path)?;
    file_source = insert_before_needle(
        &file_source,
        &format!(
            "import {{ {} }} from \"{}\";\n",
            context.manifest_name, context.package_name
        ),
        "export const consolePackageManifests",
    )?;
    file_source = insert_before_needle(
        &file_source,
        &format!("  {},\n", context.manifest_name),
        "] as const;",
    )?;
    queue_write(
        pending_writes,
        paths.manifest_exports_path.clone(),
        file_source,
    );
    Ok(())
}

fn update_module_exports(
    pending_writes: &mut PendingWrites,
    paths: &RuntimeConsolePaths,
    context: &ConsolePackageContext,
) -> Result<()> {
    let mut file_source = read_text(&paths.module_exports_path)?;
    file_source = insert_before_needle(
        &file_source,
        &format!(
            "import {{ {}, {} }} from \"{}\";\n",
            context.manifest_name, context.module_name, context.package_name
        ),
        "import {",
    )?;
    file_source = insert_before_needle(
        &file_source,
        &format!(
            "  [consolePackageKey({})]: {},\n",
            context.manifest_name, context.module_name
        ),
        "} satisfies ConsolePackageModuleExportsByKey;",
    )?;
    queue_write(
        pending_writes,
        paths.module_exports_path.clone(),
        file_source,
    );
    Ok(())
}

fn queue_remote_module_files(
    pending_writes: &mut PendingWrites,
    package_root: &Path,
    package_root_name: &str,
    context: &ConsolePackageContext,
) -> Result<()> {
    queue_write(
        pending_writes,
        package_root.join("lenso.service.json"),
        json_string_pretty(&remote_manifest_json(context, package_root_name))?,
    );
    queue_write(
        pending_writes,
        package_root.join("catalog-entry.json"),
        json_string_pretty(&remote_catalog_entry_json(context))?,
    );
    queue_write(
        pending_writes,
        package_root.join("module-services.local.json"),
        json_string_pretty(&remote_module_services_local_json(context))?,
    );
    queue_write(
        pending_writes,
        package_root.join("package.json"),
        remote_root_package_json(&context.module_id)?,
    );
    queue_write(
        pending_writes,
        package_root.join("README.md"),
        remote_package_readme(&context.module_id, package_root_name),
    );
    queue_write(
        pending_writes,
        package_root.join("RUNBOOK.md"),
        remote_package_runbook(&context.module_id),
    );
    queue_write(
        pending_writes,
        package_root.join("backend/README.md"),
        remote_backend_readme(&context.module_id),
    );
    queue_write(
        pending_writes,
        package_root.join("backend/package.json"),
        remote_backend_package_json(&context.module_id)?,
    );
    queue_write(
        pending_writes,
        package_root.join("backend/src/server.mjs"),
        remote_backend_server(context),
    );
    queue_write(
        pending_writes,
        package_root.join("backend/src/smoke.mjs"),
        remote_backend_smoke(&context.module_id),
    );
    queue_write(
        pending_writes,
        package_root.join("backend/openapi.yaml"),
        format!(
            "openapi: 3.1.0\ninfo:\n  title: {} Service\n  version: 0.1.0\npaths: {{}}\n",
            context.label
        ),
    );
    queue_write(
        pending_writes,
        package_root.join("contracts/README.md"),
        remote_contracts_readme(),
    );
    queue_write(
        pending_writes,
        package_root.join("contracts/events/.gitkeep"),
        String::new(),
    );
    queue_write(
        pending_writes,
        package_root.join("contracts/runtime-functions/.gitkeep"),
        String::new(),
    );
    queue_console_package_files(pending_writes, context)?;
    Ok(())
}

fn service_id_for_module(module_id: &str) -> String {
    format!("{module_id}-service")
}

fn remote_manifest_json(context: &ConsolePackageContext, package_root_name: &str) -> Value {
    let module = json!({
        "admin": {
            "entities": [
                {
                    "fields": [
                        {
                            "field_type": { "kind": "string" },
                            "label": "Email",
                            "name": "email",
                            "nullable": false,
                        },
                        {
                            "field_type": { "kind": "string" },
                            "label": "Name",
                            "name": "name",
                            "nullable": false,
                        },
                        {
                            "field_type": { "kind": "timestamp" },
                            "label": "Created At",
                            "name": "created_at",
                            "nullable": false,
                        },
                    ],
                    "label": "Contacts",
                    "name": "contacts",
                    "read_capability": context.capability,
                },
            ],
            "kind": "schema",
        },
        "capabilities": [context.capability],
        "console": [
            {
                "area": context.area,
                "icon": context.icon,
                "label": context.label,
                "name": context.surface_name,
                "navigation": {
                    "order": 10,
                    "workspace": {
                        "icon": context.icon,
                        "id": context.module_id,
                        "label": context.label,
                    },
                },
                "package": {
                    "export": context.module_name,
                    "name": context.package_name,
                },
                "required_capabilities": [context.capability],
                "route": context.route,
            },
        ],
        "http_routes": [
            {
                "capability": context.capability,
                "display_name": "Fetch Contact",
                "method": "GET",
                "path": "/contacts/{id}",
                "story_title": "Fetch Contact",
            },
        ],
        "lifecycle": {
            "activation_jobs": [
                {
                    "function_name": format!("{}.contacts.enrich.v1", context.module_id),
                    "input": { "reason": "worker_startup" },
                    "name": "sync contacts on startup",
                    "required": true,
                    "run_policy": "every_startup",
                },
            ],
            "startup_checks": [
                {
                    "function_name": format!("{}.contacts.enrich.v1", context.module_id),
                    "kind": "function_registered",
                    "name": "contacts enrich function is registered",
                    "required": true,
                },
            ],
        },
        "name": context.module_id,
        "runtime": {
            "functions": [
                {
                    "input_schema": format!("{}.contacts.enrich.v1", context.module_id),
                    "name": format!("{}.contacts.enrich.v1", context.module_id),
                    "queue": context.module_id,
                    "retry_policy": {
                        "initial_delay_ms": 1000,
                        "max_attempts": 3,
                    },
                    "version": 1,
                },
            ],
        },
        "version": "0.1.0",
    });
    let service_id = service_id_for_module(&context.module_id);
    json!({
        "compatibility": {
            "consolePackageApi": CONSOLE_BUNDLE_HOST_API,
            "remoteProtocolVersion": REMOTE_PROTOCOL_VERSION,
            "requiredHostFeatures": SUPPORTED_SERVICE_MODULE_FEATURES,
        },
        "deployment": {
            "commands": ["pnpm --dir backend dev"],
            "target": "container-paas",
        },
        "install": {
            "services": [
                {
                    "autoStart": true,
                    "command": "pnpm --dir backend dev",
                    "cwd": format!("../{package_root_name}"),
                    "name": service_id.clone(),
                    "readyTimeoutMs": 12000,
                    "readyUrl": "http://127.0.0.1:4100/lenso/service/v1/status",
                },
            ],
        },
        "modules": [module],
        "name": service_id,
        "protocol": "lenso.service.v1",
        "requiredEnv": ["PORT"],
        "statusPath": "/lenso/service/v1/status",
        "transports": ["http"],
        "version": "0.1.0",
    })
}

fn remote_module_services_local_json(context: &ConsolePackageContext) -> Value {
    let service_id = service_id_for_module(&context.module_id);
    json!({
        "modules": [
            {
                "moduleName": service_id,
                "services": [
                    {
                        "autoStart": true,
                        "command": "pnpm --dir backend dev",
                        "cwd": ".",
                        "name": "api",
                        "readyTimeoutMs": 12000,
                        "readyUrl": "http://127.0.0.1:4100/lenso/service/v1/status",
                    },
                ],
            },
        ],
        "version": 1,
    })
}

fn remote_catalog_entry_json(context: &ConsolePackageContext) -> Value {
    let service_id = service_id_for_module(&context.module_id);
    json!({
        "baseUrl": "https://example.com/lenso/service/v1",
        "compatibility": {
            "consolePackageApi": CONSOLE_BUNDLE_HOST_API,
            "remoteProtocolVersion": REMOTE_PROTOCOL_VERSION,
            "requiredHostFeatures": SUPPORTED_SERVICE_MODULE_FEATURES,
        },
        "consolePackages": [
            {
                "exportName": context.module_name,
                "packageName": context.package_name,
                "route": context.route,
            },
        ],
        "deployment": {
            "commands": ["pnpm --dir backend dev"],
            "target": "container-paas",
        },
        "install": {
            "services": [
                {
                    "command": "pnpm --dir backend dev",
                    "name": service_id.clone(),
                },
            ],
        },
        "manifestReference": "https://example.com/lenso/service/v1/manifest",
        "modules": [
            {
                "capabilities": [context.capability],
                "name": context.module_id,
                "version": "0.1.0",
            },
        ],
        "name": service_id.clone(),
        "service": {
            "requiredEnv": ["PORT"],
            "statusPath": "/lenso/service/v1/status",
            "statusUrl": "https://example.com/lenso/service/v1/status",
            "transports": ["http"],
        },
        "source": "service",
        "summary": format!("{} workspace and operations", context.label),
        "version": "0.1.0",
    })
}

fn remote_root_package_json(module_id: &str) -> Result<String> {
    json_string_pretty(&json!({
        "name": format!("lenso-{module_id}"),
        "private": true,
        "scripts": {
            "check": "pnpm --dir backend check && pnpm --dir console check",
            "dev": "pnpm --dir backend dev",
            "service:export": format!("lenso service export --module {} --module-services-file module-services.local.json", service_id_for_module(module_id)),
            "service:list": "lenso service list --module-services-file module-services.local.json",
            "service:start": format!("lenso service start {} api --module-services-file module-services.local.json", service_id_for_module(module_id)),
            "service:status": format!("lenso service status {} api --module-services-file module-services.local.json", service_id_for_module(module_id)),
            "service:stop": format!("lenso service stop {} api --module-services-file module-services.local.json", service_id_for_module(module_id)),
            "service:verify": "lenso service verify ./lenso.service.json",
            "smoke": "pnpm --dir backend smoke",
        },
        "type": "module",
        "version": "0.1.0",
    }))
}

fn remote_package_readme(module_id: &str, package_root_name: &str) -> String {
    format!(
        r#"# {}

Lenso service package scaffold.

## Shape

- `lenso.service.json`: install-time service manifest.
- `catalog-entry.json`: optional local catalog entry for discovery.
- `module-services.local.json`: local service lifecycle file for CLI checks.
- `RUNBOOK.md`: create, run, install, inspect, and troubleshoot steps.
- `backend/`: service backend implementation.
- `console/`: optional Runtime Console package.
- `contracts/`: module-owned event and runtime-function contracts.

## Local

```sh
pnpm dev
pnpm smoke
pnpm check
```

The backend prints the manifest and status URLs on startup. The generated
service lifecycle sample treats the status URL as the readiness check:

```sh
lenso service list --module-services-file module-services.local.json
lenso service status {module_id}-service api --module-services-file module-services.local.json
```

## Install

Expose the service protocol from a stable base URL such as:

```text
GET https://example.com/lenso/service/v1/manifest
GET https://example.com/lenso/service/v1/status
```

Use `catalog-entry.json` as the local discovery record, or add the manifest
URL directly:

```sh
lenso service install https://example.com/lenso/service/v1/manifest
```

If you want it to appear in Available Modules before installing it, add a local
catalog entry:

```sh
lenso module catalog add https://example.com/lenso/service/v1/manifest
```

If the manifest is inspected from a local file, provide the runtime base URL:

```sh
lenso service install ./lenso.service.json --base-url https://example.com/lenso/service/v1
```

The generated `lenso.service.json` also declares a local service process:

```json
{{
  "install": {{
    "services": [
      {{
        "name": "api",
        "command": "pnpm --dir backend dev",
        "cwd": "../{package_root_name}",
        "readyUrl": "http://127.0.0.1:4100/lenso/service/v1/status"
      }}
    ]
  }}
}}
```

Adjust `cwd` if the host app and this package are not sibling directories.
Services are stored in the host `.lenso/module-services.json` and started
before the host loads service-provided modules on API/worker startup.

## Operator Loop

```sh
lenso service install http://127.0.0.1:4100/lenso/service/v1/manifest
lenso service list
lenso service doctor {module_id} --json
```

Runtime Console should show the service as installed, configured, and ready,
and the provided `{module_id}` module as loaded once the host API/worker restart
with the configured provider source.

This scaffold lives in `{package_root_name}` and should stay separate from a
host application's linked `modules/` workspace.
"#,
        title_case(module_id),
    )
}

fn remote_package_runbook(module_id: &str) -> String {
    format!(
        r#"# Service Runbook

## 1. Create

This package is an independently running Lenso service. The service provider is
`{module_id}-service`; it provides the `{module_id}` module. Keep the module
name, capabilities, route names, runtime function names, and event names stable
if you later extract it from a linked module.

## 2. Run

```sh
pnpm install
pnpm dev
```

The backend listens on `PORT` or `4100` and exposes:

```text
GET http://127.0.0.1:4100/lenso/service/v1/manifest
GET http://127.0.0.1:4100/lenso/service/v1/status
```

## 3. Inspect Local Service State

From this package root:

```sh
lenso service list --module-services-file module-services.local.json
lenso service status {module_id}-service api --module-services-file module-services.local.json
lenso service start {module_id}-service api --module-services-file module-services.local.json
lenso service stop {module_id}-service api --module-services-file module-services.local.json
```

## 4. Install Into A Host

From the host app root:

```sh
lenso service install http://127.0.0.1:4100/lenso/service/v1/manifest
lenso service doctor {module_id} --json
```

Restart the host API and worker after installation so they load the configured
service provider source and any service process declaration.

## 5. Observe

Runtime Console evidence should stay on the host side:

- Modules shows installed / configured / ready.
- Remote Calls shows proxied HTTP, action, and runtime calls.
- Runtime Story links host-owned runtime work back to this service.
- Technical Operations records retries, queues, and operational failures.

## 6. Troubleshoot

| State | Meaning | Next action |
| --- | --- | --- |
| `manifest_unreachable` | Host cannot fetch the service manifest. | Start the backend or fix `REMOTE_MODULES`. |
| `service_not_ready` | The lifecycle ready URL failed. | Run the service or inspect its logs. |
| `restart_pending` | `.env` or service state changed after host startup. | Restart API and worker. |
| `stale_state` | Lock/pid state exists but readiness failed. | Stop the service or remove stale lock/pid files. |
"#
    )
}

fn remote_backend_readme(module_id: &str) -> String {
    format!(
        r#"# Service Backend

The generated Node server exposes the `{module_id}-service` manifest at:

```text
GET /lenso/service/v1/manifest
GET /lenso/service/v1/status
```

Run it locally:

```sh
cd backend
pnpm install
pnpm dev
```

Replace `src/server.mjs` with the language or framework you prefer as the
module grows.

The backend should expose the Lenso service protocol, including a stable
manifest endpoint and module-scoped schema-admin, action, HTTP proxy, or
runtime-function endpoints.

The host owns auth, capability enforcement, proxy policy, runtime queues,
retries, Runtime Stories, and Technical Operations records.
"#
    )
}

fn remote_backend_package_json(module_id: &str) -> Result<String> {
    json_string_pretty(&json!({
        "dependencies": {
            "@lenso/service-kit": "^0.1.0",
        },
        "name": format!("{}-backend", service_id_for_module(module_id)),
        "private": true,
        "scripts": {
            "check": "node src/smoke.mjs",
            "dev": "node src/server.mjs",
            "smoke": "node src/smoke.mjs",
            "start": "node src/server.mjs",
        },
        "type": "module",
        "version": "0.1.0",
    }))
}

fn remote_backend_server(context: &ConsolePackageContext) -> String {
    let service_id = service_id_for_module(&context.module_id);
    format!(
        r#"import {{
  defineModule,
  defineSchemaEntity,
  defineService,
  everyStartup,
  getRoute,
  lifecycle,
  runtimeFunction,
  schemaAdmin,
  serveService,
  textField,
  timestampField,
}} from "@lenso/service-kit";

const moduleName = {};
const serviceName = {};
const readCapability = {};
const enrichFunctionName = {};

const contacts = [
  {{
    id: "contact_1",
    created_at: "2026-01-01T00:00:00Z",
    email: "ada@example.com",
    name: "Ada Lovelace",
  }},
  {{
    id: "contact_2",
    created_at: "2026-01-02T00:00:00Z",
    email: "grace@example.com",
    name: "Grace Hopper",
  }},
];

const contactsEntity = defineSchemaEntity({{
  fields: [textField("email"), textField("name"), timestampField("created_at")],
  label: "Contacts",
  name: "contacts",
  readCapability,
}});

const providedModule = defineModule({{
  admin: schemaAdmin([contactsEntity]),
  capabilities: [readCapability],
  console: [
    {{
      area: {},
      icon: {},
      label: {},
      name: {},
      navigation: {{
        order: 10,
        workspace: {{
          icon: {},
          id: {},
          label: {},
        }},
      }},
      package: {{
        export: {},
        name: {},
      }},
      required_capabilities: [readCapability],
      route: {},
    }},
  ],
  httpRoutes: [
    getRoute("/contacts/{{id}}", {{
      capability: readCapability,
      displayName: "Fetch Contact",
      storyTitle: "Fetch Contact",
    }}),
  ],
  lifecycle: lifecycle({{
    activationJobs: [
      everyStartup(
        "sync contacts on startup",
        enrichFunctionName,
        {{
          input: {{ reason: "worker_startup" }},
        }}
      ),
    ],
    startupChecks: [
      {{
        function_name: enrichFunctionName,
        kind: "function_registered",
        name: "contacts enrich function is registered",
        required: true,
      }},
    ],
  }}),
  name: moduleName,
  runtimeFunctions: [
    runtimeFunction(enrichFunctionName, {{
      inputSchema: enrichFunctionName,
      queue: moduleName,
      retryPolicy: {{
        initial_delay_ms: 1000,
        max_attempts: 3,
      }},
      version: 1,
    }}),
  ],
  version: "0.1.0",
}});

const service = defineService({{
  compatibility: {{
    console_package_api: "1",
    remote_protocol_version: "1",
    required_host_features: ["service.status"],
  }},
  deployment: {{
    commands: ["pnpm --dir backend dev"],
    target: "container-paas",
  }},
  install: {{
    services: [
      {{
        autoStart: true,
        command: "pnpm --dir backend dev",
        name: serviceName,
      }},
    ],
  }},
  modules: [providedModule],
  name: serviceName,
  requiredEnv: ["PORT"],
  statusPath: "/lenso/service/v1/status",
  transports: ["http"],
  version: "0.1.0",
}});

await serveService(service, {{
  modules: {{
    [moduleName]: {{
      data: {{
        contacts: {{
          detail: async (id) => contacts.find((contact) => contact.id === id),
          list: async ({{ limit }}) => ({{
            next_cursor: null,
            records: contacts.slice(0, limit),
          }}),
        }},
      }},
      http: {{
        "GET /contacts/{{id}}": ({{ params }}) =>
          contacts.find((contact) => contact.id === params.id) ?? null,
      }},
      runtime: {{
        [enrichFunctionName]: ({{ input }}) => {{
          const contactId = input?.contact_id;
          const contact = contacts.find((item) => item.id === contactId);
          return {{
            contact,
            enriched: Boolean(contact),
            source: moduleName,
          }};
        }},
      }},
    }},
  }},
  port: Number(process.env.PORT ?? 4100),
  onReady: ({{ manifestUrl, statusUrl }}) => {{
    console.log({} + manifestUrl);
    console.log("Status: " + statusUrl);
  }},
}});
"#,
        ts_string_literal_lossy(&context.module_id),
        ts_string_literal_lossy(&service_id),
        ts_string_literal_lossy(&context.capability),
        ts_string_literal_lossy(&format!("{}.contacts.enrich.v1", context.module_id)),
        ts_string_literal_lossy(&context.area),
        ts_string_literal_lossy(&context.icon),
        ts_string_literal_lossy(&context.label),
        ts_string_literal_lossy(&context.surface_name),
        ts_string_literal_lossy(&context.icon),
        ts_string_literal_lossy(&context.module_id),
        ts_string_literal_lossy(&context.label),
        ts_string_literal_lossy(&context.module_name),
        ts_string_literal_lossy(&context.package_name),
        ts_string_literal_lossy(&context.route),
        ts_string_literal_lossy(&format!("{} service manifest: ", service_id)),
    )
}

fn remote_backend_smoke(module_id: &str) -> String {
    let service_id = service_id_for_module(module_id);
    format!(
        r#"import {{ spawn }} from "node:child_process";

const childProcess = spawn(process.execPath, ["src/server.mjs"], {{
  env: {{ ...process.env, PORT: "0" }},
  stdio: ["ignore", "pipe", "inherit"],
}});

const timeout = setTimeout(() => childProcess.kill(), 3000);

try {{
  let manifestUrl = "";
  for await (const chunk of childProcess.stdout) {{
    manifestUrl = String(chunk).match(new RegExp("http://\\S+", "u"))?.[0] ?? "";
    if (manifestUrl) {{
      break;
    }}
  }}

  if (!manifestUrl) {{
    throw new Error("manifest URL was not printed");
  }}

  const manifest = await fetch(manifestUrl).then((response) => response.json());
  if (manifest.name !== {} || manifest.protocol !== "lenso.service.v1") {{
    throw new Error("service manifest response did not match {service_id}");
  }}
  const moduleManifest = manifest.modules?.find((item) => item.name === {});
  if (!moduleManifest) {{
    throw new Error("service manifest did not provide {module_id}");
  }}
  const serviceBaseUrl = manifestUrl.slice(0, -"/manifest".length);
  const moduleBaseUrl = serviceBaseUrl + "/modules/{module_id}";
  const contact = await fetch(moduleBaseUrl + "/contacts/contact_1").then(
    (response) => response.json()
  );
  if (contact.email !== "ada@example.com") {{
    throw new Error("HTTP route response did not match {module_id}");
  }}
  const runtimeResult = await fetch(
    moduleBaseUrl + "/runtime/functions/{module_id}.contacts.enrich.v1/invoke",
    {{
      body: JSON.stringify({{
        actor: {{ id: "worker", kind: "service", scopes: [] }},
        attempt: 1,
        correlation_id: "corr_1",
        function_name: "{module_id}.contacts.enrich.v1",
        function_run_id: "fnrun_1",
        input: {{ contact_id: "contact_1" }},
        request_id: "req_1",
        trace: {{ span_id: "span_1", trace_id: "trace_1" }},
      }}),
      headers: {{ "content-type": "application/json" }},
      method: "POST",
    }}
  ).then((response) => response.json());
  if (!runtimeResult.output?.enriched) {{
    throw new Error("runtime function response did not match {module_id}");
  }}

  console.log("{module_id} backend smoke passed");
}} finally {{
  clearTimeout(timeout);
  childProcess.kill();
}}
"#,
        ts_string_literal_lossy(&service_id),
        ts_string_literal_lossy(module_id)
    )
}

fn remote_contracts_readme() -> String {
    "# Module-owned contracts\n\nKeep event and runtime-function JSON Schema contracts here.\n\nThe host may validate these before installing or enabling a service-provided module.\n".to_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConsolePackagePlanItem {
    export_name: String,
    package_name: String,
}

#[derive(Debug)]
struct RuntimeConsolePaths {
    manifest_exports_path: PathBuf,
    module_exports_path: PathBuf,
    oxlint_config_path: PathBuf,
    package_json_path: PathBuf,
    tsconfig_path: PathBuf,
}

fn repo_paths(repo_root: &Path) -> RepoPaths {
    RepoPaths {
        lenso_bootstrap_cargo_toml_path: repo_root.join("crates/lenso-bootstrap/Cargo.toml"),
        lenso_bootstrap_lib_path: repo_root.join("crates/lenso-bootstrap/src/lib.rs"),
        cargo_toml_path: repo_root.join("Cargo.toml"),
    }
}

fn is_framework_workspace_root(path: &Path) -> bool {
    path.join("Cargo.toml").exists() && path.join("crates/lenso-bootstrap").exists()
}

fn is_starter_host_root(path: &Path) -> bool {
    path.join("Cargo.toml").exists()
        && path.join("src/lib.rs").exists()
        && path.join("src/modules/mod.rs").exists()
}

fn queue_write(pending_writes: &mut PendingWrites, file_path: PathBuf, contents: String) {
    pending_writes.insert(file_path, contents);
}

fn write_pending_files(pending_writes: &PendingWrites) -> Result<()> {
    for (file_path, contents) in pending_writes {
        write_file(file_path, contents.as_bytes())?;
    }
    Ok(())
}

fn json_string_pretty(value: &Value) -> Result<String> {
    let mut contents = serde_json::to_string_pretty(value)?;
    contents.push('\n');
    Ok(contents)
}

fn append_json_string(items: &mut Vec<Value>, item: &str) {
    if items.iter().any(|value| value.as_str() == Some(item)) {
        return;
    }
    items.push(Value::String(item.to_owned()));
}

fn append_token(value: &str, token: &str, before_token: &str) -> String {
    let mut tokens = value.split(' ').collect::<Vec<_>>();
    if tokens.contains(&token) {
        return value.to_owned();
    }
    let insert_index = tokens
        .iter()
        .position(|candidate| *candidate == before_token)
        .unwrap_or(tokens.len());
    tokens.insert(insert_index, token);
    tokens.join(" ")
}

fn insert_before_first_needle(file_source: &str, entry: &str, needles: &[&str]) -> Result<String> {
    if file_source.contains(entry.trim()) {
        return Ok(file_source.to_owned());
    }
    for needle in needles {
        if file_source.contains(needle) {
            return insert_before_needle(file_source, entry, needle);
        }
    }
    Ok(format!("{}\n{entry}", file_source.trim_end()))
}

fn insert_into_demo_linked_module_entries(file_source: &str, entry: &str) -> Result<String> {
    if file_source.contains(entry.trim()) {
        return Ok(file_source.to_owned());
    }
    let entries_start = file_source
        .find("const DEMO_LINKED_MODULE_ENTRIES")
        .ok_or_else(|| anyhow!("Could not find DEMO_LINKED_MODULE_ENTRIES in lenso-bootstrap"))?;
    let entries_end = file_source[entries_start..]
        .find("];")
        .map(|index| entries_start + index)
        .ok_or_else(|| anyhow!("Could not find DEMO_LINKED_MODULE_ENTRIES closing bracket"))?;
    Ok(format!(
        "{}{}{}",
        &file_source[..entries_end],
        entry,
        &file_source[entries_end..]
    ))
}

fn slugify(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_dash = false;
    for character in value.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            last_was_dash = false;
        } else if !last_was_dash && !output.is_empty() {
            output.push('-');
            last_was_dash = true;
        }
    }
    output.trim_matches('-').to_owned()
}

fn snake_case(value: &str) -> String {
    value.replace('-', "_")
}

fn camel_case(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = false;
    for character in value.chars() {
        if character == '-' {
            uppercase_next = true;
        } else if uppercase_next {
            output.extend(character.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(character);
        }
    }
    output
}

fn pascal_case(value: &str) -> String {
    let camel = camel_case(value);
    let mut chars = camel.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    format!("{}{}", first.to_uppercase(), chars.collect::<String>())
}

fn export_stem_from_package_slug(package_slug: &str) -> String {
    let normalized = package_slug
        .strip_suffix("-console")
        .unwrap_or(package_slug);
    format!("{}Console", camel_case(normalized))
}

fn rust_console_area(area_name: &str) -> Result<&'static str> {
    match area_name {
        "configuration" => Ok("Configuration"),
        "data" => Ok("Data"),
        "operations" => Ok("Operations"),
        "runtime" => Ok("Runtime"),
        other => bail!("Unsupported console surface area: {other}"),
    }
}

fn title_case(value: &str) -> String {
    value
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!("{}{}", first.to_uppercase(), chars.collect::<String>())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn default_icon(area_name: &str) -> &'static str {
    if area_name == "runtime" {
        "workflow"
    } else {
        "database"
    }
}

fn rust_string_literal(value: &str) -> String {
    format!("{value:?}")
}

fn rust_string_array_literal(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| rust_string_literal(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn json_string_array(value: &Value, context: &str) -> Result<Vec<String>> {
    let array = value
        .as_array()
        .ok_or_else(|| anyhow!("Linked module descriptor {context} must be an array"))?;
    array
        .iter()
        .map(|value| {
            let value = value
                .as_str()
                .ok_or_else(|| {
                    anyhow!("Linked module descriptor {context} entries must be strings")
                })?
                .trim();
            if value.is_empty() {
                bail!("Linked module descriptor {context} entries must be non-empty");
            }
            Ok(value.to_owned())
        })
        .collect()
}

fn ts_string_literal(value: &str) -> Result<String> {
    serde_json::to_string(value).context("serialize TypeScript string literal")
}

fn ts_string_literal_lossy(value: &str) -> String {
    ts_string_literal(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn validate_remote_module_manifest(manifest: Value) -> Result<Value> {
    if !manifest.is_object() {
        bail!("Remote module manifest must be a JSON object");
    }
    let name = string_field(&manifest, "name")?;
    if name.trim().is_empty() {
        bail!("Remote module manifest name is required");
    }
    let version = string_field(&manifest, "version")?;
    if version.trim().is_empty() {
        bail!("Remote module manifest version is required");
    }
    if manifest.get("source").and_then(Value::as_str) != Some("remote") {
        bail!("Remote module manifest source must be remote");
    }
    if !manifest.get("capabilities").is_some_and(Value::is_array) {
        bail!("Remote module manifest capabilities must be an array");
    }
    if !manifest.get("console").is_some_and(Value::is_array) {
        bail!("Remote module manifest console must be an array");
    }
    Ok(manifest)
}

fn is_service_manifest(manifest: &Value) -> bool {
    manifest.get("protocol").and_then(Value::as_str) == Some("lenso.service.v1")
        || manifest.get("modules").is_some_and(Value::is_array)
}

fn is_service_package_manifest(manifest: &Value) -> bool {
    manifest.get("protocol").and_then(Value::as_str) == Some("lenso.service-package.v1")
}

fn is_module_release_descriptor(manifest: &Value) -> bool {
    manifest.get("protocol").and_then(Value::as_str) == Some("lenso.module-release.v1")
}

fn validate_module_release_descriptor(manifest: Value) -> Result<Value> {
    if !manifest.is_object() {
        bail!("Module release must be a JSON object");
    }
    if manifest.get("protocol").and_then(Value::as_str) != Some("lenso.module-release.v1") {
        bail!("Module release protocol must be lenso.module-release.v1");
    }
    let name = string_field(&manifest, "name")?.trim();
    if name.is_empty() {
        bail!("Module release name is required");
    }
    let version = string_field(&manifest, "version")?.trim();
    if version.is_empty() {
        bail!("Module release version is required");
    }
    let source = string_field(&manifest, "source")?.trim();
    if !matches!(source, "service" | "linked" | "bundled") {
        bail!("Module release source must be service, linked, or bundled");
    }
    validate_service_string_array(manifest.get("capabilities"), "$.capabilities")?;
    validate_service_string_array(manifest.get("dependencies"), "$.dependencies")?;
    if source == "service" {
        module_release_provider(&manifest)?;
    } else if let Some(provider) = manifest.get("provider")
        && !provider.is_object()
    {
        bail!("Module release provider must be an object");
    }
    Ok(manifest)
}

fn module_release_source(manifest: &Value) -> Result<&str> {
    let source = string_field(manifest, "source")?.trim();
    if matches!(source, "service" | "linked" | "bundled") {
        Ok(source)
    } else {
        bail!("Module release source must be service, linked, or bundled");
    }
}

fn module_release_provider(manifest: &Value) -> Result<&Map<String, Value>> {
    manifest
        .get("provider")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("Module release provider must be an object"))
}

fn module_release_service_reference(release_reference: &str, release: &Value) -> Result<String> {
    let provider = module_release_provider(release)?;
    let service_reference = provider
        .get("servicePackage")
        .or_else(|| provider.get("service_package"))
        .or_else(|| provider.get("serviceManifest"))
        .or_else(|| provider.get("service_manifest"))
        .or_else(|| provider.get("manifestReference"))
        .or_else(|| provider.get("manifest_reference"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "Module release provider.servicePackage or provider.serviceManifest is required"
            )
        })?;
    resolve_reference_from_base(
        release_reference,
        service_reference,
        "module release provider",
    )
}

fn ensure_module_release_matches_service_manifest(
    release: &Value,
    service_manifest: &Value,
) -> Result<()> {
    let release_name = string_field(release, "name")?.trim();
    let release_version = string_field(release, "version")?.trim();
    let service_version = string_field(service_manifest, "version")?.trim();
    let service_name = string_field(service_manifest, "name")?.trim();
    let module = service_manifest
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|module| module.get("name").and_then(Value::as_str) == Some(release_name))
        .ok_or_else(|| {
            anyhow!("Module release `{release_name}` is not provided by service `{service_name}`")
        })?;
    let module_version = module
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or(service_version)
        .trim();
    if release_version != module_version {
        bail!(
            "Module release `{release_name}` version {release_version} points at module version {module_version}"
        );
    }
    Ok(())
}

fn validate_service_package_manifest(manifest: Value) -> Result<Value> {
    if !manifest.is_object() {
        bail!("Service package must be a JSON object");
    }
    if manifest.get("protocol").and_then(Value::as_str) != Some("lenso.service-package.v1") {
        bail!("Service package protocol must be lenso.service-package.v1");
    }
    let name = string_field(&manifest, "name")?.trim();
    if name.is_empty() {
        bail!("Service package name is required");
    }
    let version = string_field(&manifest, "version")?.trim();
    if version.is_empty() {
        bail!("Service package version is required");
    }
    let service_manifest = string_field(&manifest, "serviceManifest")?.trim();
    if service_manifest.is_empty() {
        bail!("Service package serviceManifest is required");
    }
    let modules = manifest
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Service package modules must be an array"))?;
    if modules.is_empty() {
        bail!("Service package modules must not be empty");
    }
    let mut module_names = BTreeSet::new();
    for module in modules {
        let Some(module_name) = module.as_str().map(str::trim) else {
            bail!("Service package modules entries must be strings");
        };
        if module_name.is_empty() {
            bail!("Service package module name is required");
        }
        if !module_names.insert(module_name.to_owned()) {
            bail!("Service package module `{module_name}` is declared more than once");
        }
    }
    Ok(manifest)
}

fn service_package_manifest_reference(package_reference: &str, package: &Value) -> Result<String> {
    let service_manifest = string_field(package, "serviceManifest")?.trim();
    resolve_reference_from_base(package_reference, service_manifest, "serviceManifest")
}

fn resolve_reference_from_base(
    base_reference: &str,
    reference: &str,
    field_name: &str,
) -> Result<String> {
    if reference.starts_with("http://")
        || reference.starts_with("https://")
        || reference.starts_with("file://")
        || Path::new(reference).is_absolute()
    {
        return Ok(reference.to_owned());
    }
    if base_reference.starts_with("http://") || base_reference.starts_with("https://") {
        return Ok(reqwest::Url::parse(base_reference)
            .with_context(|| format!("parse base URL {base_reference}"))?
            .join(reference)
            .with_context(|| format!("resolve {field_name} {reference}"))?
            .to_string());
    }
    let package_path = base_reference
        .strip_prefix("file://")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(base_reference));
    let package_dir = package_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(package_dir.join(reference).to_string_lossy().to_string())
}

fn ensure_service_package_matches_manifest(
    package: &Value,
    service_manifest: &Value,
) -> Result<()> {
    let package_name = string_field(package, "name")?.trim();
    let service_name = string_field(service_manifest, "name")?.trim();
    if package_name != service_name {
        bail!("Service package `{package_name}` points at service `{service_name}`");
    }
    let package_version = string_field(package, "version")?.trim();
    let service_version = string_field(service_manifest, "version")?.trim();
    if package_version != service_version {
        bail!(
            "Service package `{package_name}` version {package_version} points at service version {service_version}"
        );
    }
    let package_modules = package
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .collect::<BTreeSet<_>>();
    let service_modules = service_manifest
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|module| module.get("name").and_then(Value::as_str))
        .map(str::trim)
        .collect::<BTreeSet<_>>();
    if package_modules != service_modules {
        bail!("Service package `{package_name}` modules do not match its service manifest");
    }
    Ok(())
}

fn validate_service_manifest(manifest: Value) -> Result<Value> {
    if !manifest.is_object() {
        bail!("Service manifest must be a JSON object");
    }
    let name = string_field(&manifest, "name")?;
    if name.trim().is_empty() {
        bail!("Service manifest name is required");
    }
    let version = string_field(&manifest, "version")?;
    if version.trim().is_empty() {
        bail!("Service manifest version is required");
    }
    validate_service_provider(&manifest)?;
    validate_named_object_array(manifest.get("config"), "$.config", "key")?;
    validate_named_object_array(manifest.get("env"), "$.env", "name")?;
    validate_service_string_array(
        manifest
            .get("requiredEnv")
            .or_else(|| manifest.get("required_env")),
        "$.requiredEnv",
    )?;
    validate_service_compatibility(&manifest)?;
    validate_service_local_process(
        manifest
            .get("localProcess")
            .or_else(|| manifest.get("local_process")),
    )?;
    validate_service_install(&manifest)?;
    let modules = manifest
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Service manifest modules must be an array"))?;
    if modules.is_empty() {
        bail!("Service manifest modules must not be empty");
    }
    let mut module_names = BTreeSet::new();
    for (index, module) in modules.iter().enumerate() {
        if !module.is_object() {
            bail!("Service manifest modules entries must be objects");
        }
        let module_name = string_field(module, "name")?.trim();
        if module_name.is_empty() {
            bail!("Service manifest module name is required");
        }
        if !module_names.insert(module_name.to_owned()) {
            bail!("Service manifest module `{module_name}` is declared more than once");
        }
        validate_service_string_array(
            module.get("capabilities"),
            &format!("$.modules[{index}].capabilities"),
        )?;
        validate_service_string_array(
            module.get("dependencies"),
            &format!("$.modules[{index}].dependencies"),
        )?;
    }
    Ok(manifest)
}

fn validate_service_provider(manifest: &Value) -> Result<()> {
    let Some(provider) = manifest.get("provider") else {
        return Ok(());
    };
    let provider = provider
        .as_object()
        .ok_or_else(|| anyhow!("Service manifest $.provider must be an object"))?;
    require_service_string(provider.get("name"), "$.provider.name")
}

fn validate_service_compatibility(manifest: &Value) -> Result<()> {
    let Some(compatibility) = manifest.get("compatibility") else {
        return Ok(());
    };
    let compatibility = compatibility
        .as_object()
        .ok_or_else(|| anyhow!("Service manifest $.compatibility must be an object"))?;
    validate_service_string_array(
        compatibility
            .get("requiredHostFeatures")
            .or_else(|| compatibility.get("required_host_features")),
        "$.compatibility.requiredHostFeatures",
    )
}

fn validate_named_object_array(value: Option<&Value>, path: &str, name_field: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let array = value
        .as_array()
        .ok_or_else(|| anyhow!("Service manifest {path} must be an array"))?;
    for (index, item) in array.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| anyhow!("Service manifest {path}[{index}] must be an object"))?;
        require_service_string(
            object.get(name_field),
            &format!("{path}[{index}].{name_field}"),
        )?;
    }
    Ok(())
}

fn validate_service_local_process(value: Option<&Value>) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let object = value
        .as_object()
        .ok_or_else(|| anyhow!("Service manifest $.localProcess must be an object"))?;
    require_service_string(object.get("command"), "$.localProcess.command")
}

fn validate_service_install(manifest: &Value) -> Result<()> {
    let Some(install) = manifest.get("install") else {
        return Ok(());
    };
    let install = install
        .as_object()
        .ok_or_else(|| anyhow!("Service manifest $.install must be an object"))?;
    let Some(services) = install.get("services") else {
        return Ok(());
    };
    let services = services
        .as_array()
        .ok_or_else(|| anyhow!("Service manifest $.install.services must be an array"))?;
    for (index, service) in services.iter().enumerate() {
        let service = service.as_object().ok_or_else(|| {
            anyhow!("Service manifest $.install.services[{index}] must be an object")
        })?;
        require_service_string(
            service.get("name"),
            &format!("$.install.services[{index}].name"),
        )?;
        require_service_string(
            service.get("command"),
            &format!("$.install.services[{index}].command"),
        )?;
    }
    Ok(())
}

fn validate_service_string_array(value: Option<&Value>, path: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let array = value
        .as_array()
        .ok_or_else(|| anyhow!("Service manifest {path} must be an array"))?;
    for (index, item) in array.iter().enumerate() {
        require_service_string(Some(item), &format!("{path}[{index}]"))?;
    }
    Ok(())
}

fn require_service_string(value: Option<&Value>, path: &str) -> Result<()> {
    if value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Ok(());
    }
    bail!("Service manifest {path} must be a non-empty string")
}

fn service_module_install_manifests(
    service_manifest: &Value,
    manifest_reference: &str,
    base_url: &str,
) -> Result<Vec<Value>> {
    let service_version = string_field(service_manifest, "version")?.trim();
    let modules = service_manifest
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Service manifest modules must be an array"))?;

    modules
        .iter()
        .map(|module| {
            let mut module_manifest = module.clone();
            let object = module_manifest
                .as_object_mut()
                .ok_or_else(|| anyhow!("Service manifest modules entries must be objects"))?;
            object.insert("source".to_owned(), json!("remote"));
            object
                .entry("version".to_owned())
                .or_insert_with(|| json!(service_version));
            object
                .entry("capabilities".to_owned())
                .or_insert_with(|| json!([]));
            object
                .entry("console".to_owned())
                .or_insert_with(|| json!([]));
            copy_optional_manifest_field(service_manifest, &mut module_manifest, "compatibility");
            copy_optional_manifest_field(service_manifest, &mut module_manifest, "deployment");
            module_manifest["service"] =
                service_module_provider_metadata(service_manifest, manifest_reference, base_url)?;
            validate_remote_module_manifest(module_manifest)
        })
        .collect()
}

fn service_module_provider_metadata(
    service_manifest: &Value,
    manifest_reference: &str,
    base_url: &str,
) -> Result<Value> {
    let mut service = json!({
        "baseUrl": base_url,
        "manifestReference": manifest_reference,
        "name": string_field(service_manifest, "name")?,
        "statusPath": service_status_path(service_manifest),
        "statusUrl": service_status_url(service_manifest, base_url),
        "version": string_field(service_manifest, "version")?,
    });
    copy_optional_manifest_alias_field(
        service_manifest,
        &mut service,
        "requiredEnv",
        "required_env",
    );
    copy_optional_manifest_alias_field(service_manifest, &mut service, "transports", "transports");
    copy_optional_manifest_field(service_manifest, &mut service, "deployment");
    Ok(service)
}

fn copy_optional_manifest_alias_field(
    source: &Value,
    target: &mut Value,
    target_field: &str,
    source_field: &str,
) {
    if let Some(value) = source
        .get(source_field)
        .or_else(|| source.get(target_field))
    {
        target[target_field] = value.clone();
    }
}

fn service_status_path(service_manifest: &Value) -> String {
    service_manifest
        .get("status_path")
        .or_else(|| service_manifest.get("statusPath"))
        .and_then(Value::as_str)
        .unwrap_or("/lenso/service/v1/status")
        .to_owned()
}

fn service_status_url(service_manifest: &Value, base_url: &str) -> String {
    if let Some(status_url) = service_manifest
        .get("status_url")
        .or_else(|| service_manifest.get("statusUrl"))
        .and_then(Value::as_str)
        .map(trim_trailing_slashes)
    {
        return status_url;
    }
    let path = service_status_path(service_manifest);
    reqwest::Url::parse(&format!("{}/", trim_trailing_slashes(base_url)))
        .ok()
        .and_then(|base| base.join(&path).ok())
        .map(|url| trim_trailing_slashes(url.as_str()))
        .unwrap_or_else(|| join_url_path(base_url, &path))
}

fn service_module_base_url(base_url: &str, module_name: &str) -> String {
    join_url_path(base_url, &format!("modules/{module_name}"))
}

fn join_url_path(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        trim_trailing_slashes(base_url),
        path.trim_start_matches('/')
    )
}

fn service_manifest_install_services(
    manifest: &Value,
    service_name: &str,
    base_url: &str,
) -> Result<Vec<RemoteModuleServiceInstallSpec>> {
    let mut services = remote_module_install_services(manifest, service_name, base_url)?;
    let default_manifest_ready_url = join_url_path(base_url, "manifest");
    let default_status_ready_url = service_status_url(manifest, base_url);
    for service in &mut services {
        if service.ready_url == default_manifest_ready_url {
            service.ready_url = default_status_ready_url.clone();
        }
    }
    Ok(services)
}

fn service_receipt_base_url(receipt: &Value) -> Option<String> {
    receipt
        .get("service")
        .and_then(|service| service.get("baseUrl").or_else(|| service.get("base_url")))
        .and_then(Value::as_str)
        .map(trim_trailing_slashes)
}

fn service_receipt_name(receipt: &Value) -> Option<&str> {
    receipt
        .get("service")
        .and_then(|service| service.get("name").or_else(|| service.get("serviceName")))
        .and_then(Value::as_str)
}

fn remote_uninstall_target(
    install_ledger_path: &Path,
    requested_name: &str,
) -> Result<RemoteUninstallTarget> {
    let Some(ledger) = read_json_if_exists(install_ledger_path)? else {
        return Ok(RemoteUninstallTarget {
            provider_name: requested_name.to_owned(),
            module_names: vec![requested_name.to_owned()],
        });
    };
    let modules = ledger
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Module install ledger modules must be an array"))?;

    if let Some(receipt) = modules
        .iter()
        .find(|module| module.get("moduleName").and_then(Value::as_str) == Some(requested_name))
        && let Some(provider_name) = service_receipt_name(receipt)
    {
        return Ok(RemoteUninstallTarget {
            provider_name: provider_name.to_owned(),
            module_names: service_receipt_module_names(modules, provider_name),
        });
    }

    let module_names = service_receipt_module_names(modules, requested_name);
    if module_names.is_empty() {
        Ok(RemoteUninstallTarget {
            provider_name: requested_name.to_owned(),
            module_names: vec![requested_name.to_owned()],
        })
    } else {
        Ok(RemoteUninstallTarget {
            provider_name: requested_name.to_owned(),
            module_names,
        })
    }
}

fn remote_uninstall_dependency_warnings(
    install_ledger_path: &Path,
    target: &RemoteUninstallTarget,
) -> Result<Vec<String>> {
    let Some(ledger) = read_json_if_exists(install_ledger_path)? else {
        return Ok(Vec::new());
    };
    let removed = target
        .module_names
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    Ok(ledger
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let module_name = entry.get("moduleName").and_then(Value::as_str)?;
            if removed.contains(module_name) {
                return None;
            }
            let dependency = entry
                .get("dependencies")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .find(|dependency| removed.contains(dependency))?;
            Some(format!(
                "`{module_name}` still declares dependency on removed module `{dependency}`"
            ))
        })
        .collect())
}

fn service_receipt_module_names(modules: &[Value], provider_name: &str) -> Vec<String> {
    modules
        .iter()
        .filter(|module| service_receipt_name(module) == Some(provider_name))
        .filter_map(|module| module.get("moduleName").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn remote_module_manifest_compatibility_issue(manifest: &Value) -> Option<String> {
    let compatibility = manifest.get("compatibility")?;
    let module_name = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("module");
    if let Some(lenso) = compatibility.get("lenso") {
        if let Some(min_version) = lenso
            .get("minVersion")
            .or_else(|| lenso.get("min_version"))
            .and_then(Value::as_str)
            && !matches!(
                compare_versions(env!("CARGO_PKG_VERSION"), min_version),
                Some(Ordering::Equal | Ordering::Greater)
            )
        {
            return Some(format!(
                "{module_name} requires Lenso >= {min_version}; CLI is {}",
                env!("CARGO_PKG_VERSION")
            ));
        }
        if let Some(max_version) = lenso
            .get("maxVersion")
            .or_else(|| lenso.get("max_version"))
            .and_then(Value::as_str)
            && !matches!(
                compare_versions(env!("CARGO_PKG_VERSION"), max_version),
                Some(Ordering::Equal | Ordering::Less)
            )
        {
            return Some(format!(
                "{module_name} supports Lenso <= {max_version}; CLI is {}",
                env!("CARGO_PKG_VERSION")
            ));
        }
    }
    if let Some(console_package_api) = compatibility
        .get("consolePackageApi")
        .or_else(|| compatibility.get("console_package_api"))
        .and_then(Value::as_str)
        && console_package_api != CONSOLE_BUNDLE_HOST_API
    {
        return Some(format!(
            "{module_name} requires console package API {console_package_api}; host supports {CONSOLE_BUNDLE_HOST_API}"
        ));
    }
    if let Some(protocol_version) = compatibility
        .get("remoteProtocolVersion")
        .or_else(|| compatibility.get("remote_protocol_version"))
        .and_then(Value::as_str)
        && protocol_version != REMOTE_PROTOCOL_VERSION
    {
        return Some(format!(
            "{module_name} requires remote protocol {protocol_version}; host supports {REMOTE_PROTOCOL_VERSION}"
        ));
    }
    let unsupported_feature = compatibility
        .get("requiredHostFeatures")
        .or_else(|| compatibility.get("required_host_features"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .find(|feature| !SUPPORTED_SERVICE_MODULE_FEATURES.contains(feature));
    unsupported_feature
        .map(|feature| format!("{module_name} requires unsupported host feature {feature}"))
}

fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    Some(parse_version(left)?.cmp(&parse_version(right)?))
}

fn parse_version(value: &str) -> Option<[u64; 3]> {
    let mut parts = value.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some([major, minor, patch])
}

async fn read_json_reference(reference: &str) -> Result<Value> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        let response = reqwest::get(reference)
            .await
            .with_context(|| format!("fetch module manifest {reference}"))?;
        if !response.status().is_success() {
            bail!(
                "Failed to fetch module manifest: {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("")
            );
        }
        return response
            .json::<Value>()
            .await
            .context("parse remote module manifest JSON");
    }
    let path = if let Some(file_path) = reference.strip_prefix("file://") {
        PathBuf::from(file_path)
    } else {
        PathBuf::from(reference)
    };
    read_json(&path)
}

fn derive_remote_base_url(base_url: Option<&str>, manifest_reference: &str) -> Result<String> {
    if let Some(base_url) = base_url {
        return Ok(trim_trailing_slashes(base_url));
    }
    if manifest_reference.starts_with("http://") || manifest_reference.starts_with("https://") {
        let mut url = reqwest::Url::parse(manifest_reference)
            .with_context(|| format!("parse manifest URL {manifest_reference}"))?;
        if url.path().ends_with("/manifest") {
            let next_path = url.path().trim_end_matches("/manifest").to_owned();
            url.set_path(&next_path);
            url.set_query(None);
            url.set_fragment(None);
            return Ok(trim_trailing_slashes(url.as_str()));
        }
    }
    bail!("Remote module base URL is required unless the manifest URL ends with /manifest");
}

fn update_remote_modules_env(
    env_file_path: &Path,
    module_name: &str,
    base_url: &str,
) -> Result<String> {
    let source = read_text_if_exists(env_file_path)?;
    let current_value = source
        .lines()
        .find_map(|line| line.strip_prefix("REMOTE_MODULES="))
        .unwrap_or_default();
    let mut entries = parse_remote_module_entries(current_value);
    entries.retain(|(name, _)| name != module_name);
    entries.push((module_name.to_owned(), base_url.to_owned()));
    Ok(upsert_env_value(
        &source,
        "REMOTE_MODULES",
        &format_remote_module_entries(&entries),
    ))
}

fn remove_remote_module_from_env(
    env_file_path: &Path,
    module_name: &str,
) -> Result<Option<String>> {
    if !env_file_path.exists() {
        return Ok(None);
    }
    Ok(remove_remote_module_from_env_source(
        &read_text(env_file_path)?,
        module_name,
    ))
}

fn remove_remote_module_from_env_source(source: &str, module_name: &str) -> Option<String> {
    let current_value = source
        .lines()
        .find_map(|line| line.strip_prefix("REMOTE_MODULES="))?;
    let mut entries = parse_remote_module_entries(current_value);
    let original_len = entries.len();
    entries.retain(|(name, _)| name != module_name);
    if entries.len() == original_len {
        return None;
    }
    let next_value = format_remote_module_entries(&entries);
    Some(if next_value.is_empty() {
        remove_env_value(source, "REMOTE_MODULES")
    } else {
        upsert_env_value(source, "REMOTE_MODULES", &next_value)
    })
}

fn remote_module_install_state_exists(
    module_name: &str,
    env_file_path: &Path,
    install_plan_path: &Path,
    console_extension_registry_path: &Path,
    module_services_path: &Path,
) -> Result<bool> {
    let env_source = read_text_if_exists(env_file_path)?;
    if remote_module_entries_from_env_source(&env_source)
        .iter()
        .any(|(name, _)| name == module_name)
    {
        return Ok(true);
    }

    if read_json_if_exists(install_plan_path)?
        .as_ref()
        .is_some_and(|plan| install_plan_has_module(plan, module_name))
    {
        return Ok(true);
    }

    if read_json_if_exists(console_extension_registry_path)?
        .as_ref()
        .is_some_and(|registry| console_extension_registry_has_module(registry, module_name))
    {
        return Ok(true);
    }

    Ok(read_remote_module_service_states(module_services_path)?
        .iter()
        .any(|state| state.module_name == module_name))
}

fn install_plan_has_module(plan: &Value, module_name: &str) -> bool {
    plan.get("modules")
        .and_then(Value::as_array)
        .is_some_and(|modules| {
            modules
                .iter()
                .any(|module| module.get("moduleName").and_then(Value::as_str) == Some(module_name))
        })
}

fn console_extension_registry_has_module(registry: &Value, module_name: &str) -> bool {
    registry
        .get("bundles")
        .and_then(Value::as_array)
        .is_some_and(|bundles| {
            bundles
                .iter()
                .any(|bundle| bundle.get("moduleName").and_then(Value::as_str) == Some(module_name))
        })
}

fn update_module_install_ledger(ledger_path: &Path, entry: Value) -> Result<Value> {
    let ledger =
        read_json_if_exists(ledger_path)?.unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
    upsert_module_install_ledger_entry(ledger, entry)
}

fn upsert_module_install_ledger_entry(mut ledger: Value, entry: Value) -> Result<Value> {
    let module_name = entry
        .get("moduleName")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Module install ledger entry moduleName is required"))?
        .to_owned();
    let modules = ledger
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Module install ledger modules must be an array"))?;
    modules.retain(|module| {
        module.get("moduleName").and_then(Value::as_str) != Some(module_name.as_str())
    });
    modules.push(entry);
    Ok(json!({ "modules": modules.clone(), "version": 1 }))
}

fn remove_module_install_ledger_modules(
    ledger_path: &Path,
    module_names: &[String],
) -> Result<Option<Value>> {
    read_json_if_exists(ledger_path)?.map_or(Ok(None), |mut ledger| {
        let modules = ledger
            .get_mut("modules")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Module install ledger modules must be an array"))?;
        let original_len = modules.len();
        modules.retain(|module| {
            let Some(module_name) = module.get("moduleName").and_then(Value::as_str) else {
                return true;
            };
            !module_names.iter().any(|name| name == module_name)
        });
        if modules.len() == original_len {
            return Ok(None);
        }
        Ok(Some(json!({ "modules": modules.clone(), "version": 1 })))
    })
}

#[cfg(test)]
fn remove_module_install_ledger_module_value(
    mut ledger: Value,
    module_name: &str,
) -> Result<Option<Value>> {
    let modules = ledger
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Module install ledger modules must be an array"))?;
    let original_len = modules.len();
    modules.retain(|module| module.get("moduleName").and_then(Value::as_str) != Some(module_name));
    if modules.len() == original_len {
        return Ok(None);
    }
    Ok(Some(json!({ "modules": modules.clone(), "version": 1 })))
}

fn set_linked_module_enabled_ledger(
    ledger_path: &Path,
    module_name: &str,
    enabled: bool,
    env_path: &str,
) -> Result<Value> {
    let Some(mut ledger) = read_json_if_exists(ledger_path)? else {
        return update_module_install_ledger(
            ledger_path,
            simple_linked_module_install_ledger_entry(module_name, enabled, env_path),
        );
    };
    let modules = ledger
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Module install ledger modules must be an array"))?;
    if let Some(module) = modules
        .iter_mut()
        .find(|module| module.get("moduleName").and_then(Value::as_str) == Some(module_name))
    {
        module
            .as_object_mut()
            .ok_or_else(|| anyhow!("Module install ledger entries must be objects"))?
            .insert("enabled".to_owned(), json!(enabled));
        return Ok(json!({ "modules": modules.clone(), "version": 1 }));
    }
    modules.push(simple_linked_module_install_ledger_entry(
        module_name,
        enabled,
        env_path,
    ));
    Ok(json!({ "modules": modules.clone(), "version": 1 }))
}

fn module_install_ledger_entry(ledger_path: &Path, module_name: &str) -> Result<Option<Value>> {
    let Some(ledger) = read_json_if_exists(ledger_path)? else {
        return Ok(None);
    };
    Ok(module_install_ledger_entry_value(&ledger, module_name).cloned())
}

fn module_install_ledger_entry_value<'a>(
    ledger: &'a Value,
    module_name: &str,
) -> Option<&'a Value> {
    ledger
        .get("modules")
        .and_then(Value::as_array)
        .and_then(|modules| {
            modules.iter().find(|module| {
                module.get("moduleName").and_then(Value::as_str) == Some(module_name)
            })
        })
}

fn module_install_ledger_source(
    ledger_path: &Path,
    module_name: &str,
) -> Result<Option<ModuleSource>> {
    let entry = module_install_ledger_entry(ledger_path, module_name)?;
    let source = entry
        .as_ref()
        .and_then(|module| module.get("source"))
        .and_then(Value::as_str);
    source.map(parse_module_source).transpose()
}

fn module_update_reference(manifest_reference: &str) -> &str {
    manifest_reference
        .strip_prefix("builtin:")
        .or_else(|| manifest_reference.strip_prefix("linked:"))
        .unwrap_or(manifest_reference)
}

fn linked_module_uninstall_call(ledger_path: &Path, module_name: &str) -> Result<Option<String>> {
    if let Some(call) = read_json_if_exists(ledger_path)?
        .as_ref()
        .and_then(|ledger| {
            ledger
                .get("modules")
                .and_then(Value::as_array)
                .and_then(|modules| {
                    modules.iter().find(|module| {
                        module.get("moduleName").and_then(Value::as_str) == Some(module_name)
                    })
                })
                .and_then(|module| module.get("linked"))
                .and_then(|linked| linked.get("call"))
                .and_then(Value::as_str)
        })
    {
        return Ok(Some(call.to_owned()));
    }

    linked_module_uninstall_call_from_builtin(module_name)
}

fn linked_module_uninstall_call_from_builtin(module_name: &str) -> Result<Option<String>> {
    Ok(builtin_linked_module_descriptor(module_name)
        .map(|descriptor| string_field(&descriptor["linked"], "call").map(ToOwned::to_owned))
        .transpose()?)
}

fn remove_linked_module_from_host_lib_source(source: &str, call: &str) -> Option<String> {
    let entry = format!(".linked_module({call})");
    let lines = source
        .lines()
        .filter(|line| !line.trim().starts_with(&entry))
        .collect::<Vec<_>>();
    (lines.len() != source.lines().count()).then(|| format!("{}\n", lines.join("\n")))
}

fn remove_linked_modules_from_host_lib_source(source: &str, calls: &[String]) -> Option<String> {
    let mut current = source.to_owned();
    let mut changed = false;
    for call in calls {
        if let Some(next) = remove_linked_module_from_host_lib_source(&current, call) {
            current = next;
            changed = true;
        }
    }
    changed.then_some(current)
}

fn linked_modules_to_uninstall(
    module_name: &str,
    ledger: Option<&Value>,
    env_source: &str,
    host_lib_source: &str,
) -> Result<Vec<String>> {
    let mut modules = Vec::new();
    collect_linked_dependents_to_uninstall(
        module_name,
        ledger,
        env_source,
        host_lib_source,
        &mut modules,
    )?;
    if !modules.iter().any(|candidate| candidate == module_name) {
        modules.push(module_name.to_owned());
    }
    Ok(modules)
}

fn collect_linked_dependents_to_uninstall(
    module_name: &str,
    ledger: Option<&Value>,
    env_source: &str,
    host_lib_source: &str,
    modules: &mut Vec<String>,
) -> Result<()> {
    for dependent in builtin_linked_module_dependents(module_name)? {
        if !linked_module_is_installed(&dependent, ledger, env_source, host_lib_source)? {
            continue;
        }
        collect_linked_dependents_to_uninstall(
            &dependent,
            ledger,
            env_source,
            host_lib_source,
            modules,
        )?;
        if !modules.iter().any(|module| module == &dependent) {
            modules.push(dependent);
        }
    }
    Ok(())
}

fn builtin_linked_module_dependents(module_name: &str) -> Result<Vec<String>> {
    builtin_linked_module_names()
        .iter()
        .filter_map(|candidate| {
            let descriptor = builtin_linked_module_descriptor(candidate)?;
            let dependencies = descriptor.get("dependencies").and_then(Value::as_array)?;
            dependencies
                .iter()
                .filter_map(Value::as_str)
                .any(|dependency| dependency == module_name)
                .then(|| Ok((*candidate).to_owned()))
        })
        .collect()
}

fn linked_module_is_installed(
    module_name: &str,
    ledger: Option<&Value>,
    env_source: &str,
    host_lib_source: &str,
) -> Result<bool> {
    if ledger.is_some_and(|ledger| {
        ledger
            .get("modules")
            .and_then(Value::as_array)
            .is_some_and(|modules| {
                modules.iter().any(|module| {
                    module.get("moduleName").and_then(Value::as_str) == Some(module_name)
                        && module.get("source").and_then(Value::as_str) == Some("linked")
                })
            })
    }) {
        return Ok(true);
    }
    if linked_module_enabled_env_exists(env_source, module_name) {
        return Ok(true);
    }
    Ok(linked_module_uninstall_call_from_builtin(module_name)?
        .as_deref()
        .is_some_and(|call| host_lib_source.contains(&format!(".linked_module({call})"))))
}

fn remote_module_install_ledger_entry(
    module_name: &str,
    manifest_reference: &str,
    base_url: &str,
    manifest: &Value,
    writes: Vec<Value>,
    install_env: &[(String, String)],
    install_commands: &[InstallCommandSpec],
    install_services: &[RemoteModuleServiceInstallSpec],
    console_package_count: usize,
) -> Value {
    let mut entry = json!({
        "baseUrl": base_url,
        "enabled": true,
        "install": {
            "commands": install_command_receipts(install_commands),
            "consolePackages": console_package_count,
            "env": install_env_receipts(install_env),
            "services": install_service_receipts(install_services),
        },
        "manifestReference": manifest_reference,
        "moduleName": module_name,
        "source": "remote",
        "writes": writes,
    });
    copy_optional_manifest_field(manifest, &mut entry, "compatibility");
    copy_optional_manifest_field(manifest, &mut entry, "dependencies");
    copy_optional_manifest_field(manifest, &mut entry, "deployment");
    copy_optional_manifest_field(manifest, &mut entry, "service");
    entry
}

fn linked_module_install_ledger_entry(
    module_name: &str,
    descriptor_reference: &str,
    call: &str,
    dependencies: &[String],
    writes: Vec<Value>,
    cargo_toml_changed: bool,
) -> Value {
    let manifest_reference = if builtin_linked_module_descriptor(descriptor_reference).is_some() {
        format!("builtin:{descriptor_reference}")
    } else {
        descriptor_reference.to_owned()
    };
    json!({
        "dependencies": dependencies,
        "enabled": true,
        "linked": {
            "call": call,
            "cargoTomlChanged": cargo_toml_changed,
        },
        "manifestReference": manifest_reference,
        "moduleName": module_name,
        "source": "linked",
        "writes": writes,
    })
}

fn copy_optional_manifest_field(manifest: &Value, entry: &mut Value, field: &str) {
    if let Some(value) = manifest.get(field) {
        entry[field] = value.clone();
    }
}

fn simple_linked_module_install_ledger_entry(
    module_name: &str,
    enabled: bool,
    env_path: &str,
) -> Value {
    json!({
        "enabled": enabled,
        "manifestReference": format!("linked:{module_name}"),
        "moduleName": module_name,
        "source": "linked",
        "writes": [
            { "kind": "env", "key": linked_module_enabled_env_key(module_name), "path": env_path }
        ],
    })
}

fn remote_module_install_writes(
    repo_root: &Path,
    env_file_path: &Path,
    console_extension_registry_path: Option<&Path>,
    module_services_path: Option<&Path>,
) -> Vec<Value> {
    let mut writes = vec![json!({
        "kind": "env",
        "key": "REMOTE_MODULES",
        "path": display_relative(repo_root, env_file_path),
    })];
    if let Some(console_extension_registry_path) = console_extension_registry_path {
        writes.push(json!({
            "kind": "consoleExtensionRegistry",
            "path": display_relative(repo_root, console_extension_registry_path),
        }));
    }
    if let Some(module_services_path) = module_services_path {
        writes.push(json!({
            "kind": "moduleServices",
            "path": display_relative(repo_root, module_services_path),
        }));
    }
    writes
}

fn linked_module_install_writes(
    repo_root: &Path,
    env_file_path: &Path,
    cargo_toml_path: Option<&Path>,
    host_lib_path: &Path,
    runtime_config_defaults_path: Option<&Path>,
    console_extension_registry_path: Option<&Path>,
) -> Vec<Value> {
    let mut writes = vec![json!({
        "kind": "env",
        "path": display_relative(repo_root, env_file_path),
    })];
    if let Some(cargo_toml_path) = cargo_toml_path {
        writes.push(json!({
            "kind": "cargoToml",
            "path": display_relative(repo_root, cargo_toml_path),
        }));
    }
    writes.push(json!({
        "kind": "hostComposition",
        "path": display_relative(repo_root, host_lib_path),
    }));
    if let Some(runtime_config_defaults_path) = runtime_config_defaults_path {
        writes.push(json!({
            "kind": "runtimeConfigDefaults",
            "path": display_relative(repo_root, runtime_config_defaults_path),
        }));
    }
    if let Some(console_extension_registry_path) = console_extension_registry_path {
        writes.push(json!({
            "kind": "consoleExtensionRegistry",
            "path": display_relative(repo_root, console_extension_registry_path),
        }));
    }
    writes
}

fn install_env_receipts(install_env: &[(String, String)]) -> Vec<Value> {
    install_env
        .iter()
        .map(|(key, _)| json!({ "key": key }))
        .collect()
}

fn install_command_receipts(install_commands: &[InstallCommandSpec]) -> Vec<Value> {
    install_commands
        .iter()
        .map(|command| {
            json!({
                "command": &command.command,
                "cwd": command.cwd.as_deref().unwrap_or("."),
            })
        })
        .collect()
}

fn install_service_receipts(install_services: &[RemoteModuleServiceInstallSpec]) -> Vec<Value> {
    install_services
        .iter()
        .map(|service| {
            json!({
                "autoStart": service.auto_start,
                "command": &service.command,
                "cwd": service.cwd.as_deref().unwrap_or("."),
                "name": &service.name,
                "readyTimeoutMs": service.ready_timeout_ms,
                "readyUrl": &service.ready_url,
            })
        })
        .collect()
}

fn set_linked_module_enabled(
    module_name: &str,
    enabled: bool,
    env_file: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    dry_run: bool,
) -> Result<()> {
    let module_name = slugify(module_name);
    if module_name.is_empty() {
        bail!("Module name is required");
    }
    let repo_root = resolve_repo_root(repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        env_file.as_deref().unwrap_or_else(|| Path::new(".env")),
    );
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let key = linked_module_enabled_env_key(&module_name);
    let value = if enabled { "true" } else { "false" };
    let env_file =
        set_linked_module_enabled_env(&read_text_if_exists(&env_file_path)?, &module_name, enabled);
    let install_ledger = set_linked_module_enabled_ledger(
        &install_ledger_path,
        &module_name,
        enabled,
        &display_relative(&repo_root, &env_file_path),
    )?;

    if dry_run {
        let action = if enabled { "install" } else { "uninstall" };
        println!("Linked module {action} dry run:");
        println!("- {}", display_relative(&repo_root, &env_file_path));
        println!("- {}", display_relative(&repo_root, &install_ledger_path));
        println!("- {key}={value}");
        return Ok(());
    }

    write_file(&env_file_path, env_file.as_bytes())?;
    write_json(&install_ledger_path, &install_ledger)?;
    if enabled {
        println!("Enabled linked module {module_name}.");
    } else {
        println!("Disabled linked module {module_name}.");
    }
    println!("Next steps:");
    println!("- restart the API and worker");

    Ok(())
}

fn uninstall_linked_module(module_name: &str, options: RemoteModuleUninstallOptions) -> Result<()> {
    let module_name = slugify(module_name);
    if module_name.is_empty() {
        bail!("Module name is required");
    }
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let env_file_path = resolve_path(
        &repo_root,
        options
            .env_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".env")),
    );
    let host_lib_path = repo_root.join("src/lib.rs");
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let ledger = read_json_if_exists(&install_ledger_path)?;
    let env_source = read_text_if_exists(&env_file_path)?;
    let host_lib_source = read_text_if_exists(&host_lib_path)?;
    let modules =
        linked_modules_to_uninstall(&module_name, ledger.as_ref(), &env_source, &host_lib_source)?;
    let mut calls = Vec::new();
    for module_name in &modules {
        if let Some(call) = linked_module_uninstall_call(&install_ledger_path, module_name)? {
            calls.push(call);
        }
    }
    let env_file = modules
        .iter()
        .fold(env_source.clone(), |source, module_name| {
            remove_env_value(&source, &linked_module_enabled_env_key(module_name))
        });
    let env_file = (env_file != env_source).then_some(env_file);
    let host_lib = remove_linked_modules_from_host_lib_source(&host_lib_source, &calls);
    let install_ledger = remove_module_install_ledger_modules(&install_ledger_path, &modules)?;
    let console_extension_registry_path = repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH);
    let console_registry =
        remove_runtime_console_bundle_registry_modules(&console_extension_registry_path, &modules)?;
    let console_extension_module_dirs = modules
        .iter()
        .map(|module_name| {
            repo_root
                .join(".lenso/console/extensions")
                .join(slugify(module_name))
        })
        .filter(|path| path.exists())
        .collect::<Vec<_>>();

    if options.dry_run {
        println!("Linked module uninstall dry run:");
        if env_file.is_some() {
            println!("- {}", display_relative(&repo_root, &env_file_path));
        }
        if host_lib.is_some() {
            println!("- {}", display_relative(&repo_root, &host_lib_path));
        }
        if install_ledger.is_some() {
            println!("- {}", display_relative(&repo_root, &install_ledger_path));
        }
        if console_registry.is_some() {
            println!(
                "- {}",
                display_relative(&repo_root, &console_extension_registry_path)
            );
        }
        for path in &console_extension_module_dirs {
            println!("- {}", display_relative(&repo_root, path));
        }
        for call in calls {
            println!("- remove {call}");
        }
        return Ok(());
    }

    if let Some(env_file) = env_file {
        write_file(&env_file_path, env_file.as_bytes())?;
    }
    if let Some(host_lib) = host_lib {
        write_file(&host_lib_path, host_lib.as_bytes())?;
    }
    if let Some(install_ledger) = install_ledger {
        write_json(&install_ledger_path, &install_ledger)?;
    }
    if let Some(console_registry) = console_registry {
        write_json(&console_extension_registry_path, &console_registry)?;
    }
    for path in console_extension_module_dirs {
        fs::remove_dir_all(&path)
            .with_context(|| format!("remove console extension directory {}", path.display()))?;
    }

    println!("Uninstalled linked module(s): {}.", modules.join(", "));
    println!("Next steps:");
    println!("- restart the API and worker");

    Ok(())
}

fn set_linked_module_enabled_env(source: &str, module_name: &str, enabled: bool) -> String {
    upsert_env_value(
        source,
        &linked_module_enabled_env_key(module_name),
        if enabled { "true" } else { "false" },
    )
}

fn linked_module_enabled_env_key(module_name: &str) -> String {
    format!(
        "LENSO_MODULE_{}_ENABLED",
        module_name.replace('-', "_").to_ascii_uppercase()
    )
}

fn module_enabled_from_env_source(source: &str, module_name: &str) -> bool {
    let key = linked_module_enabled_env_key(module_name);
    source
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key}=")))
        .and_then(parse_env_bool)
        .unwrap_or(true)
}

fn infer_uninstall_module_source(
    module_name: &str,
    env_source: &str,
    remote_installed: bool,
) -> Result<ModuleSource> {
    if remote_installed {
        return Ok(ModuleSource::Remote);
    }

    if builtin_linked_module_descriptor(module_name).is_some()
        || linked_module_enabled_env_exists(env_source, module_name)
    {
        return Ok(ModuleSource::Linked);
    }

    Ok(ModuleSource::Remote)
}

fn linked_module_enabled_env_exists(source: &str, module_name: &str) -> bool {
    let key = linked_module_enabled_env_key(module_name);
    source
        .lines()
        .any(|line| line.trim_start().starts_with(&format!("{key}=")))
}

fn parse_env_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn run_install_commands(repo_root: &Path, commands: &[InstallCommandSpec]) -> Result<()> {
    for command in commands {
        let cwd = command
            .cwd
            .as_deref()
            .map(|cwd| resolve_path(repo_root, Path::new(cwd)))
            .unwrap_or_else(|| repo_root.to_path_buf());
        println!("Running install command: {}", command.command);
        let status = shell_command(&command.command)
            .current_dir(&cwd)
            .status()
            .with_context(|| format!("run install command `{}`", command.command))?;
        if !status.success() {
            bail!("Install command failed: {}", command.command);
        }
    }
    Ok(())
}

fn shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut process = Command::new("cmd");
        process.arg("/C").arg(command);
        process
    } else {
        let mut process = Command::new("sh");
        process.arg("-c").arg(command);
        process
    }
}

#[derive(Debug, Clone)]
struct ConsoleBundleInstall {
    bundle_count: usize,
    bundle_files: Vec<PathBuf>,
    registry_changed: bool,
}

#[derive(Debug, Clone)]
struct ConsoleBundleSpec {
    bundle_url: String,
    entry: String,
    export_name: String,
    host_api: String,
    module_name: String,
    package_name: String,
    required_capabilities: Vec<String>,
    styles: Vec<ConsoleBundleStyleSpec>,
    target_path: PathBuf,
    version: Option<String>,
}

#[derive(Debug, Clone)]
struct ConsoleBundleStyleSpec {
    entry: String,
    source_url: String,
    target_path: PathBuf,
}

async fn install_runtime_console_bundles(
    repo_root: &Path,
    registry_path: &Path,
    manifest: &Value,
    base_url: &str,
    enabled: bool,
    dry_run: bool,
) -> Result<ConsoleBundleInstall> {
    if !enabled {
        return Ok(ConsoleBundleInstall {
            bundle_count: 0,
            bundle_files: Vec::new(),
            registry_changed: false,
        });
    }

    let specs = remote_module_console_bundle_specs(repo_root, manifest, base_url)?;
    install_runtime_console_bundle_specs(registry_path, specs, dry_run).await
}

async fn install_runtime_console_bundles_for_manifests(
    repo_root: &Path,
    registry_path: &Path,
    manifests: &[&Value],
    enabled: bool,
    dry_run: bool,
) -> Result<ConsoleBundleInstall> {
    if !enabled {
        return Ok(ConsoleBundleInstall {
            bundle_count: 0,
            bundle_files: Vec::new(),
            registry_changed: false,
        });
    }

    let mut specs = Vec::new();
    for manifest in manifests {
        specs.extend(remote_module_console_bundle_specs(repo_root, manifest, "")?);
    }
    install_runtime_console_bundle_specs(registry_path, specs, dry_run).await
}

async fn install_runtime_console_bundle_specs(
    registry_path: &Path,
    specs: Vec<ConsoleBundleSpec>,
    dry_run: bool,
) -> Result<ConsoleBundleInstall> {
    if specs.is_empty() {
        return Ok(ConsoleBundleInstall {
            bundle_count: 0,
            bundle_files: Vec::new(),
            registry_changed: false,
        });
    }

    if !dry_run {
        for spec in &specs {
            let bytes = read_bundle_reference(&spec.bundle_url).await?;
            write_file(&spec.target_path, &bytes)?;
            for style in &spec.styles {
                let bytes = read_bundle_reference(&style.source_url).await?;
                write_file(&style.target_path, &bytes)?;
            }
        }
        let registry = update_runtime_console_bundle_registry(registry_path, &specs)?;
        write_json(registry_path, &registry)?;
    }

    Ok(ConsoleBundleInstall {
        bundle_count: specs.len(),
        bundle_files: specs
            .iter()
            .flat_map(|spec| {
                std::iter::once(spec.target_path.clone())
                    .chain(spec.styles.iter().map(|style| style.target_path.clone()))
            })
            .collect(),
        registry_changed: true,
    })
}

fn remote_module_console_bundle_specs(
    repo_root: &Path,
    manifest: &Value,
    base_url: &str,
) -> Result<Vec<ConsoleBundleSpec>> {
    let module_name = string_field(manifest, "name")?.trim();
    let module_slug = slugify(module_name);
    let mut specs = Vec::new();
    let Some(surfaces) = manifest.get("console").and_then(Value::as_array) else {
        return Ok(specs);
    };
    for surface in surfaces {
        let package = surface.get("package").and_then(Value::as_object);
        let Some(package_name) = package.and_then(|p| p.get("name")).and_then(Value::as_str) else {
            continue;
        };
        let Some(export_name) = package
            .and_then(|p| p.get("export"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(bundle_reference) = console_bundle_url(surface, package) else {
            continue;
        };
        let bundle_url = resolve_bundle_reference(bundle_reference, base_url)?;
        let file_name = console_bundle_file_name(&bundle_url, export_name);
        let target_path = repo_root
            .join(".lenso/console/extensions")
            .join(&module_slug)
            .join(&file_name);
        let entry = format!("{CONSOLE_EXTENSION_ROUTE_PREFIX}/{module_slug}/{file_name}");
        let styles = console_bundle_styles(surface, package)
            .into_iter()
            .map(|style_reference| {
                let source_url = resolve_bundle_reference(style_reference, base_url)?;
                let file_name = console_style_file_name(&source_url, export_name);
                Ok(ConsoleBundleStyleSpec {
                    entry: format!("{CONSOLE_EXTENSION_ROUTE_PREFIX}/{module_slug}/{file_name}"),
                    source_url,
                    target_path: repo_root
                        .join(".lenso/console/extensions")
                        .join(&module_slug)
                        .join(file_name),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        specs.push(ConsoleBundleSpec {
            bundle_url,
            entry,
            export_name: export_name.to_owned(),
            host_api: console_bundle_host_api(surface, package).to_owned(),
            module_name: module_name.to_owned(),
            package_name: package_name.to_owned(),
            required_capabilities: surface
                .get("required_capabilities")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect(),
            styles,
            target_path,
            version: package
                .and_then(|p| p.get("version"))
                .or_else(|| surface.get("version"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        });
    }
    Ok(specs)
}

fn console_bundle_url<'a>(
    surface: &'a Value,
    package: Option<&'a Map<String, Value>>,
) -> Option<&'a str> {
    package
        .and_then(|p| p.get("bundleUrl").or_else(|| p.get("bundle_url")))
        .and_then(Value::as_str)
        .or_else(|| {
            package
                .and_then(|p| p.get("bundle"))
                .and_then(|bundle| bundle.get("url"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            surface
                .get("bundleUrl")
                .or_else(|| surface.get("bundle_url"))
                .and_then(Value::as_str)
        })
}

fn console_bundle_styles<'a>(
    surface: &'a Value,
    package: Option<&'a Map<String, Value>>,
) -> Vec<&'a str> {
    let mut styles = Vec::new();
    collect_console_bundle_styles(package.and_then(|p| p.get("styles")), &mut styles);
    collect_console_bundle_styles(
        package
            .and_then(|p| p.get("bundle"))
            .and_then(|bundle| bundle.get("styles")),
        &mut styles,
    );
    collect_console_bundle_styles(surface.get("styles"), &mut styles);
    collect_console_bundle_styles(
        surface
            .get("bundle")
            .and_then(|bundle| bundle.get("styles")),
        &mut styles,
    );
    styles
}

fn collect_console_bundle_styles<'a>(value: Option<&'a Value>, styles: &mut Vec<&'a str>) {
    match value {
        Some(Value::String(style)) => styles.push(style),
        Some(Value::Array(items)) => {
            styles.extend(items.iter().filter_map(Value::as_str));
        }
        _ => {}
    }
}

fn console_bundle_host_api<'a>(
    surface: &'a Value,
    package: Option<&'a Map<String, Value>>,
) -> &'a str {
    package
        .and_then(|p| p.get("hostApi").or_else(|| p.get("host_api")))
        .and_then(Value::as_str)
        .or_else(|| {
            surface
                .get("hostApi")
                .or_else(|| surface.get("host_api"))
                .and_then(Value::as_str)
        })
        .unwrap_or(CONSOLE_BUNDLE_HOST_API)
}

fn resolve_bundle_reference(reference: &str, base_url: &str) -> Result<String> {
    if reference.starts_with("http://")
        || reference.starts_with("https://")
        || reference.starts_with("file://")
    {
        return Ok(reference.to_owned());
    }
    let normalized_base = format!("{}/", trim_trailing_slashes(base_url));
    let base = reqwest::Url::parse(&normalized_base)
        .with_context(|| format!("parse base URL {base_url}"))?;
    let resolved = base
        .join(reference)
        .with_context(|| format!("resolve console bundle URL {reference}"))?;
    Ok(resolved.to_string())
}

fn console_bundle_file_name(bundle_url: &str, export_name: &str) -> String {
    console_asset_file_name(bundle_url, export_name, "js")
}

fn console_style_file_name(style_url: &str, export_name: &str) -> String {
    console_asset_file_name(style_url, export_name, "css")
}

fn console_asset_file_name(asset_url: &str, export_name: &str, extension: &str) -> String {
    reqwest::Url::parse(asset_url)
        .ok()
        .and_then(|url| {
            url.path_segments()
                .and_then(Iterator::last)
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            Path::new(asset_url)
                .file_name()
                .and_then(|name| name.to_str())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| format!("{}.{}", slugify(export_name), extension))
}

async fn read_bundle_reference(reference: &str) -> Result<Vec<u8>> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        let response = reqwest::get(reference)
            .await
            .with_context(|| format!("fetch console bundle {reference}"))?;
        if !response.status().is_success() {
            bail!(
                "Failed to fetch console bundle: {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("")
            );
        }
        return response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .context("read console bundle bytes");
    }
    let path = if let Some(file_path) = reference.strip_prefix("file://") {
        PathBuf::from(file_path)
    } else {
        PathBuf::from(reference)
    };
    fs::read(&path).with_context(|| format!("read console bundle {}", path.display()))
}

fn update_runtime_console_bundle_registry(
    registry_path: &Path,
    specs: &[ConsoleBundleSpec],
) -> Result<Value> {
    let mut registry = read_json_if_exists(registry_path)?
        .unwrap_or_else(|| json!({ "bundles": [], "version": 1 }));
    let bundles = registry
        .get_mut("bundles")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Runtime Console extension registry bundles must be an array"))?;
    for spec in specs {
        bundles.retain(|entry| {
            entry.get("packageName").and_then(Value::as_str) != Some(spec.package_name.as_str())
                || entry.get("exportName").and_then(Value::as_str)
                    != Some(spec.export_name.as_str())
        });
        let mut entry = json!({
            "entry": spec.entry,
            "exportName": spec.export_name,
            "hostApi": spec.host_api,
            "moduleName": spec.module_name,
            "packageName": spec.package_name,
        });
        if !spec.required_capabilities.is_empty() {
            entry["requiredCapabilities"] = json!(spec.required_capabilities);
        }
        if !spec.styles.is_empty() {
            entry["styles"] = json!(
                spec.styles
                    .iter()
                    .map(|style| style.entry.as_str())
                    .collect::<Vec<_>>()
            );
        }
        if let Some(version) = &spec.version {
            entry["version"] = json!(version);
        }
        bundles.push(entry);
    }
    Ok(registry)
}

fn update_remote_module_services_file(
    services_file_path: &Path,
    module_name: &str,
    install_services: &[RemoteModuleServiceInstallSpec],
) -> Result<Option<Value>> {
    let existed = services_file_path.exists();
    let mut state = read_json_if_exists(services_file_path)?
        .unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
    let modules = state
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Remote module services file modules must be an array"))?;
    let original_len = modules.len();
    modules.retain(|entry| entry.get("moduleName").and_then(Value::as_str) != Some(module_name));
    if !install_services.is_empty() {
        modules.push(json!({
            "moduleName": module_name,
            "services": remote_module_service_plans(install_services),
        }));
    }
    if !existed && modules.is_empty() {
        return Ok(None);
    }
    if existed || original_len != modules.len() || !install_services.is_empty() {
        return Ok(Some(json!({ "modules": modules.clone(), "version": 1 })));
    }
    Ok(None)
}

fn remove_remote_module_services_file_module(
    services_file_path: &Path,
    module_name: &str,
) -> Result<Option<Value>> {
    read_json_if_exists(services_file_path)?.map_or(Ok(None), |mut state| {
        let modules = state
            .get_mut("modules")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Remote module services file modules must be an array"))?;
        let original_len = modules.len();
        modules
            .retain(|entry| entry.get("moduleName").and_then(Value::as_str) != Some(module_name));
        if modules.len() == original_len {
            return Ok(None);
        }
        Ok(Some(json!({ "modules": modules.clone(), "version": 1 })))
    })
}

fn apply_manifest_install_env(source: String, install_env: &[(String, String)]) -> String {
    install_env.iter().fold(source, |source, (key, value)| {
        upsert_env_value(&source, key, value)
    })
}

fn update_runtime_config_defaults(
    source: Option<Value>,
    defaults: &[RuntimeConfigDefault],
) -> Result<Value> {
    let mut state = source.unwrap_or_else(|| json!({ "version": 1, "values": [] }));
    let object = state
        .as_object_mut()
        .ok_or_else(|| anyhow!("Runtime config defaults file must be a JSON object"))?;
    object.entry("version").or_insert_with(|| json!(1));
    let values = object
        .entry("values")
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| anyhow!("Runtime config defaults file values must be an array"))?;

    for default in defaults {
        upsert_runtime_config_default(values, default);
    }
    Ok(state)
}

fn upsert_runtime_config_default(values: &mut Vec<Value>, default: &RuntimeConfigDefault) {
    let next = json!({
        "key": &default.key,
        "service": &default.service,
        "value": &default.value,
    });
    if let Some(existing) = values.iter_mut().find(|entry| {
        entry.get("service").and_then(Value::as_str) == Some(default.service.as_str())
            && entry.get("key").and_then(Value::as_str) == Some(default.key.as_str())
    }) {
        *existing = next;
    } else {
        values.push(next);
    }
}

fn remote_module_install_env(manifest: &Value) -> Result<Vec<(String, String)>> {
    let Some(env) = manifest
        .get("install")
        .and_then(|install| install.get("env"))
    else {
        return Ok(Vec::new());
    };
    let object = env
        .as_object()
        .ok_or_else(|| anyhow!("Remote module manifest install.env must be an object"))?;
    let mut values = Vec::new();
    for (key, value) in object {
        let key = key.trim();
        if key.is_empty() {
            bail!("Remote module manifest install.env keys must be non-empty");
        }
        if key == "REMOTE_MODULES" {
            bail!("Remote module manifest install.env must not override REMOTE_MODULES");
        }
        let value = value
            .as_str()
            .ok_or_else(|| anyhow!("Remote module manifest install.env.{key} must be a string"))?;
        values.push((key.to_owned(), value.to_owned()));
    }
    Ok(values)
}

fn remote_module_install_commands(manifest: &Value) -> Result<Vec<InstallCommandSpec>> {
    let Some(commands) = manifest
        .get("install")
        .and_then(|install| install.get("commands"))
    else {
        return Ok(Vec::new());
    };
    let commands = commands
        .as_array()
        .ok_or_else(|| anyhow!("Remote module manifest install.commands must be an array"))?;
    commands
        .iter()
        .map(|entry| match entry {
            Value::String(command) => install_command_spec(command, None),
            Value::Object(object) => {
                let command = object
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        anyhow!("Remote module manifest install.commands[].command is required")
                    })?;
                let cwd = object
                    .get("cwd")
                    .map(|value| {
                        value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                            anyhow!(
                                "Remote module manifest install.commands[].cwd must be a string"
                            )
                        })
                    })
                    .transpose()?;
                install_command_spec(command, cwd)
            }
            _ => {
                bail!("Remote module manifest install.commands entries must be strings or objects")
            }
        })
        .collect()
}

fn remote_module_install_services(
    manifest: &Value,
    module_name: &str,
    base_url: &str,
) -> Result<Vec<RemoteModuleServiceInstallSpec>> {
    let Some(services) = manifest
        .get("install")
        .and_then(|install| install.get("services"))
    else {
        return Ok(Vec::new());
    };
    let services = services
        .as_array()
        .ok_or_else(|| anyhow!("Remote module manifest install.services must be an array"))?;
    services
        .iter()
        .map(|entry| {
            let object = entry.as_object().ok_or_else(|| {
                anyhow!("Remote module manifest install.services entries must be objects")
            })?;
            let command = object
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("Remote module manifest install.services[].command is required")
                })?
                .trim();
            if command.is_empty() {
                bail!("Remote module manifest install service command must be non-empty");
            }
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(module_name)
                .trim();
            let ready_url = object
                .get("readyUrl")
                .or_else(|| object.get("ready_url"))
                .and_then(Value::as_str)
                .map(trim_trailing_slashes)
                .unwrap_or_else(|| format!("{}/manifest", trim_trailing_slashes(base_url)));
            Ok(RemoteModuleServiceInstallSpec {
                name: if name.is_empty() {
                    module_name.to_owned()
                } else {
                    name.to_owned()
                },
                command: command.to_owned(),
                cwd: object
                    .get("cwd")
                    .map(|value| {
                        value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                            anyhow!(
                                "Remote module manifest install.services[].cwd must be a string"
                            )
                        })
                    })
                    .transpose()?
                    .map(|value| value.trim().to_owned())
                    .filter(|value| !value.is_empty()),
                ready_url,
                ready_timeout_ms: object
                    .get("readyTimeoutMs")
                    .or_else(|| object.get("ready_timeout_ms"))
                    .and_then(Value::as_u64)
                    .unwrap_or(10_000),
                auto_start: object
                    .get("autoStart")
                    .or_else(|| object.get("auto_start"))
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            })
        })
        .collect()
}

fn install_command_spec(command: &str, cwd: Option<String>) -> Result<InstallCommandSpec> {
    let command = command.trim();
    if command.is_empty() {
        bail!("Remote module manifest install command must be non-empty");
    }
    Ok(InstallCommandSpec {
        command: command.to_owned(),
        cwd: cwd
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
    })
}

#[cfg(test)]
fn install_service_plans(install_services: &[RemoteModuleServiceInstallSpec]) -> Vec<Value> {
    install_services
        .iter()
        .map(|service| {
            json!({
                "autoStart": service.auto_start,
                "command": &service.command,
                "cwd": service.cwd.as_deref().unwrap_or("."),
                "name": &service.name,
                "readyTimeoutMs": service.ready_timeout_ms,
                "readyUrl": &service.ready_url,
                "status": if service.auto_start { "registered" } else { "manual" },
            })
        })
        .collect()
}

fn remote_module_service_plans(install_services: &[RemoteModuleServiceInstallSpec]) -> Vec<Value> {
    install_services
        .iter()
        .map(|service| {
            json!({
                "autoStart": service.auto_start,
                "command": &service.command,
                "cwd": service.cwd.as_deref().unwrap_or("."),
                "name": &service.name,
                "readyTimeoutMs": service.ready_timeout_ms,
                "readyUrl": &service.ready_url,
            })
        })
        .collect()
}

fn read_remote_module_service_states(
    services_file_path: &Path,
) -> Result<Vec<RemoteModuleServiceState>> {
    let Some(value) = read_json_if_exists(services_file_path)? else {
        return Ok(Vec::new());
    };
    parse_remote_module_service_states(&value)
}

fn parse_remote_module_service_states(value: &Value) -> Result<Vec<RemoteModuleServiceState>> {
    let modules = value
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Remote module services file modules must be an array"))?;
    let mut states = Vec::new();
    for module in modules {
        let module_name = module
            .get("moduleName")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Remote module services file moduleName must be a string"))?
            .trim();
        if module_name.is_empty() {
            bail!("Remote module services file moduleName must be non-empty");
        }
        let services = module
            .get("services")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow!("{module_name} services must be an array"))?;
        let mut service_specs = Vec::new();
        for service in services {
            let command = service
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{module_name} service command must be a string"))?
                .trim();
            if command.is_empty() {
                bail!("{module_name} service command must be non-empty");
            }
            let ready_url = service
                .get("readyUrl")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("{module_name} service readyUrl must be a string"))?
                .trim();
            if ready_url.is_empty() {
                bail!("{module_name} service readyUrl must be non-empty");
            }
            service_specs.push(RemoteModuleServiceInstallSpec {
                name: service
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or(module_name)
                    .to_owned(),
                command: command.to_owned(),
                cwd: service
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|cwd| !cwd.is_empty())
                    .map(ToOwned::to_owned),
                ready_url: ready_url.to_owned(),
                ready_timeout_ms: service
                    .get("readyTimeoutMs")
                    .and_then(Value::as_u64)
                    .unwrap_or(10_000),
                auto_start: service
                    .get("autoStart")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            });
        }
        states.push(RemoteModuleServiceState {
            module_name: module_name.to_owned(),
            services: service_specs,
        });
    }
    Ok(states)
}

async fn remote_service_ready_url(client: &reqwest::Client, ready_url: &str) -> bool {
    client
        .get(ready_url)
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

async fn wait_for_started_module_service_ready(
    client: &reqwest::Client,
    child: &mut Child,
    module_name: &str,
    service: &RemoteModuleServiceInstallSpec,
    lock_file_path: &Path,
    pid_file_path: &Path,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(service.ready_timeout_ms);
    loop {
        if remote_service_ready_url(client, &service.ready_url).await {
            println!("{}/{} ready", module_name, service.name);
            return Ok(());
        }
        if let Some(status) = child
            .try_wait()
            .with_context(|| format!("check service {}/{}", module_name, service.name))?
        {
            let _ = fs::remove_file(pid_file_path);
            let _ = fs::remove_file(lock_file_path);
            bail!(
                "service {}/{} exited before ready: {status}",
                module_name,
                service.name
            );
        }
        if Instant::now() >= deadline {
            bail!(
                "service {}/{} did not become ready at {} within {}ms",
                module_name,
                service.name,
                service.ready_url,
                service.ready_timeout_ms
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

fn remote_module_service_doctor_status(
    configured: bool,
    enabled: bool,
    auto_start: bool,
    ready: bool,
    lock_exists: bool,
    pid_exists: bool,
) -> RemoteModuleServiceDoctorStatus {
    if !configured {
        return RemoteModuleServiceDoctorStatus::NotConfigured;
    }
    if !enabled {
        return RemoteModuleServiceDoctorStatus::Disabled;
    }
    if ready {
        return RemoteModuleServiceDoctorStatus::Ready;
    }
    if !auto_start {
        return RemoteModuleServiceDoctorStatus::ManualNotReady;
    }
    if lock_exists || pid_exists {
        return RemoteModuleServiceDoctorStatus::StaleState;
    }
    RemoteModuleServiceDoctorStatus::NotReady
}

fn remote_module_service_doctor_fix(
    status: RemoteModuleServiceDoctorStatus,
) -> Option<&'static str> {
    match status {
        RemoteModuleServiceDoctorStatus::Ready => None,
        RemoteModuleServiceDoctorStatus::Disabled => {
            Some("enable the module if this service should run")
        }
        RemoteModuleServiceDoctorStatus::ManualNotReady => {
            Some("start this service manually or set autoStart=true in the manifest")
        }
        RemoteModuleServiceDoctorStatus::NotConfigured => {
            Some("install the module or remove its service entry")
        }
        RemoteModuleServiceDoctorStatus::NotReady => {
            Some("start the service command or restart the API/worker")
        }
        RemoteModuleServiceDoctorStatus::StaleState => {
            Some("restart the API/worker; remove stale lock/pid files if it remains stuck")
        }
    }
}

fn module_service_log_path(repo_root: &Path, module_name: &str, service_name: &str) -> PathBuf {
    repo_root
        .join(".lenso/service-logs")
        .join(remote_module_service_state_segment(module_name))
        .join(format!(
            "{}.log",
            remote_module_service_state_segment(service_name)
        ))
}

fn tail_lines(contents: &str, tail: usize) -> Vec<&str> {
    let lines = contents.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(tail)..].to_vec()
}

fn remote_module_service_state_path(
    services_state_dir: &Path,
    module_name: &str,
    service: &RemoteModuleServiceInstallSpec,
    extension: &str,
) -> PathBuf {
    services_state_dir.join(format!(
        "remote-{}-{}.{}",
        remote_module_service_state_segment(module_name),
        remote_module_service_state_segment(&service.name),
        extension
    ))
}

fn remote_module_service_state_segment(value: &str) -> String {
    let mut segment = String::new();
    let mut previous_dash = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            segment.push(character.to_ascii_lowercase());
            previous_dash = false;
        } else if !segment.is_empty() && !previous_dash {
            segment.push('-');
            previous_dash = true;
        }
    }
    while segment.ends_with('-') {
        segment.pop();
    }
    if segment.is_empty() {
        "service".to_owned()
    } else {
        segment
    }
}

#[cfg(test)]
fn install_command_plans(
    install_commands: &[InstallCommandSpec],
    install_commands_executed: bool,
) -> Vec<Value> {
    let status = if install_commands_executed {
        "executed"
    } else {
        "requires_manual_run"
    };
    install_commands
        .iter()
        .map(|command| {
            json!({
                "command": &command.command,
                "cwd": command.cwd.as_deref().unwrap_or("."),
                "status": status,
            })
        })
        .collect()
}

fn remove_console_package_install_plan_module(
    install_plan_path: &Path,
    module_name: &str,
) -> Result<Option<Value>> {
    remove_console_package_install_plan_modules(install_plan_path, &[module_name.to_owned()])
}

fn remove_console_package_install_plan_modules(
    install_plan_path: &Path,
    module_names: &[String],
) -> Result<Option<Value>> {
    read_json_if_exists(install_plan_path)?.map_or(Ok(None), |mut plan| {
        let version = plan.get("version").cloned().unwrap_or_else(|| json!(1));
        let modules = plan
            .get_mut("modules")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Console package install plan modules must be an array"))?;
        let original_len = modules.len();
        modules.retain(|entry| {
            let Some(module_name) = entry.get("moduleName").and_then(Value::as_str) else {
                return true;
            };
            !module_names.iter().any(|name| name == module_name)
        });
        if modules.len() == original_len {
            return Ok(None);
        }
        Ok(Some(
            json!({ "modules": modules.clone(), "version": version }),
        ))
    })
}

#[cfg(test)]
fn remove_console_package_install_plan_module_value(
    mut plan: Value,
    module_name: &str,
) -> Result<Option<Value>> {
    let version = plan.get("version").cloned().unwrap_or_else(|| json!(1));
    let modules = plan
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Console package install plan modules must be an array"))?;
    let original_len = modules.len();
    modules.retain(|entry| entry.get("moduleName").and_then(Value::as_str) != Some(module_name));
    if modules.len() == original_len {
        return Ok(None);
    }
    Ok(Some(
        json!({ "modules": modules.clone(), "version": version }),
    ))
}

fn remove_runtime_console_bundle_registry_module(
    registry_path: &Path,
    module_name: &str,
) -> Result<Option<Value>> {
    remove_runtime_console_bundle_registry_modules(registry_path, &[module_name.to_owned()])
}

fn remove_runtime_console_bundle_registry_modules(
    registry_path: &Path,
    module_names: &[String],
) -> Result<Option<Value>> {
    read_json_if_exists(registry_path)?.map_or(Ok(None), |mut registry| {
        let version = registry.get("version").cloned().unwrap_or_else(|| json!(1));
        let bundles = registry
            .get_mut("bundles")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| {
                anyhow!("Runtime Console extension registry bundles must be an array")
            })?;
        let original_len = bundles.len();
        bundles.retain(|entry| {
            let Some(module_name) = entry.get("moduleName").and_then(Value::as_str) else {
                return true;
            };
            !module_names.iter().any(|name| name == module_name)
        });
        if bundles.len() == original_len {
            return Ok(None);
        }
        Ok(Some(
            json!({ "bundles": bundles.clone(), "version": version }),
        ))
    })
}

fn remove_stale_module_console_artifacts(
    repo_root: &Path,
    module_name: &str,
    include_install_plan: bool,
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    if include_install_plan {
        let install_plan_path = repo_root.join(".lenso/console-package-install-plan.json");
        if let Some(install_plan) =
            remove_console_package_install_plan_module(&install_plan_path, module_name)?
        {
            changed.push(install_plan_path.clone());
            if !dry_run {
                write_json(&install_plan_path, &install_plan)?;
            }
        }
    }

    let registry_path = repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH);
    if let Some(registry) =
        remove_runtime_console_bundle_registry_module(&registry_path, module_name)?
    {
        changed.push(registry_path.clone());
        if !dry_run {
            write_json(&registry_path, &registry)?;
        }
    }

    let module_slug = slugify(module_name);
    if module_slug.is_empty() {
        return Ok(changed);
    }
    let module_dir = repo_root
        .join(".lenso/console/extensions")
        .join(module_slug);
    if module_dir.exists() {
        changed.push(module_dir.clone());
        if !dry_run {
            fs::remove_dir_all(&module_dir).with_context(|| {
                format!(
                    "remove console extension directory {}",
                    module_dir.display()
                )
            })?;
        }
    }

    Ok(changed)
}

fn module_catalog_entry_from_manifest(
    manifest: &Value,
    manifest_reference: &str,
    base_url: &str,
    summary: Option<&str>,
) -> Result<Value> {
    let empty = Vec::new();
    let console_surfaces = manifest
        .get("console")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    let console_packages = console_surfaces
        .iter()
        .filter_map(|surface| {
            let package = surface.get("package").and_then(Value::as_object)?;
            let mut package_hint = json!({
                "exportName": package.get("export")?.as_str()?,
                "packageName": package.get("name")?.as_str()?,
                "route": surface.get("route").and_then(Value::as_str).unwrap_or("-"),
            });
            if let Some(bundle_url) = console_bundle_url(surface, Some(package)) {
                package_hint["bundleUrl"] = json!(bundle_url);
            }
            let styles = console_bundle_styles(surface, Some(package));
            if !styles.is_empty() {
                package_hint["styles"] = json!(styles);
            }
            if let Some(host_api) = package
                .get("hostApi")
                .or_else(|| package.get("host_api"))
                .or_else(|| surface.get("hostApi"))
                .or_else(|| surface.get("host_api"))
                .and_then(Value::as_str)
            {
                package_hint["hostApi"] = json!(host_api);
            }
            if let Some(version) = package
                .get("version")
                .or_else(|| surface.get("version"))
                .and_then(Value::as_str)
            {
                package_hint["version"] = json!(version);
            }
            Some(package_hint)
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "baseUrl": base_url,
        "consolePackages": console_packages,
        "manifestReference": manifest_reference,
        "name": string_field(manifest, "name")?.trim(),
        "source": "remote",
        "summary": summary.or_else(|| manifest.get("summary").and_then(Value::as_str)).unwrap_or("-"),
        "version": string_field(manifest, "version")?.trim(),
    }))
}

fn module_release_catalog_entry_from_manifest(
    manifest: &Value,
    manifest_reference: &str,
    base_url: Option<&str>,
    summary: Option<&str>,
) -> Result<Value> {
    let source = module_release_source(manifest)?;
    let mut entry = json!({
        "manifestReference": manifest_reference,
        "name": string_field(manifest, "name")?.trim(),
        "protocol": "lenso.module-release.v1",
        "source": source,
        "summary": summary.or_else(|| manifest.get("summary").and_then(Value::as_str)).unwrap_or("-"),
        "version": string_field(manifest, "version")?.trim(),
    });
    if source == "service" {
        let provider = module_release_provider(manifest)?;
        let provider_name = provider
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("Module release provider.name is required"))?;
        entry["providedBy"] = json!(provider_name);
        entry["provider"] = Value::Object(provider.clone());
        if let Some(service_manifest) = provider
            .get("serviceManifest")
            .or_else(|| provider.get("service_manifest"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            entry["serviceManifest"] = json!(service_manifest);
        }
    }
    if let Some(base_url) = base_url.map(str::trim).filter(|value| !value.is_empty()) {
        entry["baseUrl"] = json!(base_url);
    }
    copy_optional_manifest_field(manifest, &mut entry, "capabilities");
    copy_optional_manifest_field(manifest, &mut entry, "dependencies");
    copy_optional_manifest_field(manifest, &mut entry, "compatibility");
    copy_optional_manifest_field(manifest, &mut entry, "linked");
    Ok(entry)
}

fn service_catalog_entry_from_manifest(
    manifest: &Value,
    manifest_reference: &str,
    base_url: &str,
    summary: Option<&str>,
) -> Result<Value> {
    let provided_modules = manifest
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Service manifest modules must be an array"))?
        .iter()
        .map(|module| {
            json!({
                "capabilities": module.get("capabilities").cloned().unwrap_or_else(|| json!([])),
                "name": string_field(module, "name").unwrap_or("-").trim(),
                "version": module
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| string_field(manifest, "version").unwrap_or("0.1.0")),
            })
        })
        .collect::<Vec<_>>();
    let mut entry = json!({
        "baseUrl": base_url,
        "manifestReference": manifest_reference,
        "modules": provided_modules,
        "name": string_field(manifest, "name")?.trim(),
        "service": {
            "requiredEnv": manifest.get("required_env").or_else(|| manifest.get("requiredEnv")).cloned().unwrap_or_else(|| json!([])),
            "statusPath": service_status_path(manifest),
            "statusUrl": service_status_url(manifest, base_url),
            "transports": manifest.get("transports").cloned().unwrap_or_else(|| json!(["http"])),
        },
        "source": "service",
        "summary": summary.or_else(|| manifest.get("summary").and_then(Value::as_str)).unwrap_or("-"),
        "version": string_field(manifest, "version")?.trim(),
    });
    copy_optional_manifest_field(manifest, &mut entry, "compatibility");
    copy_optional_manifest_field(manifest, &mut entry, "deployment");
    copy_optional_manifest_field(manifest, &mut entry, "install");
    Ok(entry)
}

fn unique_console_package_plan_items(install_plan: &Value) -> Vec<ConsolePackagePlanItem> {
    let mut items_by_key = BTreeMap::new();
    for module_plan in install_plan
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for console_package in module_plan
            .get("consolePackages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(package_name) = console_package.get("packageName").and_then(Value::as_str)
            else {
                continue;
            };
            let Some(export_name) = console_package.get("exportName").and_then(Value::as_str)
            else {
                continue;
            };
            items_by_key.insert(
                console_package_key(package_name, export_name),
                ConsolePackagePlanItem {
                    export_name: export_name.to_owned(),
                    package_name: package_name.to_owned(),
                },
            );
        }
    }
    items_by_key.into_values().collect()
}

fn update_package_json_dependency(
    package_json: &mut Value,
    package_name: &str,
    dependency_version: &str,
) -> Result<()> {
    let object = package_json
        .as_object_mut()
        .ok_or_else(|| anyhow!("Runtime Console package.json must be a JSON object"))?;
    let dependencies = object
        .entry("dependencies")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| anyhow!("Runtime Console package.json dependencies must be an object"))?;
    dependencies
        .entry(package_name.to_owned())
        .or_insert_with(|| Value::String(dependency_version.to_owned()));
    Ok(())
}

fn manifest_name_from_module_export(module_name: &str) -> String {
    module_name.strip_suffix("Module").map_or_else(
        || format!("{module_name}Manifest"),
        |stem| format!("{stem}Manifest"),
    )
}

fn console_package_key(package_name: &str, export_name: &str) -> String {
    format!("{package_name}#{export_name}")
}

async fn read_install_descriptor(reference: &str) -> Result<Option<Value>> {
    if let Some(descriptor) = builtin_linked_module_descriptor(reference) {
        return Ok(Some(descriptor));
    }

    if !looks_like_json_reference(reference) {
        return Ok(None);
    }

    let descriptor = read_json_reference(reference).await?;
    Ok(
        (descriptor.get("source").is_some() && !is_service_manifest(&descriptor))
            .then_some(descriptor),
    )
}

fn builtin_linked_module_descriptor(reference: &str) -> Option<Value> {
    match reference.trim() {
        "auth" => Some(json!({
            "name": "auth",
            "source": "linked",
            "console": [
                {
                    "package": {
                        "bundleUrl": "https://cdn.jsdelivr.net/npm/@lenso/auth-console@0.1.1/dist/auth-console.js",
                        "export": "authConsoleModule",
                        "hostApi": "1",
                        "name": "@lenso/auth-console",
                        "styles": ["https://cdn.jsdelivr.net/npm/@lenso/auth-console@0.1.1/dist/auth-console.css"],
                        "version": "0.1.1"
                    },
                    "required_capabilities": ["auth.users.read"]
                }
            ],
            "linked": {
                "call": "builtins::auth()"
            },
            "install": {
                "profiles": {
                    "redis-session-cache": {
                        "linked": {
                            "cargo": {
                                "package": "lenso-module-auth",
                                "version": "0.1.6",
                                "features": ["redis"]
                            }
                        },
                        "env": {
                            "REDIS_URL": "redis://localhost:6379/0"
                        },
                        "runtimeConfigDefaults": {
                            "auth.session_cache": "redis"
                        }
                    }
                }
            }
        })),
        "auth-password" => Some(json!({
            "name": "auth-password",
            "source": "linked",
            "dependencies": ["auth"],
            "linked": {
                "call": "builtins::auth_password()"
            }
        })),
        "auth-device" => Some(json!({
            "name": "auth-device",
            "source": "linked",
            "dependencies": ["auth"],
            "linked": {
                "call": "auth_device::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-auth-device",
                    "version": "0.1.1"
                }
            }
        })),
        _ => None,
    }
}

fn builtin_linked_module_names() -> &'static [&'static str] {
    &["auth", "auth-password", "auth-device"]
}

fn apply_linked_install_profiles(
    descriptor: &Value,
    profiles: &[String],
) -> Result<(Value, LinkedInstallProfileEffects)> {
    if profiles.is_empty() {
        return Ok((descriptor.clone(), LinkedInstallProfileEffects::default()));
    }

    let mut descriptor = descriptor.clone();
    let mut effects = LinkedInstallProfileEffects::default();
    for profile in profiles {
        let profile = profile.trim();
        if profile.is_empty() {
            bail!("Linked module install profile names must be non-empty");
        }
        let profile_descriptor = descriptor
            .get("install")
            .and_then(|install| install.get("profiles"))
            .and_then(|profiles| profiles.get(profile))
            .cloned()
            .ok_or_else(|| {
                anyhow!("Linked module descriptor install profile `{profile}` is not declared")
            })?;
        let profile_object = profile_descriptor.as_object().ok_or_else(|| {
            anyhow!("Linked module descriptor install profile `{profile}` must be an object")
        })?;

        if let Some(linked) = profile_object.get("linked") {
            merge_linked_install_profile(&mut descriptor, profile, linked)?;
        }
        if let Some(env) = profile_object.get("env") {
            effects.env.extend(install_profile_env(profile, env)?);
        }
        if let Some(runtime_config_defaults) = profile_object
            .get("runtimeConfigDefaults")
            .or_else(|| profile_object.get("runtime_config_defaults"))
        {
            effects
                .runtime_config_defaults
                .extend(install_profile_runtime_config_defaults(
                    profile,
                    runtime_config_defaults,
                )?);
        }
    }

    Ok((descriptor, effects))
}

fn merge_linked_install_profile(
    descriptor: &mut Value,
    profile: &str,
    linked: &Value,
) -> Result<()> {
    let linked_object = linked.as_object().ok_or_else(|| {
        anyhow!("Linked module descriptor install profile `{profile}` linked must be an object")
    })?;
    let target_linked = descriptor
        .get_mut("linked")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("Linked module descriptor linked section is required"))?;

    for (key, value) in linked_object {
        if key == "cargo" {
            merge_linked_cargo_profile(target_linked, profile, value)?;
        } else {
            target_linked.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

fn merge_linked_cargo_profile(
    target_linked: &mut Map<String, Value>,
    profile: &str,
    cargo: &Value,
) -> Result<()> {
    let cargo_object = cargo.as_object().ok_or_else(|| {
        anyhow!(
            "Linked module descriptor install profile `{profile}` linked.cargo must be an object"
        )
    })?;
    let target_cargo = target_linked
        .entry("cargo".to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let target_cargo = target_cargo.as_object_mut().ok_or_else(|| {
        anyhow!("Linked module descriptor install profile `{profile}` cannot merge linked.cargo into non-object")
    })?;

    for (key, value) in cargo_object {
        if key == "features" {
            merge_json_string_array(target_cargo, key, value, "linked.cargo.features")?;
        } else {
            target_cargo.insert(key.clone(), value.clone());
        }
    }
    Ok(())
}

fn merge_json_string_array(
    target: &mut Map<String, Value>,
    key: &str,
    value: &Value,
    context: &str,
) -> Result<()> {
    let values = json_string_array(value, context)?;
    let target_value = target
        .entry(key.to_owned())
        .or_insert_with(|| Value::Array(Vec::new()));
    let target_array = target_value
        .as_array_mut()
        .ok_or_else(|| anyhow!("Linked module descriptor {context} must be an array"))?;

    for value in values {
        if !target_array
            .iter()
            .any(|item| item.as_str() == Some(&value))
        {
            target_array.push(Value::String(value));
        }
    }
    Ok(())
}

fn install_profile_env(profile: &str, env: &Value) -> Result<Vec<(String, String)>> {
    let object = env.as_object().ok_or_else(|| {
        anyhow!("Linked module descriptor install profile `{profile}` env must be an object")
    })?;
    let mut values = Vec::new();
    for (key, value) in object {
        let key = key.trim();
        if key.is_empty() {
            bail!(
                "Linked module descriptor install profile `{profile}` env keys must be non-empty"
            );
        }
        if key == "REMOTE_MODULES" {
            bail!(
                "Linked module descriptor install profile `{profile}` env must not override REMOTE_MODULES"
            );
        }
        let value = value.as_str().ok_or_else(|| {
            anyhow!(
                "Linked module descriptor install profile `{profile}` env.{key} must be a string"
            )
        })?;
        values.push((key.to_owned(), value.to_owned()));
    }
    Ok(values)
}

fn install_profile_runtime_config_defaults(
    profile: &str,
    runtime_config_defaults: &Value,
) -> Result<Vec<RuntimeConfigDefault>> {
    if let Some(object) = runtime_config_defaults.as_object() {
        let mut values = Vec::new();
        for (key, value) in object {
            let key = key.trim();
            if key.is_empty() {
                bail!(
                    "Linked module descriptor install profile `{profile}` runtimeConfigDefaults keys must be non-empty"
                );
            }
            values.push(RuntimeConfigDefault {
                service: "*".to_owned(),
                key: key.to_owned(),
                value: value.clone(),
            });
        }
        return Ok(values);
    }

    let array = runtime_config_defaults.as_array().ok_or_else(|| {
        anyhow!(
            "Linked module descriptor install profile `{profile}` runtimeConfigDefaults must be an object or array"
        )
    })?;
    array
        .iter()
        .map(|entry| {
            let object = entry.as_object().ok_or_else(|| {
                anyhow!(
                    "Linked module descriptor install profile `{profile}` runtimeConfigDefaults entries must be objects"
                )
            })?;
            let key = object
                .get("key")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!(
                        "Linked module descriptor install profile `{profile}` runtimeConfigDefaults[].key is required"
                    )
                })?
                .trim();
            if key.is_empty() {
                bail!(
                    "Linked module descriptor install profile `{profile}` runtimeConfigDefaults[].key must be non-empty"
                );
            }
            Ok(RuntimeConfigDefault {
                service: object
                    .get("service")
                    .and_then(Value::as_str)
                    .unwrap_or("*")
                    .trim()
                    .to_owned(),
                key: key.to_owned(),
                value: object
                    .get("value")
                    .cloned()
                    .ok_or_else(|| anyhow!("Linked module descriptor install profile `{profile}` runtimeConfigDefaults[].value is required"))?,
            })
        })
        .collect()
}

fn looks_like_json_reference(reference: &str) -> bool {
    reference.starts_with("http://")
        || reference.starts_with("https://")
        || reference.starts_with("file://")
        || reference.ends_with(".json")
        || Path::new(reference).exists()
}

fn update_host_cargo_toml_for_linked_descriptor(
    source: &str,
    cargo: Option<&Value>,
) -> Result<Option<String>> {
    let Some(cargo) = cargo.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let package = string_field(cargo, "package")?.trim();
    if package.is_empty() {
        bail!("Linked module descriptor linked.cargo.package is required");
    }
    let features = linked_cargo_features(cargo)?;
    if let Some(updated) =
        update_existing_host_cargo_dependency_features(source, package, &features)?
    {
        return Ok(Some(updated));
    }
    let dependency = linked_cargo_dependency(package, cargo)?;
    Ok(Some(insert_after_needle(
        source,
        &format!("{dependency}\n"),
        "[dependencies]\n",
    )?))
}

fn linked_cargo_dependency(package: &str, cargo: &Value) -> Result<String> {
    let features = linked_cargo_features(cargo)?;
    if let Some(path) = cargo.get("path").and_then(Value::as_str) {
        let mut fields = vec![format!("path = {}", rust_string_literal(path))];
        if !features.is_empty() {
            fields.push(format!(
                "features = {}",
                rust_string_array_literal(&features)
            ));
        }
        return Ok(format!("{package} = {{ {} }}", fields.join(", ")));
    }
    if let Some(git) = cargo.get("git").and_then(Value::as_str) {
        let mut fields = vec![format!("git = {}", rust_string_literal(git))];
        for key in ["rev", "tag", "branch"] {
            if let Some(value) = cargo.get(key).and_then(Value::as_str) {
                fields.push(format!("{key} = {}", rust_string_literal(value)));
            }
        }
        if !features.is_empty() {
            fields.push(format!(
                "features = {}",
                rust_string_array_literal(&features)
            ));
        }
        return Ok(format!("{package} = {{ {} }}", fields.join(", ")));
    }
    let version = cargo.get("version").and_then(Value::as_str).unwrap_or("*");
    if features.is_empty() {
        Ok(format!("{package} = {}", rust_string_literal(version)))
    } else {
        Ok(format!(
            "{package} = {{ version = {}, features = {} }}",
            rust_string_literal(version),
            rust_string_array_literal(&features)
        ))
    }
}

fn linked_cargo_features(cargo: &Value) -> Result<Vec<String>> {
    cargo.get("features").map_or_else(
        || Ok(Vec::new()),
        |features| json_string_array(features, "linked.cargo.features"),
    )
}

fn update_existing_host_cargo_dependency_features(
    source: &str,
    package: &str,
    features: &[String],
) -> Result<Option<String>> {
    let Some(index) = source
        .lines()
        .position(|line| dependency_line_matches_package(line, package))
    else {
        return Ok(None);
    };
    if features.is_empty() {
        return Ok(None);
    }

    let mut lines = source
        .split('\n')
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let Some(updated_line) = merge_dependency_line_features(&lines[index], package, features)?
    else {
        return Ok(None);
    };
    lines[index] = updated_line;
    Ok(Some(lines.join("\n")))
}

fn dependency_line_matches_package(line: &str, package: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return false;
    }
    let direct_prefix = trimmed.strip_prefix(package).is_some_and(|rest| {
        let rest = rest.trim_start();
        rest.starts_with('=')
    });
    direct_prefix || trimmed.contains(&format!("package = {}", rust_string_literal(package)))
}

fn merge_dependency_line_features(
    line: &str,
    package: &str,
    features: &[String],
) -> Result<Option<String>> {
    let merged_features = merge_inline_feature_values(line, features)?;
    if merged_features.len() == inline_feature_values(line)?.len() && line.contains("features") {
        return Ok(None);
    }
    let feature_literal = rust_string_array_literal(&merged_features);

    if let Some((start, end)) = inline_feature_array_range(line)? {
        let mut updated = String::new();
        updated.push_str(&line[..start]);
        updated.push_str(&feature_literal);
        updated.push_str(&line[end..]);
        return Ok(Some(updated));
    }

    if let Some(close_brace) = line.rfind('}') {
        let before = line[..close_brace].trim_end();
        let separator = if before.ends_with('{') { " " } else { ", " };
        return Ok(Some(format!(
            "{before}{separator}features = {feature_literal} {}",
            &line[close_brace..]
        )));
    }

    let (left, right) = line
        .split_once('=')
        .ok_or_else(|| anyhow!("Cargo dependency line for `{package}` must contain `=`"))?;
    if left.trim() != package {
        bail!(
            "Cargo dependency alias for `{package}` must use inline table syntax to add features"
        );
    }
    let version = right.trim();
    Ok(Some(format!(
        "{}= {{ version = {version}, features = {feature_literal} }}",
        left
    )))
}

fn merge_inline_feature_values(line: &str, features: &[String]) -> Result<Vec<String>> {
    let mut values = inline_feature_values(line)?;
    for feature in features {
        if !values.contains(feature) {
            values.push(feature.clone());
        }
    }
    Ok(values)
}

fn inline_feature_values(line: &str) -> Result<Vec<String>> {
    let Some((start, end)) = inline_feature_array_range(line)? else {
        return Ok(Vec::new());
    };
    serde_json::from_str(&line[start..end]).with_context(|| "parse Cargo dependency features array")
}

fn inline_feature_array_range(line: &str) -> Result<Option<(usize, usize)>> {
    let Some(features_start) = find_inline_feature_key(line) else {
        return Ok(None);
    };
    let after_features = &line[features_start + "features".len()..];
    let equals_offset = after_features
        .find('=')
        .ok_or_else(|| anyhow!("Cargo dependency features field must contain `=`"))?;
    let after_equals_start = features_start + "features".len() + equals_offset + 1;
    let after_equals = &line[after_equals_start..];
    let array_start_offset = after_equals
        .find('[')
        .ok_or_else(|| anyhow!("Cargo dependency features field must be an array"))?;
    let array_start = after_equals_start + array_start_offset;
    let array_end_offset = line[array_start..]
        .find(']')
        .ok_or_else(|| anyhow!("Cargo dependency features array must be closed"))?;
    Ok(Some((array_start, array_start + array_end_offset + 1)))
}

fn find_inline_feature_key(line: &str) -> Option<usize> {
    let mut offset = 0;
    while let Some(relative_start) = line[offset..].find("features") {
        let start = offset + relative_start;
        let before = line[..start].chars().next_back();
        let after = line[start + "features".len()..].chars().next();
        let before_ok = before.is_none_or(|character| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        });
        let after_ok = after.is_none_or(|character| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        });
        if before_ok && after_ok {
            return Some(start);
        }
        offset = start + "features".len();
    }
    None
}

fn update_host_lib_for_linked_descriptor(
    source: &str,
    use_path: Option<&str>,
    call: &str,
) -> Result<String> {
    let source = maybe_insert_use(source, use_path)?;
    let entry = format!("        .linked_module({call})\n");
    if source.contains(entry.trim()) {
        return Ok(source);
    }
    if source.contains("        .linked_module(modules::app::linked_module())\n") {
        return insert_before_needle(
            &source,
            &entry,
            "        .linked_module(modules::app::linked_module())\n",
        );
    }
    insert_before_needle(&source, &entry, "        .build()")
}

fn maybe_insert_use(source: &str, use_path: Option<&str>) -> Result<String> {
    let Some(use_path) = use_path.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(source.to_owned());
    };
    let entry = format!("use {use_path};\n");
    if source.contains(entry.trim()) {
        return Ok(source.to_owned());
    }
    insert_after_needle(source, &entry, "use lenso::host::prelude::*;\n")
}

fn parse_module_source(source: &str) -> Result<ModuleSource> {
    match source.trim().to_ascii_lowercase().as_str() {
        "linked" => Ok(ModuleSource::Linked),
        "remote" => Ok(ModuleSource::Remote),
        other => bail!("Unsupported module source `{other}`; expected `remote` or `linked`"),
    }
}

fn parse_remote_module_entries(value: &str) -> Vec<(String, String)> {
    value
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }
            let (name, base_url) = entry.split_once('=')?;
            let name = name.trim();
            let base_url = base_url.trim();
            if name.is_empty() || base_url.is_empty() {
                None
            } else {
                Some((name.to_owned(), base_url.to_owned()))
            }
        })
        .collect()
}

fn remote_module_entries_from_env_source(source: &str) -> Vec<(String, String)> {
    let current_value = source
        .lines()
        .find_map(|line| line.strip_prefix("REMOTE_MODULES="))
        .unwrap_or_default();
    parse_remote_module_entries(current_value)
}

fn remote_module_manifest_url(base_url: &str) -> Option<String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
        return None;
    }
    Some(if base_url.ends_with("/manifest") {
        base_url.to_owned()
    } else {
        format!("{base_url}/manifest")
    })
}

fn format_remote_module_entries(entries: &[(String, String)]) -> String {
    entries
        .iter()
        .map(|(name, base_url)| format!("{name}={base_url}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn upsert_env_value(source: &str, key: &str, value: &str) -> String {
    let key_prefix = format!("{key}=");
    let mut lines = if source.is_empty() {
        Vec::new()
    } else {
        source
            .split('\n')
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>()
    };
    if let Some(index) = lines.iter().position(|line| line.starts_with(&key_prefix)) {
        lines[index] = format!("{key}={value}");
        format!("{}\n", lines.join("\n").trim_end_matches('\n'))
    } else {
        let trimmed = source.trim_end();
        if trimmed.is_empty() {
            format!("{key}={value}\n")
        } else {
            format!("{trimmed}\n{key}={value}\n")
        }
    }
}

fn remove_env_value(source: &str, key: &str) -> String {
    let key_prefix = format!("{key}=");
    let lines = source
        .lines()
        .filter(|line| !line.starts_with(&key_prefix))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn insert_before_needle(file_source: &str, entry: &str, needle: &str) -> Result<String> {
    if file_source.contains(entry.trim()) {
        return Ok(file_source.to_owned());
    }
    let index = file_source
        .find(needle)
        .ok_or_else(|| anyhow!("Could not find insertion point: {needle}"))?;
    Ok(format!(
        "{}{}{}",
        &file_source[..index],
        entry,
        &file_source[index..]
    ))
}

fn insert_after_needle(file_source: &str, entry: &str, needle: &str) -> Result<String> {
    if file_source.contains(entry.trim()) {
        return Ok(file_source.to_owned());
    }
    let index = file_source
        .find(needle)
        .ok_or_else(|| anyhow!("Could not find insertion point: {needle}"))?
        + needle.len();
    Ok(format!(
        "{}{}{}",
        &file_source[..index],
        entry,
        &file_source[index..]
    ))
}

fn runtime_console_paths(runtime_console_root: &Path) -> RuntimeConsolePaths {
    RuntimeConsolePaths {
        manifest_exports_path: runtime_console_root.join("src/console-package-manifest-exports.ts"),
        module_exports_path: runtime_console_root.join("src/console-package-module-exports.ts"),
        oxlint_config_path: runtime_console_root.join("oxlint.config.ts"),
        package_json_path: runtime_console_root.join("package.json"),
        tsconfig_path: runtime_console_root.join("tsconfig.json"),
    }
}

fn default_runtime_console_root_for_repo(repo_root: &Path) -> Result<PathBuf> {
    if repo_root
        .join("src/console-package-module-exports.ts")
        .exists()
    {
        return Ok(repo_root.to_path_buf());
    }
    let nested = repo_root.join("apps/runtime-console");
    if nested
        .join("src/console-package-module-exports.ts")
        .exists()
    {
        return Ok(nested);
    }
    let cwd = std::env::current_dir().context("resolve current directory")?;
    if cwd.join("src/console-package-module-exports.ts").exists() {
        return Ok(cwd);
    }
    Ok(nested)
}

fn resolve_repo_root(repo_root: Option<&Path>) -> Result<PathBuf> {
    if let Some(repo_root) = repo_root {
        return absolutize(repo_root);
    }
    find_repo_root(&std::env::current_dir().context("resolve current directory")?)
}

fn find_repo_root(start_path: &Path) -> Result<PathBuf> {
    let mut current = absolutize(start_path)?;
    loop {
        if is_framework_workspace_root(&current) || is_starter_host_root(&current) {
            return Ok(current);
        }
        let Some(parent) = current.parent() else {
            return absolutize(start_path);
        };
        if parent == current {
            return absolutize(start_path);
        }
        current = parent.to_path_buf();
    }
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("resolve current directory")?
            .join(path))
    }
}

fn resolve_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn display_relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn trim_trailing_slashes(value: &str) -> String {
    value.trim_end_matches('/').to_owned()
}

fn string_field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Remote module manifest {key} is required"))
}

fn read_json(path: &Path) -> Result<Value> {
    let source = read_text(path)?;
    serde_json::from_str(&source).with_context(|| format!("parse JSON {}", path.display()))
}

fn read_json_if_exists(path: &Path) -> Result<Option<Value>> {
    if path.exists() {
        Ok(Some(read_json(path)?))
    } else {
        Ok(None)
    }
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("read {}", path.display()))
}

fn read_text_if_exists(path: &Path) -> Result<String> {
    if path.exists() {
        read_text(path)
    } else {
        Ok(String::new())
    }
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut contents = serde_json::to_string_pretty(value)?;
    contents.push('\n');
    write_file(path, contents.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote_context() -> ConsolePackageContext {
        ConsolePackageContext {
            area: "Support".to_owned(),
            capability: "support.ticket.read".to_owned(),
            component_name: "SupportTicketModule".to_owned(),
            icon: "life-buoy".to_owned(),
            label: "Support Ticket".to_owned(),
            manifest_name: "supportTicketManifest".to_owned(),
            module_id: "support-ticket".to_owned(),
            module_name: "supportTicketModule".to_owned(),
            package_dir: PathBuf::from("console"),
            package_name: "@acme/lenso-support-ticket-console".to_owned(),
            package_private: true,
            package_slug: "support-ticket-console".to_owned(),
            registry_source: "support-ticket.console.json".to_owned(),
            route: "/support/tickets".to_owned(),
            runtime_console_api_version: "1".to_owned(),
            surface_name: "tickets".to_owned(),
        }
    }

    #[test]
    fn starter_host_module_scaffold_uses_internal_module_layout() {
        let root =
            std::env::temp_dir().join(format!("lenso-cli-starter-host-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("src/modules")).unwrap();
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"host\"\n").unwrap();
        fs::write(root.join("src/lib.rs"), "").unwrap();
        fs::write(root.join("src/modules/mod.rs"), "pub mod app;\n").unwrap();

        assert!(is_starter_host_root(&root));

        let source = host_module_manifest("support-ticket", None).unwrap();
        assert!(source.contains("pub const MODULE_NAME: &str = \"support-ticket\";"));
        assert!(
            source.contains("HostLinkedModule::manifest_only(MODULE_NAME, manifest, MIGRATIONS)")
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn remote_scaffold_manifest_declares_service_lifecycle() {
        let manifest = remote_manifest_json(&remote_context(), "lenso-support-ticket");

        assert_eq!(manifest["name"], json!("support-ticket-service"));
        assert_eq!(manifest["modules"][0]["name"], json!("support-ticket"));
        assert_eq!(manifest["protocol"], json!("lenso.service.v1"));
        assert_eq!(
            manifest["install"]["services"][0]["name"],
            json!("support-ticket-service")
        );
        assert_eq!(
            manifest["install"]["services"][0]["readyUrl"],
            json!("http://127.0.0.1:4100/lenso/service/v1/status")
        );
        assert_eq!(manifest["statusPath"], json!("/lenso/service/v1/status"));
        assert_eq!(
            manifest["compatibility"]["remoteProtocolVersion"],
            json!("1")
        );
        assert_eq!(
            manifest["install"]["services"][0]["cwd"],
            json!("../lenso-support-ticket")
        );
    }

    #[test]
    fn remote_scaffold_writes_local_service_state_sample() {
        let value = remote_module_services_local_json(&remote_context());
        let states = parse_remote_module_service_states(&value).unwrap();

        assert_eq!(states[0].module_name, "support-ticket-service");
        assert_eq!(states[0].services[0].name, "api");
        assert_eq!(states[0].services[0].cwd.as_deref(), Some("."));
        assert!(states[0].services[0].auto_start);
    }

    #[test]
    fn remote_scaffold_docs_include_lifecycle_operator_commands() {
        let readme = remote_package_readme("support-ticket", "lenso-support-ticket");
        let runbook = remote_package_runbook("support-ticket");
        let package_json = remote_root_package_json("support-ticket").unwrap();

        assert!(
            readme.contains("lenso service list --module-services-file module-services.local.json")
        );
        assert!(readme.contains("lenso service doctor support-ticket --json"));
        assert!(runbook.contains("lenso service start support-ticket-service api"));
        assert!(runbook.contains("Runtime Console evidence should stay on the host side"));
        assert!(package_json.contains("\"service:export\""));
        assert!(package_json.contains("\"service:status\""));
        assert!(package_json.contains("\"service:verify\""));
    }

    #[test]
    fn env_remote_modules_are_upserted() {
        let source = "APP_ENV=local\nREMOTE_MODULES=crm=http://old\nRUST_LOG=info\n";
        let updated = upsert_env_value(
            source,
            "REMOTE_MODULES",
            &format_remote_module_entries(&[
                ("crm".to_owned(), "http://old".to_owned()),
                ("billing".to_owned(), "http://new".to_owned()),
            ]),
        );

        assert!(updated.contains("APP_ENV=local"));
        assert!(updated.contains("RUST_LOG=info"));
        assert!(updated.contains("REMOTE_MODULES=crm=http://old,billing=http://new"));
    }

    #[test]
    fn env_remote_modules_are_removed() {
        let source = "APP_ENV=local\nREMOTE_MODULES=crm=http://old,billing=http://new\n";
        let updated = remove_remote_module_from_env_source(source, "crm").unwrap();

        assert!(updated.contains("APP_ENV=local"));
        assert!(updated.contains("REMOTE_MODULES=billing=http://new"));
        assert!(!updated.contains("crm=http://old"));
    }

    #[test]
    fn env_remote_modules_line_is_removed_when_empty() {
        let source = "APP_ENV=local\nREMOTE_MODULES=crm=http://old\n";
        let updated = remove_remote_module_from_env_source(source, "crm").unwrap();

        assert_eq!(updated, "APP_ENV=local\n");
    }

    #[test]
    fn linked_module_enabled_env_is_upserted() {
        let source = "APP_ENV=local\n";
        let updated = set_linked_module_enabled_env(source, "auth-password", false);

        assert_eq!(
            updated,
            "APP_ENV=local\nLENSO_MODULE_AUTH_PASSWORD_ENABLED=false\n"
        );
    }

    #[test]
    fn module_source_parses_supported_values() {
        assert_eq!(parse_module_source("remote").unwrap(), ModuleSource::Remote);
        assert_eq!(parse_module_source("linked").unwrap(), ModuleSource::Linked);
        assert!(parse_module_source("wasm").is_err());
    }

    #[test]
    fn catalog_service_entry_resolves_to_service_manifest() {
        let entry = serde_json::json!({
            "name": "support-ticket",
            "source": "service",
            "providedBy": "support-suite-provider",
            "serviceManifest": "http://127.0.0.1:4110/lenso/service/v1/manifest"
        });

        assert_eq!(
            catalog_service_manifest_reference(&entry),
            Some("http://127.0.0.1:4110/lenso/service/v1/manifest")
        );
    }

    #[test]
    fn provider_catalog_entry_resolves_provided_module_to_manifest_reference() {
        let entry = serde_json::json!({
            "name": "support-suite-provider",
            "source": "service",
            "manifestReference": "http://127.0.0.1:4110/lenso/service/v1/manifest",
            "modules": [{ "name": "support-ticket" }]
        });

        assert_eq!(
            catalog_service_manifest_reference_for_module(&entry, "support-ticket"),
            Some("http://127.0.0.1:4110/lenso/service/v1/manifest")
        );
    }

    #[test]
    fn linked_source_skips_service_catalog_resolution() {
        assert!(should_resolve_service_catalog_entry(ModuleSource::Remote));
        assert!(!should_resolve_service_catalog_entry(ModuleSource::Linked));
    }

    #[test]
    fn uninstall_source_infers_linked_for_builtin_when_remote_is_absent() {
        assert_eq!(
            infer_uninstall_module_source("auth", "", false).unwrap(),
            ModuleSource::Linked
        );
    }

    #[test]
    fn uninstall_source_prefers_remote_install_state() {
        assert_eq!(
            infer_uninstall_module_source("auth", "", true).unwrap(),
            ModuleSource::Remote
        );
    }

    #[test]
    fn uninstall_source_infers_linked_from_env_toggle() {
        assert_eq!(
            infer_uninstall_module_source("billing", "LENSO_MODULE_BILLING_ENABLED=true\n", false)
                .unwrap(),
            ModuleSource::Linked
        );
    }

    #[test]
    fn install_ledger_entry_replaces_existing_module() {
        let path = Path::new("/tmp/missing-module-installs.json");
        let entry = simple_linked_module_install_ledger_entry("auth", true, ".env");
        let ledger = update_module_install_ledger(path, entry).unwrap();
        let updated = update_module_install_ledger(
            path,
            simple_linked_module_install_ledger_entry("auth", false, ".env"),
        )
        .unwrap();

        assert_eq!(ledger["modules"].as_array().unwrap().len(), 1);
        assert_eq!(updated["modules"].as_array().unwrap().len(), 1);
        assert_eq!(updated["modules"][0]["enabled"], false);
    }

    #[test]
    fn install_ledger_module_is_removed() {
        let ledger = json!({
            "modules": [
                { "moduleName": "crm", "source": "remote" },
                { "moduleName": "auth", "source": "linked" }
            ],
            "version": 1
        });
        let updated = remove_module_install_ledger_module_value(ledger, "crm")
            .unwrap()
            .unwrap();

        assert_eq!(updated["modules"].as_array().unwrap().len(), 1);
        assert_eq!(updated["modules"][0]["moduleName"], "auth");
    }

    #[test]
    fn service_uninstall_target_expands_provider_modules() {
        let path = std::env::temp_dir().join(format!(
            "lenso-service-uninstall-target-{}.json",
            std::process::id()
        ));
        write_json(
            &path,
            &json!({
                "modules": [
                    {
                        "moduleName": "support-ticket",
                        "service": { "name": "support-service" },
                        "source": "remote"
                    },
                    {
                        "moduleName": "support-sla",
                        "service": { "name": "support-service" },
                        "source": "remote"
                    },
                    {
                        "moduleName": "crm",
                        "source": "remote"
                    }
                ],
                "version": 1
            }),
        )
        .unwrap();

        let by_module = remote_uninstall_target(&path, "support-ticket").unwrap();
        let by_provider = remote_uninstall_target(&path, "support-service").unwrap();
        fs::remove_file(&path).ok();

        assert_eq!(by_module.provider_name, "support-service");
        assert_eq!(
            by_module.module_names,
            vec!["support-ticket", "support-sla"]
        );
        assert_eq!(by_provider.provider_name, "support-service");
        assert_eq!(
            by_provider.module_names,
            vec!["support-ticket", "support-sla"]
        );
    }

    #[test]
    fn install_ledger_entry_is_read_for_module_update() {
        let path = std::env::temp_dir().join(format!(
            "lenso-module-update-ledger-{}.json",
            std::process::id()
        ));
        write_json(
            &path,
            &json!({
                "modules": [
                    {
                        "baseUrl": "http://127.0.0.1:4100/lenso/module/v1",
                        "manifestReference": "http://127.0.0.1:4100/lenso/module/v1/manifest",
                        "moduleName": "crm",
                        "source": "remote"
                    }
                ],
                "version": 1
            }),
        )
        .unwrap();

        let receipt = module_install_ledger_entry(&path, "crm").unwrap().unwrap();
        fs::remove_file(&path).ok();

        assert_eq!(
            receipt.get("manifestReference").and_then(Value::as_str),
            Some("http://127.0.0.1:4100/lenso/module/v1/manifest")
        );
    }

    #[test]
    fn module_update_reference_strips_receipt_prefixes() {
        assert_eq!(module_update_reference("builtin:auth"), "auth");
        assert_eq!(module_update_reference("linked:billing"), "billing");
        assert_eq!(
            module_update_reference("./lenso.module.json"),
            "./lenso.module.json"
        );
    }

    #[test]
    fn module_update_removes_stale_console_artifacts() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-module-update-console-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&repo_root).ok();
        write_json(
            &repo_root.join(".lenso/console-package-install-plan.json"),
            &json!({
                "modules": [
                    { "moduleName": "crm" },
                    { "moduleName": "billing" }
                ],
                "version": 1
            }),
        )
        .unwrap();
        write_json(
            &repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH),
            &json!({
                "bundles": [
                    { "moduleName": "crm" },
                    { "moduleName": "billing" }
                ],
                "version": 1
            }),
        )
        .unwrap();
        write_file(
            &repo_root.join(".lenso/console/extensions/crm/crm-console.js"),
            b"export const crmConsoleModule = {};\n",
        )
        .unwrap();

        let changed =
            remove_stale_module_console_artifacts(&repo_root, "crm", true, false).unwrap();

        let plan = read_json(&repo_root.join(".lenso/console-package-install-plan.json")).unwrap();
        let registry = read_json(&repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH)).unwrap();
        assert_eq!(changed.len(), 3);
        assert!(!repo_root.join(".lenso/console/extensions/crm").exists());
        assert_eq!(
            plan["modules"][0].get("moduleName").and_then(Value::as_str),
            Some("billing")
        );
        assert_eq!(
            registry["bundles"][0]
                .get("moduleName")
                .and_then(Value::as_str),
            Some("billing")
        );
        fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    fn linked_uninstall_call_reads_install_receipt() {
        let path =
            std::env::temp_dir().join(format!("lenso-module-installs-{}.json", std::process::id()));
        let ledger = json!({
            "modules": [
                {
                    "enabled": true,
                    "linked": { "call": "builtins::auth()" },
                    "moduleName": "auth",
                    "source": "linked"
                }
            ],
            "version": 1
        });
        write_json(&path, &ledger).unwrap();

        let call = linked_module_uninstall_call(&path, "auth").unwrap();
        fs::remove_file(&path).ok();

        assert_eq!(call.as_deref(), Some("builtins::auth()"));
    }

    #[test]
    fn linked_module_is_removed_from_host_composition() {
        let source = "pub fn host_composition() -> HostComposition {\n    HostBuilder::new()\n        .linked_module(builtins::auth())\n        .linked_module(modules::app::linked_module())\n        .build()\n}\n";
        let updated = remove_linked_module_from_host_lib_source(source, "builtins::auth()")
            .expect("host lib should change");

        assert!(!updated.contains("builtins::auth()"));
        assert!(updated.contains(".linked_module(modules::app::linked_module())"));
    }

    #[test]
    fn linked_uninstall_includes_installed_dependents_first() {
        let host_lib = "HostBuilder::new()\n    .linked_module(builtins::auth())\n    .linked_module(builtins::auth_password())\n    .linked_module(auth_device::module::linked_module())\n    .build()\n";
        let modules = linked_modules_to_uninstall("auth", None, "", host_lib).unwrap();

        assert_eq!(modules, vec!["auth-password", "auth-device", "auth"]);
    }

    #[test]
    fn linked_modules_are_removed_from_host_composition() {
        let source = "pub fn host_composition() -> HostComposition {\n    HostBuilder::new()\n        .linked_module(builtins::auth())\n        .linked_module(builtins::auth_password())\n        .linked_module(modules::app::linked_module())\n        .build()\n}\n";
        let updated = remove_linked_modules_from_host_lib_source(
            source,
            &[
                "builtins::auth_password()".to_owned(),
                "builtins::auth()".to_owned(),
            ],
        )
        .expect("host lib should change");

        assert!(!updated.contains("builtins::auth()"));
        assert!(!updated.contains("builtins::auth_password()"));
        assert!(updated.contains(".linked_module(modules::app::linked_module())"));
    }

    #[test]
    fn linked_uninstall_removes_console_extension_files() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-linked-uninstall-extension-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&repo_root).ok();
        write_file(&repo_root.join(".env"), b"LENSO_MODULE_AUTH_ENABLED=true\n").unwrap();
        write_file(
            &repo_root.join("src/lib.rs"),
            b"pub fn host_composition() -> HostComposition {\n    HostBuilder::new()\n        .linked_module(builtins::auth())\n        .linked_module(modules::app::linked_module())\n        .build()\n}\n",
        )
        .unwrap();
        write_json(
            &repo_root.join(MODULE_INSTALL_LEDGER_PATH),
            &json!({
                "modules": [{
                    "enabled": true,
                    "linked": { "call": "builtins::auth()" },
                    "moduleName": "auth",
                    "source": "linked"
                }],
                "version": 1
            }),
        )
        .unwrap();
        write_json(
            &repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH),
            &json!({
                "bundles": [
                    { "moduleName": "auth" },
                    { "moduleName": "crm" }
                ],
                "version": 1
            }),
        )
        .unwrap();
        write_file(
            &repo_root.join(".lenso/console/extensions/auth/auth-console.js"),
            b"export const authConsoleModule = {};\n",
        )
        .unwrap();

        uninstall_linked_module(
            "auth",
            RemoteModuleUninstallOptions {
                dry_run: false,
                env_file: None,
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                source: None,
            },
        )
        .unwrap();

        let registry = read_json(&repo_root.join(CONSOLE_EXTENSION_REGISTRY_PATH)).unwrap();
        assert!(!repo_root.join(".lenso/console/extensions/auth").exists());
        assert_eq!(registry["bundles"].as_array().unwrap().len(), 1);
        assert_eq!(registry["bundles"][0]["moduleName"], "crm");
        fs::remove_dir_all(&repo_root).ok();
    }

    #[test]
    fn builtin_auth_descriptor_declares_linked_source() {
        let descriptor = builtin_linked_module_descriptor("auth").expect("auth descriptor");

        assert_eq!(descriptor["name"], "auth");
        assert_eq!(descriptor["source"], "linked");
        assert_eq!(descriptor["linked"]["call"], "builtins::auth()");
        assert_eq!(
            descriptor["console"][0]["package"]["bundleUrl"],
            "https://cdn.jsdelivr.net/npm/@lenso/auth-console@0.1.1/dist/auth-console.js"
        );
    }

    #[test]
    fn builtin_auth_device_descriptor_declares_external_linked_crate() {
        let descriptor =
            builtin_linked_module_descriptor("auth-device").expect("auth-device descriptor");

        assert_eq!(descriptor["source"], "linked");
        assert_eq!(descriptor["dependencies"], json!(["auth"]));
        assert_eq!(
            descriptor["linked"]["call"],
            "auth_device::module::linked_module()"
        );
        assert_eq!(
            descriptor["linked"]["cargo"],
            json!({
                "package": "lenso-module-auth-device",
                "version": "0.1.1"
            })
        );
    }

    #[test]
    fn linked_descriptor_updates_host_cargo_toml() {
        let source = "[package]\nname = \"app\"\n\n[dependencies]\nanyhow = \"1\"\n";
        let cargo = json!({
            "package": "lenso-billing",
            "version": "0.1"
        });

        let updated = update_host_cargo_toml_for_linked_descriptor(source, Some(&cargo))
            .expect("cargo update")
            .expect("cargo should change");

        assert!(updated.contains("[dependencies]\nlenso-billing = \"0.1\"\nanyhow = \"1\""));
    }

    #[test]
    fn linked_install_profile_merges_generic_effects() {
        let descriptor = json!({
            "name": "auth",
            "source": "linked",
            "linked": {
                "call": "builtins::auth()"
            },
            "install": {
                "profiles": {
                    "redis-session-cache": {
                        "linked": {
                            "cargo": {
                                "package": "lenso-module-auth",
                                "version": "0.1.6",
                                "features": ["redis"]
                            }
                        },
                        "env": {
                            "REDIS_URL": "redis://localhost:6379/0"
                        },
                        "runtimeConfigDefaults": {
                            "auth.session_cache": "redis"
                        }
                    }
                }
            }
        });

        let (descriptor, effects) =
            apply_linked_install_profiles(&descriptor, &["redis-session-cache".to_owned()])
                .expect("install profile should apply");

        assert_eq!(descriptor["linked"]["call"], "builtins::auth()");
        assert_eq!(
            descriptor["linked"]["cargo"],
            json!({
                "package": "lenso-module-auth",
                "version": "0.1.6",
                "features": ["redis"]
            })
        );
        assert_eq!(
            effects.env,
            vec![(
                "REDIS_URL".to_owned(),
                "redis://localhost:6379/0".to_owned()
            )]
        );
        assert_eq!(effects.runtime_config_defaults.len(), 1);
        assert_eq!(effects.runtime_config_defaults[0].service, "*");
        assert_eq!(effects.runtime_config_defaults[0].key, "auth.session_cache");
        assert_eq!(effects.runtime_config_defaults[0].value, json!("redis"));
    }

    #[test]
    fn linked_descriptor_updates_existing_dependency_features() {
        let source = "[package]\nname = \"app\"\n\n[dependencies]\nlenso-module-auth = \"0.1.2\"\n";
        let cargo = json!({
            "package": "lenso-module-auth",
            "version": "0.1.2",
            "features": ["redis"]
        });

        let updated = update_host_cargo_toml_for_linked_descriptor(source, Some(&cargo))
            .expect("cargo update")
            .expect("cargo should change");

        assert!(
            updated.contains("lenso-module-auth = { version = \"0.1.2\", features = [\"redis\"] }")
        );
    }

    #[test]
    fn linked_descriptor_adds_features_after_default_features_field() {
        let source = "[dependencies]\nlenso-module-auth = { version = \"0.1.2\", default-features = false }\n";
        let cargo = json!({
            "package": "lenso-module-auth",
            "version": "0.1.2",
            "features": ["redis"]
        });

        let updated = update_host_cargo_toml_for_linked_descriptor(source, Some(&cargo))
            .expect("cargo update")
            .expect("cargo should change");

        assert!(updated.contains(
            "lenso-module-auth = { version = \"0.1.2\", default-features = false, features = [\"redis\"] }"
        ));
    }

    #[test]
    fn runtime_config_defaults_upsert_by_service_and_key() {
        let initial = json!({
            "version": 1,
            "values": [
                { "service": "*", "key": "auth.session_cache", "value": "database" }
            ]
        });
        let updated = update_runtime_config_defaults(
            Some(initial),
            &[RuntimeConfigDefault {
                service: "*".to_owned(),
                key: "auth.session_cache".to_owned(),
                value: json!("redis"),
            }],
        )
        .expect("runtime config defaults update");

        assert_eq!(
            updated,
            json!({
                "version": 1,
                "values": [
                    { "service": "*", "key": "auth.session_cache", "value": "redis" }
                ]
            })
        );
    }

    #[test]
    fn linked_descriptor_updates_host_composition() {
        let source = "mod modules;\n\nuse lenso::host::prelude::*;\n\npub fn host_composition() -> HostComposition {\n    HostBuilder::new()\n        .linked_module(modules::app::linked_module())\n        .build()\n}\n";

        let updated = update_host_lib_for_linked_descriptor(
            source,
            Some("lenso_billing::linked_module"),
            "linked_module()",
        )
        .expect("host lib update");

        assert!(updated.contains("use lenso_billing::linked_module;\n"));
        assert!(updated.contains(
            "        .linked_module(linked_module())\n        .linked_module(modules::app::linked_module())"
        ));
    }

    #[test]
    fn manifest_install_env_updates_source() {
        let updated = apply_manifest_install_env(
            "APP_ENV=local\n".to_owned(),
            &[("CRM_API_URL".to_owned(), "http://crm".to_owned())],
        );

        assert_eq!(updated, "APP_ENV=local\nCRM_API_URL=http://crm\n");
    }

    #[test]
    fn manifest_install_directives_are_parsed_and_planned() {
        let manifest = json!({
            "install": {
                "env": {
                    "CRM_API_URL": "http://crm"
                },
                "commands": [
                    "just migrate",
                    { "command": "pnpm install", "cwd": "../lenso-runtime-console" }
                ]
            }
        });
        let env = remote_module_install_env(&manifest).unwrap();
        let commands = remote_module_install_commands(&manifest).unwrap();
        let command_plan = install_command_plans(&commands, false);

        assert_eq!(
            env,
            vec![("CRM_API_URL".to_owned(), "http://crm".to_owned())]
        );
        assert_eq!(commands[0].command, "just migrate");
        assert_eq!(commands[1].cwd.as_deref(), Some("../lenso-runtime-console"));
        assert_eq!(
            command_plan[0].get("status").and_then(Value::as_str),
            Some("requires_manual_run")
        );
    }

    #[test]
    fn manifest_install_env_cannot_override_remote_modules() {
        let manifest = json!({
            "install": {
                "env": {
                    "REMOTE_MODULES": "crm=http://other"
                }
            }
        });

        assert!(remote_module_install_env(&manifest).is_err());
    }

    #[test]
    fn manifest_install_services_are_planned() {
        let manifest = json!({
            "install": {
                "services": [
                    {
                        "name": "crm-api",
                        "command": "pnpm --dir ../crm/backend dev",
                        "cwd": ".",
                        "readyTimeoutMs": 12000
                    }
                ]
            }
        });
        let services = remote_module_install_services(
            &manifest,
            "crm",
            "http://127.0.0.1:4100/lenso/module/v1",
        )
        .unwrap();
        let service_file = update_remote_module_services_file(
            Path::new("/tmp/missing-module-services.json"),
            "crm",
            &services,
        )
        .unwrap()
        .unwrap();
        let service_plan = install_service_plans(&services);

        assert_eq!(
            services[0].ready_url,
            "http://127.0.0.1:4100/lenso/module/v1/manifest"
        );
        assert_eq!(
            service_plan[0].get("status").and_then(Value::as_str),
            Some("registered")
        );
        assert_eq!(
            service_file
                .get("modules")
                .and_then(Value::as_array)
                .and_then(|modules| modules.first())
                .and_then(|module| module.get("moduleName"))
                .and_then(Value::as_str),
            Some("crm")
        );
    }

    #[test]
    fn remote_module_service_states_are_parsed() {
        let state = json!({
            "modules": [
                {
                    "moduleName": "crm",
                    "services": [
                        {
                            "autoStart": false,
                            "command": "pnpm --dir ../crm/backend dev",
                            "cwd": "../crm",
                            "name": "crm-api",
                            "readyTimeoutMs": 12000,
                            "readyUrl": "http://127.0.0.1:4100/lenso/module/v1/manifest"
                        }
                    ]
                }
            ],
            "version": 1
        });
        let states = parse_remote_module_service_states(&state).unwrap();

        assert_eq!(states.len(), 1);
        assert_eq!(states[0].module_name, "crm");
        assert_eq!(states[0].services[0].name, "crm-api");
        assert_eq!(states[0].services[0].cwd.as_deref(), Some("../crm"));
        assert_eq!(states[0].services[0].ready_timeout_ms, 12000);
        assert!(!states[0].services[0].auto_start);
    }

    #[test]
    fn module_enabled_env_defaults_to_true_and_parses_false() {
        assert!(module_enabled_from_env_source("", "crm"));
        assert!(!module_enabled_from_env_source(
            "LENSO_MODULE_CRM_ENABLED=false\n",
            "crm"
        ));
        assert!(module_enabled_from_env_source(
            "LENSO_MODULE_CRM_ENABLED=yes\n",
            "crm"
        ));
    }

    #[test]
    fn doctor_status_flags_stale_started_state() {
        assert_eq!(
            remote_module_service_doctor_status(true, true, true, false, true, true),
            RemoteModuleServiceDoctorStatus::StaleState
        );
        assert_eq!(
            remote_module_service_doctor_status(true, true, false, false, false, false),
            RemoteModuleServiceDoctorStatus::ManualNotReady
        );
        assert!(
            remote_module_service_doctor_status(true, true, false, false, false, false).is_issue()
        );
        assert_eq!(
            remote_module_service_doctor_status(false, true, true, true, false, false),
            RemoteModuleServiceDoctorStatus::NotConfigured
        );
    }

    #[test]
    fn remote_manifest_url_only_checks_http_sources() {
        assert_eq!(
            remote_module_manifest_url("https://example.com/lenso/module/v1"),
            Some("https://example.com/lenso/module/v1/manifest".to_owned())
        );
        assert_eq!(
            remote_module_manifest_url("https://example.com/lenso/module/v1/manifest"),
            Some("https://example.com/lenso/module/v1/manifest".to_owned())
        );
        assert_eq!(remote_module_manifest_url("grpc://example.com:50051"), None);
    }

    #[test]
    fn remote_manifest_compatibility_blocks_unsupported_protocol() {
        let manifest = json!({
            "compatibility": {
                "remoteProtocolVersion": "99"
            },
            "name": "billing"
        });

        let issue = remote_module_manifest_compatibility_issue(&manifest).unwrap();

        assert!(issue.contains("requires remote protocol 99"));
    }

    #[test]
    fn remote_install_receipt_keeps_service_metadata() {
        let manifest = json!({
            "compatibility": { "remoteProtocolVersion": "1" },
            "deployment": { "target": "container-paas" },
            "service": { "name": "api", "statusUrl": "http://127.0.0.1:4100/status" }
        });

        let receipt = remote_module_install_ledger_entry(
            "billing",
            "http://127.0.0.1:4100/manifest",
            "http://127.0.0.1:4100",
            &manifest,
            Vec::new(),
            &[],
            &[],
            &[],
            0,
        );

        assert_eq!(receipt["service"]["name"], json!("api"));
        assert_eq!(receipt["deployment"]["target"], json!("container-paas"));
        assert_eq!(
            receipt["compatibility"]["remoteProtocolVersion"],
            json!("1")
        );
    }

    #[test]
    fn service_manifest_modules_become_remote_module_manifests() {
        let manifest = validate_service_manifest(json!({
            "compatibility": { "remoteProtocolVersion": "1" },
            "deployment": { "target": "container-paas" },
            "install": {
                "services": [
                    {
                        "command": "pnpm start",
                        "name": "support-service"
                    }
                ]
            },
            "modules": [
                {
                    "capabilities": ["support.tickets.read"],
                    "console": [],
                    "name": "support-ticket"
                }
            ],
            "name": "support-service",
            "protocol": "lenso.service.v1",
            "required_env": ["SUPPORT_DATABASE_URL"],
            "status_path": "/lenso/service/v1/readyz",
            "transports": ["http"],
            "version": "0.1.0"
        }))
        .unwrap();

        let modules = service_module_install_manifests(
            &manifest,
            "https://support.example.test/lenso/service/v1/manifest",
            "https://support.example.test/lenso/service/v1",
        )
        .unwrap();

        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0]["name"], json!("support-ticket"));
        assert_eq!(modules[0]["source"], json!("remote"));
        assert_eq!(modules[0]["version"], json!("0.1.0"));
        assert_eq!(
            modules[0]["compatibility"]["remoteProtocolVersion"],
            json!("1")
        );
        assert_eq!(modules[0]["deployment"]["target"], json!("container-paas"));
        assert_eq!(modules[0]["service"]["name"], json!("support-service"));
        assert_eq!(
            modules[0]["service"]["baseUrl"],
            json!("https://support.example.test/lenso/service/v1")
        );
        assert_eq!(
            modules[0]["service"]["statusPath"],
            json!("/lenso/service/v1/readyz")
        );
        assert_eq!(
            modules[0]["service"]["statusUrl"],
            json!("https://support.example.test/lenso/service/v1/readyz")
        );
        assert_eq!(
            modules[0]["service"]["requiredEnv"],
            json!(["SUPPORT_DATABASE_URL"])
        );
    }

    #[tokio::test]
    async fn service_package_install_records_package_provenance() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-service-package-install-{}",
            uuid::Uuid::now_v7()
        ));
        let package_dir = repo_root.join("artifact");
        fs::create_dir_all(&package_dir).unwrap();
        write_json(
            &package_dir.join("lenso.service.json"),
            &json!({
                "name": "support-suite-provider",
                "protocol": "lenso.service.v1",
                "version": "0.2.0",
                "modules": [
                    {
                        "capabilities": ["support_ticket.tickets.read"],
                        "name": "support-ticket"
                    }
                ]
            }),
        )
        .unwrap();
        write_json(
            &package_dir.join("lenso.service-package.json"),
            &json!({
                "protocol": "lenso.service-package.v1",
                "name": "support-suite-provider",
                "version": "0.2.0",
                "serviceManifest": "lenso.service.json",
                "modules": ["support-ticket"]
            }),
        )
        .unwrap();

        add_remote_module(
            &package_dir
                .join("lenso.service-package.json")
                .to_string_lossy(),
            RemoteModuleInstallOptions {
                allow_incompatible: false,
                base_url: Some("http://127.0.0.1:4110/lenso/service/v1".to_owned()),
                console_plan: false,
                dry_run: false,
                env_file: None,
                install_profiles: Vec::new(),
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                run_install_commands: false,
                source: "remote".to_owned(),
            },
        )
        .await
        .unwrap();

        let ledger = read_json(&repo_root.join(MODULE_INSTALL_LEDGER_PATH)).unwrap();
        let entry = ledger["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["moduleName"] == "support-ticket")
            .unwrap();

        assert_eq!(
            entry["manifestReference"],
            json!(
                package_dir
                    .join("lenso.service.json")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert_eq!(
            entry["servicePackage"]["manifestReference"],
            json!(
                package_dir
                    .join("lenso.service-package.json")
                    .to_string_lossy()
                    .to_string()
            )
        );
        assert_eq!(
            entry["servicePackage"]["manifestSnapshot"]["protocol"],
            json!("lenso.service-package.v1")
        );
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn module_release_install_records_release_provenance() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-module-release-install-{}",
            uuid::Uuid::now_v7()
        ));
        let package_dir = repo_root.join("artifact");
        fs::create_dir_all(&package_dir).unwrap();
        write_json(
            &package_dir.join("lenso.service.json"),
            &json!({
                "name": "support-suite-provider",
                "protocol": "lenso.service.v1",
                "version": "0.3.0",
                "modules": [
                    {
                        "capabilities": ["support_ticket.tickets.read"],
                        "name": "support-ticket",
                        "version": "0.3.0"
                    }
                ]
            }),
        )
        .unwrap();
        write_json(
            &package_dir.join("lenso.service-package.json"),
            &json!({
                "protocol": "lenso.service-package.v1",
                "name": "support-suite-provider",
                "version": "0.3.0",
                "serviceManifest": "lenso.service.json",
                "modules": ["support-ticket"]
            }),
        )
        .unwrap();
        let release_path = package_dir.join("lenso.module-release.json");
        write_json(
            &release_path,
            &json!({
                "protocol": "lenso.module-release.v1",
                "name": "support-ticket",
                "version": "0.3.0",
                "source": "service",
                "provider": {
                    "name": "support-suite-provider",
                    "servicePackage": "lenso.service-package.json"
                },
                "capabilities": ["support_ticket.tickets.read"]
            }),
        )
        .unwrap();

        install_module(
            &release_path.to_string_lossy(),
            RemoteModuleInstallOptions {
                allow_incompatible: false,
                base_url: Some("http://127.0.0.1:4110/lenso/service/v1".to_owned()),
                console_plan: false,
                dry_run: false,
                env_file: None,
                install_profiles: Vec::new(),
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                run_install_commands: false,
                source: "remote".to_owned(),
            },
        )
        .await
        .unwrap();

        let ledger = read_json(&repo_root.join(MODULE_INSTALL_LEDGER_PATH)).unwrap();
        let entry = ledger["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["moduleName"] == "support-ticket")
            .unwrap();

        assert_eq!(
            entry["moduleRelease"]["manifestReference"],
            json!(release_path.to_string_lossy().to_string())
        );
        assert_eq!(
            entry["moduleRelease"]["manifestSnapshot"]["protocol"],
            json!("lenso.module-release.v1")
        );
        assert_eq!(
            entry["servicePackage"]["manifestSnapshot"]["protocol"],
            json!("lenso.service-package.v1")
        );
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn module_catalog_install_resolves_module_release() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-module-release-catalog-{}",
            uuid::Uuid::now_v7()
        ));
        let package_dir = repo_root.join("artifact");
        fs::create_dir_all(&package_dir).unwrap();
        write_json(
            &package_dir.join("lenso.service.json"),
            &json!({
                "name": "support-suite-provider",
                "protocol": "lenso.service.v1",
                "version": "0.4.0",
                "modules": [
                    {
                        "name": "support-ticket",
                        "version": "0.4.0"
                    }
                ]
            }),
        )
        .unwrap();
        write_json(
            &package_dir.join("lenso.service-package.json"),
            &json!({
                "protocol": "lenso.service-package.v1",
                "name": "support-suite-provider",
                "version": "0.4.0",
                "serviceManifest": "lenso.service.json",
                "modules": ["support-ticket"]
            }),
        )
        .unwrap();
        let release_path = package_dir.join("lenso.module-release.json");
        write_json(
            &repo_root.join(MODULE_CATALOG_PATH),
            &json!({
                "version": 1,
                "modules": [
                    {
                        "protocol": "lenso.module-release.v1",
                        "manifestReference": release_path.to_string_lossy().to_string(),
                        "name": "support-ticket",
                        "version": "0.4.0",
                        "baseUrl": "http://127.0.0.1:4110/lenso/service/v1",
                        "source": "service",
                        "provider": {
                            "name": "support-suite-provider",
                            "servicePackage": "lenso.service-package.json"
                        }
                    }
                ]
            }),
        )
        .unwrap();

        install_module(
            "support-ticket",
            RemoteModuleInstallOptions {
                allow_incompatible: false,
                base_url: None,
                console_plan: false,
                dry_run: false,
                env_file: None,
                install_profiles: Vec::new(),
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                run_install_commands: false,
                source: "remote".to_owned(),
            },
        )
        .await
        .unwrap();

        let ledger = read_json(&repo_root.join(MODULE_INSTALL_LEDGER_PATH)).unwrap();
        let entry = ledger["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["moduleName"] == "support-ticket")
            .unwrap();

        assert_eq!(
            entry["moduleRelease"]["manifestReference"],
            json!(release_path.to_string_lossy().to_string())
        );
        assert_eq!(
            entry["moduleRelease"]["manifestSnapshot"]["version"],
            json!("0.4.0")
        );
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn module_catalog_add_records_module_release_entry() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-module-release-catalog-add-{}",
            uuid::Uuid::now_v7()
        ));
        let release_path = repo_root.join("lenso.module-release.json");
        write_json(
            &release_path,
            &json!({
                "protocol": "lenso.module-release.v1",
                "name": "support-ticket",
                "version": "0.5.0",
                "source": "service",
                "provider": {
                    "name": "support-suite-provider",
                    "serviceManifest": "http://127.0.0.1:4110/lenso/service/v1/manifest"
                },
                "capabilities": ["support_ticket.tickets.read"]
            }),
        )
        .unwrap();

        add_module_catalog_entry(
            &release_path.to_string_lossy(),
            ModuleCatalogAddOptions {
                base_url: Some("http://127.0.0.1:4110/lenso/service/v1".to_owned()),
                catalog_file: None,
                dry_run: false,
                repo_root: Some(repo_root.clone()),
                summary: None,
            },
        )
        .await
        .unwrap();

        let catalog = read_json(&repo_root.join(MODULE_CATALOG_PATH)).unwrap();
        assert_eq!(catalog["modules"][0]["name"], json!("support-ticket"));
        assert_eq!(
            catalog["modules"][0]["protocol"],
            json!("lenso.module-release.v1")
        );
        assert_eq!(
            catalog["modules"][0]["manifestReference"],
            json!(release_path.to_string_lossy().to_string())
        );
        assert_eq!(
            catalog["modules"][0]["provider"]["name"],
            json!("support-suite-provider")
        );
        assert_eq!(
            catalog["modules"][0]["providedBy"],
            json!("support-suite-provider")
        );
        assert_eq!(
            catalog["modules"][0]["serviceManifest"],
            json!("http://127.0.0.1:4110/lenso/service/v1/manifest")
        );
        assert_eq!(
            catalog["modules"][0]["baseUrl"],
            json!("http://127.0.0.1:4110/lenso/service/v1")
        );
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn module_catalog_add_records_linked_module_release_entry() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-linked-module-release-catalog-add-{}",
            uuid::Uuid::now_v7()
        ));
        let release_path = repo_root.join("lenso.module-release.json");
        write_json(
            &release_path,
            &json!({
                "protocol": "lenso.module-release.v1",
                "name": "auth-password",
                "version": "0.5.0",
                "source": "linked",
                "capabilities": ["auth.password.login"],
                "linked": {
                    "call": "modules::auth_password::linked_module()"
                }
            }),
        )
        .unwrap();

        add_module_catalog_entry(
            &release_path.to_string_lossy(),
            ModuleCatalogAddOptions {
                base_url: None,
                catalog_file: None,
                dry_run: false,
                repo_root: Some(repo_root.clone()),
                summary: None,
            },
        )
        .await
        .unwrap();

        let catalog = read_json(&repo_root.join(MODULE_CATALOG_PATH)).unwrap();
        assert_eq!(catalog["modules"][0]["name"], json!("auth-password"));
        assert_eq!(catalog["modules"][0]["source"], json!("linked"));
        assert_eq!(catalog["modules"][0]["provider"], Value::Null);
        assert_eq!(
            catalog["modules"][0]["linked"]["call"],
            json!("modules::auth_password::linked_module()")
        );
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn module_release_check_requires_base_url_for_local_package() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-module-release-inspect-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&repo_root).unwrap();
        let release_path = repo_root.join("lenso.module-release.json");
        write_json(
            &release_path,
            &json!({
                "protocol": "lenso.module-release.v1",
                "name": "support-ticket",
                "version": "0.5.0",
                "source": "service",
                "provider": {
                    "name": "support-suite-provider",
                    "servicePackage": "lenso.service-package.json"
                }
            }),
        )
        .unwrap();

        let error = inspect_module_release(
            &release_path.to_string_lossy(),
            ModuleReleaseInspectOptions {
                base_url: None,
                check: true,
                json: true,
                repo_root: None,
            },
        )
        .await
        .unwrap_err();

        assert!(
            error.to_string().contains(
                "needs --base-url because its service reference is not an HTTP /manifest URL"
            ),
            "{error}"
        );
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn module_release_check_accepts_base_url_for_local_package() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-module-release-inspect-base-url-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&repo_root).unwrap();
        let release_path = repo_root.join("lenso.module-release.json");
        write_json(
            &release_path,
            &json!({
                "protocol": "lenso.module-release.v1",
                "name": "support-ticket",
                "version": "0.5.0",
                "source": "service",
                "provider": {
                    "name": "support-suite-provider",
                    "servicePackage": "lenso.service-package.json"
                }
            }),
        )
        .unwrap();

        inspect_module_release(
            &release_path.to_string_lossy(),
            ModuleReleaseInspectOptions {
                base_url: Some("http://127.0.0.1:4110/lenso/service/v1".to_owned()),
                check: true,
                json: true,
                repo_root: None,
            },
        )
        .await
        .unwrap();
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn module_release_check_accepts_linked_release_without_provider() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-linked-module-release-inspect-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&repo_root).unwrap();
        let release_path = repo_root.join("lenso.module-release.json");
        write_json(
            &release_path,
            &json!({
                "protocol": "lenso.module-release.v1",
                "name": "auth-password",
                "version": "0.5.0",
                "source": "linked"
            }),
        )
        .unwrap();

        inspect_module_release(
            &release_path.to_string_lossy(),
            ModuleReleaseInspectOptions {
                base_url: None,
                check: true,
                json: true,
                repo_root: None,
            },
        )
        .await
        .unwrap();
        fs::remove_dir_all(repo_root).ok();
    }

    #[test]
    fn service_package_manifest_reference_resolves_relative_url() {
        let package = validate_service_package_manifest(json!({
            "protocol": "lenso.service-package.v1",
            "name": "support-suite-provider",
            "version": "0.2.0",
            "serviceManifest": "lenso.service.json",
            "modules": ["support-ticket"]
        }))
        .unwrap();

        let reference = service_package_manifest_reference(
            "https://example.com/releases/support/lenso.service-package.json",
            &package,
        )
        .unwrap();

        assert_eq!(
            reference,
            "https://example.com/releases/support/lenso.service.json"
        );
    }

    #[test]
    fn service_package_must_match_service_manifest_modules() {
        let package = validate_service_package_manifest(json!({
            "protocol": "lenso.service-package.v1",
            "name": "support-suite-provider",
            "version": "0.2.0",
            "serviceManifest": "lenso.service.json",
            "modules": ["support-ticket", "support-inbox"]
        }))
        .unwrap();
        let service_manifest = validate_service_manifest(json!({
            "name": "support-suite-provider",
            "protocol": "lenso.service.v1",
            "version": "0.2.0",
            "modules": [
                {"name": "support-ticket"}
            ]
        }))
        .unwrap();

        let error = ensure_service_package_matches_manifest(&package, &service_manifest)
            .unwrap_err()
            .to_string();

        assert!(error.contains("modules do not match"));
    }

    #[test]
    fn service_install_services_default_to_service_status_url() {
        let manifest = validate_service_manifest(json!({
            "install": {
                "services": [
                    {
                        "command": "pnpm dev",
                        "name": "support-api"
                    }
                ]
            },
            "modules": [
                { "name": "support-ticket" }
            ],
            "name": "support-service",
            "protocol": "lenso.service.v1",
            "status_path": "/lenso/service/v1/status",
            "version": "0.1.0"
        }))
        .unwrap();

        let services = service_manifest_install_services(
            &manifest,
            "support-service",
            "http://127.0.0.1:4110/lenso/service/v1",
        )
        .unwrap();

        assert_eq!(services.len(), 1);
        assert_eq!(
            services[0].ready_url,
            "http://127.0.0.1:4110/lenso/service/v1/status"
        );
    }

    #[test]
    fn service_check_infers_urls_from_ready_url() {
        let manifest = json!({
            "health": {
                "readyUrl": "http://127.0.0.1:4110/lenso/service/v1/status"
            },
            "modules": [{ "name": "support-ticket" }],
            "name": "support-service",
            "version": "0.1.0"
        });

        assert_eq!(
            service_check_ready_url(Some(&manifest), None, None).as_deref(),
            Some("http://127.0.0.1:4110/lenso/service/v1/status")
        );
        assert_eq!(
            service_check_manifest_url("./lenso.service.json", Some(&manifest), None).as_deref(),
            Some("http://127.0.0.1:4110/lenso/service/v1/manifest")
        );
    }

    #[test]
    fn service_manifest_operations_include_kinds_and_safe_probe_state() {
        let manifest = json!({
            "modules": [
                {
                    "admin": {
                        "kind": "declarative_custom",
                        "actions": [
                            {
                                "capability": "support_ticket.tickets.write",
                                "name": "assign_ticket"
                            }
                        ]
                    },
                    "events": {
                        "handlers": [
                            {
                                "name": "ticket_created",
                                "operation": {
                                    "operationId": "support-ticket/event/ticket-created-handler"
                                }
                            }
                        ]
                    },
                    "http_routes": [
                        {
                            "capability": "support_ticket.tickets.read",
                            "method": "GET",
                            "operation": {
                                "operationId": "support-ticket/http/list",
                                "safeProbe": {
                                    "method": "GET",
                                    "path": "/tickets"
                                }
                            },
                            "path": "/tickets"
                        }
                    ],
                    "name": "support-ticket",
                    "runtime": {
                        "functions": [
                            { "name": "support-ticket.reindex.v1" }
                        ]
                    }
                }
            ],
            "name": "support-suite-provider",
            "version": "0.1.0"
        });

        let operations = service_manifest_operations(&manifest, None);

        assert_eq!(
            operations
                .iter()
                .map(|operation| operation["operationId"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec![
                "support-ticket/action/assign_ticket",
                "support-ticket/event/ticket-created-handler",
                "support-ticket/http/list",
                "support-ticket/runtime/support-ticket.reindex.v1",
            ]
        );
        assert_eq!(operations[0]["kind"], json!("admin_action"));
        assert_eq!(operations[0]["name"], json!("assign_ticket"));
        assert_eq!(operations[0]["safeProbe"], json!(false));
        assert_eq!(operations[1]["kind"], json!("event_handler"));
        assert_eq!(operations[1]["name"], json!("ticket_created"));
        assert_eq!(operations[1]["safeProbe"], json!(false));
        assert_eq!(operations[2]["kind"], json!("http_route"));
        assert_eq!(operations[2]["method"], json!("GET"));
        assert_eq!(operations[2]["path"], json!("/tickets"));
        assert_eq!(operations[2]["safeProbe"], json!(true));
        assert_eq!(operations[3]["kind"], json!("runtime_function"));
        assert_eq!(operations[3]["name"], json!("support-ticket.reindex.v1"));
        assert_eq!(operations[3]["safeProbe"], json!(false));
    }

    #[test]
    fn service_manifest_operations_filter_by_operation_id() {
        let manifest = json!({
            "modules": [
                {
                    "httpRoutes": [
                        { "method": "GET", "path": "/tickets" },
                        { "method": "GET", "path": "/tickets/{id}" }
                    ],
                    "name": "support-ticket"
                }
            ],
            "name": "support-suite-provider",
            "version": "0.1.0"
        });

        let operations =
            service_manifest_operations(&manifest, Some("support-ticket/http/GET:/tickets/{id}"));

        assert_eq!(operations.len(), 1);
        assert_eq!(
            operations[0]["operationId"],
            "support-ticket/http/GET:/tickets/{id}"
        );
        assert_eq!(operations[0]["path"], "/tickets/{id}");
    }

    #[tokio::test]
    async fn service_manifest_operations_safe_probe_false_is_skipped() {
        let manifest = json!({
            "modules": [
                {
                    "httpRoutes": [
                        {
                            "method": "GET",
                            "operation": {
                                "operationId": "support-ticket/http/camel-false",
                                "safeProbe": false
                            },
                            "path": "/tickets"
                        },
                        {
                            "method": "GET",
                            "operation": {
                                "operationId": "support-ticket/http/snake-false",
                                "safe_probe": false
                            },
                            "path": "/tickets/open"
                        }
                    ],
                    "name": "support-ticket"
                }
            ],
            "name": "support-suite-provider",
            "version": "0.1.0"
        });

        let operations = service_manifest_operations(&manifest, None);

        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0]["safeProbe"], json!(false));
        assert_eq!(operations[1]["safeProbe"], json!(false));

        let probes = service_check_operation_probe_summary(
            &operations,
            "http://127.0.0.1:4110/lenso/service/v1/manifest",
            None,
        )
        .await
        .unwrap();

        assert_eq!(probes.len(), 2);
        assert_eq!(probes[0]["status"], "skipped");
        assert_eq!(probes[1]["status"], "skipped");
    }

    #[tokio::test]
    async fn service_check_does_not_read_unused_sample_input() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let manifest_path = std::env::temp_dir().join(format!(
            "lenso-service-check-unused-sample-input-{nonce}.json"
        ));
        let missing_sample_input =
            std::env::temp_dir().join(format!("lenso-missing-sample-input-{nonce}.json"));
        write_json(
            &manifest_path,
            &json!({
                "modules": [
                    { "name": "support-ticket" }
                ],
                "name": "support-suite-provider",
                "version": "0.1.0"
            }),
        )
        .unwrap();

        let result = check_service_manifest_reference(
            manifest_path.to_str().unwrap(),
            ServiceManifestCheckOptions {
                cwd: None,
                json: true,
                manifest_url: None,
                operation: None,
                ready_timeout_ms: 10_000,
                ready_url: None,
                sample_input: Some(missing_sample_input),
                serve_command: None,
            },
        )
        .await;
        fs::remove_file(&manifest_path).ok();

        assert!(result.is_ok(), "{result:?}");
    }

    #[tokio::test]
    async fn service_check_operation_probe_summary_uses_ok_status_for_success() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .unwrap();
        });
        let operations = vec![json!({
            "kind": "http_route",
            "method": "GET",
            "module": "support-ticket",
            "operationId": "support-ticket/http/GET:/tickets",
            "path": "/tickets",
            "safeProbe": true
        })];

        let probes = service_check_operation_probe_summary(
            &operations,
            &format!("http://{addr}/lenso/service/v1/manifest"),
            None,
        )
        .await
        .unwrap();
        server.join().unwrap();

        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0]["status"], "ok");
    }

    #[tokio::test]
    async fn service_check_operation_probe_summary_skips_unsafe_operations() {
        let operations = vec![
            json!({
                "kind": "runtime_function",
                "module": "support-ticket",
                "name": "support-ticket.reindex.v1",
                "operationId": "support-ticket/runtime/support-ticket.reindex.v1",
                "safeProbe": false
            }),
            json!({
                "kind": "http_route",
                "method": "POST",
                "module": "support-ticket",
                "operationId": "support-ticket/http/POST:/tickets",
                "path": "/tickets",
                "safeProbe": true
            }),
        ];

        let probes = service_check_operation_probe_summary(
            &operations,
            "http://127.0.0.1:4110/lenso/service/v1/manifest",
            None,
        )
        .await
        .unwrap();

        assert_eq!(probes.len(), 2);
        assert_eq!(probes[0]["operationId"], operations[0]["operationId"]);
        assert_eq!(probes[0]["status"], "skipped");
        assert_eq!(probes[1]["operationId"], operations[1]["operationId"]);
        assert_eq!(probes[1]["status"], "skipped");
    }

    #[test]
    fn service_manifest_validation_reports_contract_paths() {
        let missing_command = validate_service_manifest(json!({
            "install": {
                "services": [
                    { "name": "support-service" }
                ]
            },
            "modules": [
                { "name": "support-ticket" }
            ],
            "name": "support-service",
            "version": "0.1.0"
        }))
        .unwrap_err()
        .to_string();
        assert!(missing_command.contains("$.install.services[0].command"));

        let bad_capability = validate_service_manifest(json!({
            "modules": [
                {
                    "capabilities": ["support.tickets.read", 42],
                    "name": "support-ticket"
                }
            ],
            "name": "support-service",
            "version": "0.1.0"
        }))
        .unwrap_err()
        .to_string();
        assert!(bad_capability.contains("$.modules[0].capabilities[1]"));
    }

    #[test]
    fn service_manifest_diff_reports_modules_capabilities_and_operations() {
        let current = json!({
            "modules": [
                {
                    "capabilities": ["support.read"],
                    "http_routes": [{ "method": "GET", "path": "/tickets" }],
                    "name": "support-ticket"
                }
            ],
            "name": "support-service",
            "requiredEnv": ["PORT"],
            "version": "0.1.0"
        });
        let candidate = json!({
            "config": [{ "key": "support.mode" }],
            "modules": [
                {
                    "capabilities": ["support.read", "support.write"],
                    "http_routes": [
                        { "method": "GET", "path": "/tickets" },
                        { "method": "POST", "path": "/tickets" }
                    ],
                    "name": "support-ticket"
                },
                { "name": "support-kb" }
            ],
            "name": "support-service",
            "requiredEnv": ["PORT", "SUPPORT_API_KEY"],
            "version": "0.2.0"
        });

        let diff = service_manifest_diff(&current, &candidate);

        assert_eq!(diff["modules"]["added"], json!(["support-kb"]));
        assert_eq!(diff["env"]["added"], json!(["SUPPORT_API_KEY"]));
        assert_eq!(diff["config"]["added"], json!(["support.mode"]));
        assert_eq!(diff["capabilities"][0]["added"], json!(["support.write"]));
        assert_eq!(
            diff["operations"][0]["added"],
            json!(["route:POST /tickets"])
        );
    }

    #[test]
    fn service_catalog_entry_records_provider_and_modules() {
        let manifest = validate_service_manifest(json!({
            "compatibility": { "remoteProtocolVersion": "1" },
            "deployment": { "target": "container-paas" },
            "install": {
                "services": [
                    {
                        "command": "pnpm start",
                        "name": "support-service"
                    }
                ]
            },
            "modules": [
                {
                    "capabilities": ["support.tickets.read"],
                    "name": "support-ticket",
                    "version": "0.1.0"
                }
            ],
            "name": "support-service",
            "protocol": "lenso.service.v1",
            "required_env": ["PORT"],
            "status_path": "/lenso/service/v1/status",
            "transports": ["http"],
            "version": "0.1.0"
        }))
        .unwrap();

        let entry = service_catalog_entry_from_manifest(
            &manifest,
            "http://127.0.0.1:4110/lenso/service/v1/manifest",
            "http://127.0.0.1:4110/lenso/service/v1",
            Some("Support ticket service"),
        )
        .unwrap();

        assert_eq!(entry["name"], json!("support-service"));
        assert_eq!(entry["source"], json!("service"));
        assert_eq!(entry["modules"][0]["name"], json!("support-ticket"));
        assert_eq!(
            entry["service"]["statusUrl"],
            json!("http://127.0.0.1:4110/lenso/service/v1/status")
        );
        assert_eq!(entry["deployment"]["target"], json!("container-paas"));
        assert_eq!(
            entry["install"]["services"][0]["name"],
            json!("support-service")
        );
        assert_eq!(entry["compatibility"]["remoteProtocolVersion"], json!("1"));
    }

    #[test]
    fn module_install_ledger_upserts_multiple_service_modules() {
        let ledger = upsert_module_install_ledger_entry(
            json!({ "modules": [], "version": 1 }),
            json!({ "moduleName": "support-ticket", "source": "remote" }),
        )
        .unwrap();
        let ledger = upsert_module_install_ledger_entry(
            ledger,
            json!({ "moduleName": "support-sla", "source": "remote" }),
        )
        .unwrap();
        let ledger = upsert_module_install_ledger_entry(
            ledger,
            json!({ "moduleName": "support-ticket", "source": "remote", "enabled": true }),
        )
        .unwrap();
        let modules = ledger.get("modules").and_then(Value::as_array).unwrap();

        assert_eq!(modules.len(), 2);
        assert!(modules.iter().any(|module| {
            module.get("moduleName").and_then(Value::as_str) == Some("support-sla")
        }));
        assert!(modules.iter().any(|module| {
            module.get("moduleName").and_then(Value::as_str) == Some("support-ticket")
                && module.get("enabled").and_then(Value::as_bool) == Some(true)
        }));
    }

    #[test]
    fn compose_export_uses_declared_service_state() {
        let state = RemoteModuleServiceState {
            module_name: "support-ticket".to_owned(),
            services: vec![RemoteModuleServiceInstallSpec {
                name: "api".to_owned(),
                command: "pnpm start".to_owned(),
                cwd: Some("examples/support-ticket".to_owned()),
                ready_url: "http://127.0.0.1:4110/lenso/module/v1/status".to_owned(),
                ready_timeout_ms: 10_000,
                auto_start: true,
            }],
        };
        let source = compose_service_export_source(&state);

        assert!(source.contains("support-ticket-api:"));
        assert!(source.contains("pnpm start"));
        assert!(source.contains("lenso.ready_url"));
        assert!(systemd_service_export_source(&state).contains("ExecStart=/bin/sh -lc"));
        assert!(dockerfile_service_export_source(&state).contains("CMD [\"sh\", \"-lc\""));
        assert!(env_service_export_source(&state, None).contains("LENSO_API_READY_URL="));
    }

    #[test]
    fn doctor_manifest_status_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(ModuleDoctorManifestStatus::Unreachable).unwrap(),
            json!("unreachable")
        );
    }

    #[test]
    fn module_service_list_items_filter_by_module() {
        let states = vec![
            RemoteModuleServiceState {
                module_name: "crm".to_owned(),
                services: vec![RemoteModuleServiceInstallSpec {
                    name: "api".to_owned(),
                    command: "pnpm dev".to_owned(),
                    cwd: None,
                    ready_url: "http://127.0.0.1:4100/readyz".to_owned(),
                    ready_timeout_ms: 10_000,
                    auto_start: true,
                }],
            },
            RemoteModuleServiceState {
                module_name: "billing".to_owned(),
                services: vec![RemoteModuleServiceInstallSpec {
                    name: "api".to_owned(),
                    command: "node server.mjs".to_owned(),
                    cwd: None,
                    ready_url: "http://127.0.0.1:4200/readyz".to_owned(),
                    ready_timeout_ms: 10_000,
                    auto_start: false,
                }],
            },
        ];

        let items = module_service_list_items(&states, Some("billing"));

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].module_name, "billing");
        assert_eq!(items[0].service_name, "api");
        assert_eq!(items[0].auto_start, false);
    }

    #[test]
    fn module_service_list_report_serializes_camel_case() {
        let report = ModuleServiceListReport {
            services: vec![ModuleServiceListItem {
                module_name: "support-ticket".to_owned(),
                service_name: "api".to_owned(),
                auto_start: true,
                command: "pnpm dev".to_owned(),
                ready_url: "http://127.0.0.1:4110/readyz".to_owned(),
            }],
        };
        let value = serde_json::to_value(report).unwrap();

        assert_eq!(value["services"][0]["moduleName"], json!("support-ticket"));
        assert_eq!(value["services"][0]["serviceName"], json!("api"));
        assert_eq!(value["services"][0]["autoStart"], json!(true));
    }

    #[test]
    fn remote_module_service_state_path_sanitizes_names() {
        let service = RemoteModuleServiceInstallSpec {
            name: "API Worker".to_owned(),
            command: "node server.mjs".to_owned(),
            cwd: None,
            ready_url: "http://127.0.0.1:4100/lenso/module/v1/manifest".to_owned(),
            ready_timeout_ms: 10_000,
            auto_start: true,
        };
        let path =
            remote_module_service_state_path(Path::new(".lenso"), "CRM Module", &service, "lock");

        assert_eq!(
            path,
            PathBuf::from(".lenso/remote-crm-module-api-worker.lock")
        );
    }

    #[test]
    fn module_service_log_path_sanitizes_names() {
        let path = module_service_log_path(Path::new("/repo"), "CRM Module", "API Worker");

        assert_eq!(
            path,
            PathBuf::from("/repo/.lenso/service-logs/crm-module/api-worker.log")
        );
    }

    #[test]
    fn tail_lines_returns_requested_suffix() {
        let lines = tail_lines("one\ntwo\nthree\n", 2);

        assert_eq!(lines, vec!["two", "three"]);
        assert_eq!(tail_lines("one\ntwo\n", 10), vec!["one", "two"]);
        assert!(tail_lines("one\ntwo\n", 0).is_empty());
    }

    #[test]
    fn install_plan_module_is_removed() {
        let plan = json!({
            "modules": [
                { "moduleName": "crm", "consolePackages": [] },
                { "moduleName": "billing", "consolePackages": [] }
            ],
            "version": 1
        });
        let updated = remove_console_package_install_plan_module_value(plan, "crm")
            .unwrap()
            .unwrap();
        let modules = updated.get("modules").and_then(Value::as_array).unwrap();

        assert_eq!(modules.len(), 1);
        assert_eq!(
            modules[0].get("moduleName").and_then(Value::as_str),
            Some("billing")
        );
    }

    #[test]
    fn install_plan_service_modules_are_removed_together() {
        let path = std::env::temp_dir().join(format!(
            "lenso-console-install-plan-{}.json",
            std::process::id()
        ));
        write_json(
            &path,
            &json!({
                "modules": [
                    { "moduleName": "support-ticket", "consolePackages": [] },
                    { "moduleName": "support-sla", "consolePackages": [] },
                    { "moduleName": "billing", "consolePackages": [] }
                ],
                "version": 1
            }),
        )
        .unwrap();

        let updated = remove_console_package_install_plan_modules(
            &path,
            &["support-ticket".to_owned(), "support-sla".to_owned()],
        )
        .unwrap()
        .unwrap();
        fs::remove_file(&path).ok();
        let modules = updated.get("modules").and_then(Value::as_array).unwrap();

        assert_eq!(modules.len(), 1);
        assert_eq!(
            modules[0].get("moduleName").and_then(Value::as_str),
            Some("billing")
        );
    }

    #[test]
    fn manifest_url_derives_base_url() {
        let base = derive_remote_base_url(
            None,
            "https://example.com/lenso/module/v1/manifest?debug=1#hash",
        )
        .unwrap();

        assert_eq!(base, "https://example.com/lenso/module/v1");
    }

    #[test]
    fn plan_items_are_unique() {
        let plan = json!({
            "modules": [
                {
                    "consolePackages": [
                        { "packageName": "@vendor/a", "exportName": "aModule" },
                        { "packageName": "@vendor/a", "exportName": "aModule" }
                    ]
                }
            ]
        });

        assert_eq!(
            unique_console_package_plan_items(&plan),
            vec![ConsolePackagePlanItem {
                export_name: "aModule".to_owned(),
                package_name: "@vendor/a".to_owned(),
            }]
        );
    }

    #[test]
    fn console_bundle_specs_use_manifest_bundle_url() {
        let manifest = json!({
            "console": [
                {
                    "package": {
                        "bundleUrl": "console/entry.js",
                        "export": "crmConsoleModule",
                        "hostApi": "1",
                        "name": "@vendor/crm-console",
                        "styles": ["console/entry.css"],
                        "version": "1.2.3"
                    },
                    "required_capabilities": ["crm.read"]
                }
            ],
            "name": "remote-crm"
        });

        let specs = remote_module_console_bundle_specs(
            Path::new("/tmp/host"),
            &manifest,
            "https://module.example.test/lenso/module/v1",
        )
        .unwrap();

        assert_eq!(specs.len(), 1);
        assert_eq!(
            specs[0].bundle_url,
            "https://module.example.test/lenso/module/v1/console/entry.js"
        );
        assert_eq!(specs[0].entry, "/console/extensions/remote-crm/entry.js");
        assert_eq!(
            specs[0].target_path,
            PathBuf::from("/tmp/host/.lenso/console/extensions/remote-crm/entry.js")
        );
        assert_eq!(
            specs[0].styles[0].source_url,
            "https://module.example.test/lenso/module/v1/console/entry.css"
        );
        assert_eq!(
            specs[0].styles[0].entry,
            "/console/extensions/remote-crm/entry.css"
        );
        assert_eq!(
            specs[0].styles[0].target_path,
            PathBuf::from("/tmp/host/.lenso/console/extensions/remote-crm/entry.css")
        );
        assert_eq!(specs[0].required_capabilities, vec!["crm.read"]);
        assert_eq!(specs[0].version.as_deref(), Some("1.2.3"));
    }

    #[test]
    fn runtime_console_bundle_registry_upserts_by_package_export() {
        let registry_path = Path::new("/tmp/missing-console-registry.json");
        let specs = vec![ConsoleBundleSpec {
            bundle_url: "https://module.example.test/entry.js".to_owned(),
            entry: "/console/extensions/crm/entry.js".to_owned(),
            export_name: "crmConsoleModule".to_owned(),
            host_api: "1".to_owned(),
            module_name: "crm".to_owned(),
            package_name: "@vendor/crm-console".to_owned(),
            required_capabilities: vec!["crm.read".to_owned()],
            styles: vec![ConsoleBundleStyleSpec {
                entry: "/console/extensions/crm/entry.css".to_owned(),
                source_url: "https://module.example.test/entry.css".to_owned(),
                target_path: PathBuf::from("/tmp/host/.lenso/console/extensions/crm/entry.css"),
            }],
            target_path: PathBuf::from("/tmp/host/.lenso/console/extensions/crm/entry.js"),
            version: Some("1.0.0".to_owned()),
        }];

        let registry = update_runtime_console_bundle_registry(registry_path, &specs).unwrap();

        assert_eq!(registry["version"], 1);
        assert_eq!(registry["bundles"][0]["moduleName"], "crm");
        assert_eq!(registry["bundles"][0]["packageName"], "@vendor/crm-console");
        assert_eq!(registry["bundles"][0]["exportName"], "crmConsoleModule");
        assert_eq!(
            registry["bundles"][0]["requiredCapabilities"],
            json!(["crm.read"])
        );
        assert_eq!(
            registry["bundles"][0]["styles"],
            json!(["/console/extensions/crm/entry.css"])
        );
    }
}
