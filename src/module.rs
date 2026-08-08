use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use lenso_service::check_contract_artifact_value;
use serde_json::{Map, Value, json};

const SERVICE_CHECK_ARTIFACT_VERSION: &str = "lenso.service-check.v1";
const SERVICE_DIFF_ARTIFACT_VERSION: &str = "lenso.service-diff.v1";
const SERVICE_DOCTOR_ARTIFACT_VERSION: &str = "lenso.service-doctor.v1";

fn machine_result<T>(
    result: Result<T>,
    json_output: bool,
    code: &str,
    next_action: &str,
) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error) if json_output => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "artifactVersion": "lenso.command-error.v1",
                    "code": code,
                    "message": error.to_string(),
                    "nextAction": next_action,
                }))?
            );
            Err(error)
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug, Clone)]
pub struct ServiceModuleInstallOptions {
    pub allow_incompatible: bool,
    pub base_url: Option<String>,
    pub catalog_url: Option<String>,
    pub dry_run: bool,
    pub env_file: Option<PathBuf>,
    pub install_profiles: Vec<String>,
    pub module_services_file: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub run_install_commands: bool,
    pub source: String,
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
pub struct ServiceModuleUninstallOptions {
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
    pub env_file: Option<PathBuf>,
    pub json: bool,
    pub manifest_url: Option<String>,
    pub operation: Option<String>,
    pub ready_timeout_ms: u64,
    pub ready_url: Option<String>,
    pub repo_root: Option<PathBuf>,
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
    pub json: bool,
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
pub struct ServiceEnvListOptions {
    pub json: bool,
    pub repo_root: Option<PathBuf>,
    pub service_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ServiceEnvAddOptions {
    pub environment_name: String,
    pub image: Option<String>,
    pub ingress_host: Option<String>,
    pub json: bool,
    pub kube_context: Option<String>,
    pub manifest_reference: Option<String>,
    pub namespace: Option<String>,
    pub port: Option<u16>,
    pub public_base_url: Option<String>,
    pub release_track: Option<String>,
    pub replicas: Option<u32>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct ServiceEnvRemoveOptions {
    pub dry_run: bool,
    pub environment_name: String,
    pub json: bool,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ServiceEnvVerifyOptions {
    pub environment_name: String,
    pub json: bool,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ServiceDeployExportOptions {
    pub environment_name: String,
    pub image: Option<String>,
    pub ingress_host: Option<String>,
    pub json: bool,
    pub hpa: bool,
    pub namespace: Option<String>,
    pub network_policy: bool,
    pub output_dir: PathBuf,
    pub pdb: bool,
    pub port: Option<u16>,
    pub replicas: Option<u32>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
    pub target: String,
}

#[derive(Debug, Clone)]
pub struct ServiceDeployStatusOptions {
    pub environment_name: String,
    pub from_file: Option<PathBuf>,
    pub json: bool,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
    pub source: String,
    pub write_state: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceDeployWaitOptions {
    pub environment_name: String,
    pub from_file: Option<PathBuf>,
    pub interval_seconds: u64,
    pub json: bool,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
    pub source: String,
    pub timeout_seconds: u64,
    pub write_state: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceReleasePlanOptions {
    pub environment_name: Option<String>,
    pub fail_on: Option<String>,
    pub json: bool,
    pub manifest_reference: String,
    pub output: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
}

#[derive(Debug, Clone)]
pub struct ServiceReleaseCheckOptions {
    pub environment_name: Option<String>,
    pub fail_on: Option<String>,
    pub json: bool,
    pub plan_file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ServiceReleaseApplyOptions {
    pub allow_incompatible: bool,
    pub base_url: Option<String>,
    pub dry_run: bool,
    pub environment_name: Option<String>,
    pub env_file: Option<PathBuf>,
    pub module_services_file: Option<PathBuf>,
    pub plan_file: PathBuf,
    pub repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ServiceReleasePromoteOptions {
    pub fail_on: Option<String>,
    pub from_environment: String,
    pub json: bool,
    pub output: Option<PathBuf>,
    pub repo_root: Option<PathBuf>,
    pub service_name: String,
    pub to_environment: String,
}

#[derive(Debug, Clone)]
pub struct ServiceReleaseRollbackPlanOptions {
    pub environment_name: String,
    pub json: bool,
    pub output: Option<PathBuf>,
    pub release_id: Option<String>,
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
pub struct ModuleCreateOptions {
    pub capability: Option<String>,
    pub dry_run: bool,
    pub icon: Option<String>,
    pub label: Option<String>,
    pub module_id: String,
    pub repo_root: Option<PathBuf>,
    pub route: Option<String>,
    pub surface_name: Option<String>,
    pub with_console: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModuleSource {
    Linked,
    Service,
}

#[derive(Debug, Clone)]
enum CatalogInstallTarget {
    Descriptor {
        descriptor: Value,
        descriptor_reference: String,
        provenance: ManifestProvenance,
    },
    ServiceManifest {
        manifest_reference: String,
    },
}

#[derive(Debug, Clone)]
struct ConsoleUiScaffold {
    capability: String,
    icon: String,
    label: String,
    module_id: String,
    route: String,
    surface_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InstallCommandSpec {
    command: String,
    cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceModuleServiceInstallSpec {
    name: String,
    command: String,
    cwd: Option<String>,
    ready_url: String,
    ready_timeout_ms: u64,
    auto_start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServiceModuleServiceState {
    module_name: String,
    services: Vec<ServiceModuleServiceInstallSpec>,
}

struct ServiceUninstallTarget {
    provider_name: String,
    module_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceModuleServiceDoctorStatus {
    Ready,
    Disabled,
    ManualNotReady,
    NotConfigured,
    NotReady,
    StaleState,
}

impl ServiceModuleServiceDoctorStatus {
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
    artifact_version: String,
    issue_count: usize,
    sources_checked: usize,
    services_checked: usize,
    sources: Vec<ModuleDoctorSourceReport>,
    services: Vec<ModuleDoctorServiceReport>,
    next_actions: Vec<String>,
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

pub const OFFICIAL_MODULE_CATALOG_URL: &str = "https://catalog.lenso.dev/v1/modules.json";
pub const OFFICIAL_MODULE_CATALOG_FALLBACK_URL: &str =
    "https://lenso-catalog.lenso.workers.dev/v1/modules.json";
const MODULE_INSTALL_LEDGER_PATH: &str = ".lenso/module-installs.json";
const SERVICE_RELEASE_LEDGER_PATH: &str = ".lenso/service-releases.json";
const SERVICE_ENVIRONMENTS_PATH: &str = ".lenso/service-environments.json";
const SERVICE_DEPLOYMENTS_PATH: &str = ".lenso/service-deployments.json";
const RUNTIME_CONFIG_DEFAULTS_PATH: &str = ".lenso/runtime-config-defaults.json";
const PROVIDER_PROTOCOL_VERSION: &str = "lenso.provider.v1";
const SUPPORTED_SERVICE_MODULE_FEATURES: &[&str] = &["service.lifecycle", "service.status"];

pub async fn create_module(options: ModuleCreateOptions) -> Result<()> {
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

    let console_surface = if options.with_console {
        Some(ConsoleUiScaffold::from_options(&options, &module_id))
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
        queue_console_ui_artifact(&mut pending_writes, &module_dir, console_surface)?;
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
        println!(
            "Created Module-owned Console UI artifact at {}/console-ui.",
            display_relative(&repo_root, &module_dir)
        );
        println!(
            "- The {} surface uses a console_ui_esm entry and is bound to the Module Release.",
            console_surface.surface_name
        );
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

pub async fn install_module(
    module_reference: &str,
    options: ServiceModuleInstallOptions,
) -> Result<()> {
    let source = parse_module_source(&options.source)?;
    let should_resolve_catalog_name = should_resolve_service_catalog_entry(source)
        && !looks_like_json_reference(module_reference);
    if !should_resolve_catalog_name
        && let Some(loaded) = read_install_descriptor(module_reference).await?
    {
        let descriptor = loaded.value;
        if is_module_release_descriptor(&descriptor) {
            return install_module_descriptor(
                &descriptor,
                module_reference,
                &loaded.provenance,
                options,
            )
            .await;
        }
        if should_resolve_service_catalog_entry(source)
            && let Some(manifest_reference) = catalog_service_manifest_reference(&descriptor)
        {
            return add_service_module(manifest_reference, options).await;
        }
        return install_module_descriptor(
            &descriptor,
            module_reference,
            &loaded.provenance,
            options,
        )
        .await;
    }

    if should_resolve_catalog_name
        && let Some(target) =
            official_catalog_install_target(module_reference, options.catalog_url.as_deref())
                .await?
    {
        return match target {
            CatalogInstallTarget::Descriptor {
                descriptor,
                descriptor_reference,
                provenance,
            } => {
                install_module_descriptor(&descriptor, &descriptor_reference, &provenance, options)
                    .await
            }
            CatalogInstallTarget::ServiceManifest { manifest_reference } => {
                add_service_module(&manifest_reference, options).await
            }
        };
    }

    if should_resolve_catalog_name
        && let Some(loaded) = read_install_descriptor(module_reference).await?
    {
        let descriptor = loaded.value;
        if is_module_release_descriptor(&descriptor) {
            return install_module_descriptor(
                &descriptor,
                module_reference,
                &loaded.provenance,
                options,
            )
            .await;
        }
        return install_module_descriptor(
            &descriptor,
            module_reference,
            &loaded.provenance,
            options,
        )
        .await;
    }

    match source {
        ModuleSource::Service => add_service_module(module_reference, options).await,
        ModuleSource::Linked => install_linked_module(module_reference, options),
    }
}

fn should_resolve_service_catalog_entry(source: ModuleSource) -> bool {
    matches!(source, ModuleSource::Service)
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

fn catalog_entry_is_linked(entry: &Value) -> bool {
    entry
        .get("source")
        .and_then(Value::as_str)
        .is_some_and(|source| source.eq_ignore_ascii_case("linked"))
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

async fn official_catalog_install_target(
    module_name: &str,
    catalog_url: Option<&str>,
) -> Result<Option<CatalogInstallTarget>> {
    if let Some(catalog_url) = catalog_url.map(str::trim).filter(|value| !value.is_empty()) {
        let catalog = read_json_reference_with_provenance(catalog_url)
            .await
            .with_context(|| format!("resolve official module catalog {catalog_url}"))?;
        return catalog_install_target_for_module_with_provenance(
            &catalog.value,
            module_name,
            catalog.provenance,
        );
    }

    official_catalog_install_target_from_urls(
        module_name,
        OFFICIAL_MODULE_CATALOG_URL,
        &[OFFICIAL_MODULE_CATALOG_FALLBACK_URL.to_owned()],
    )
    .await
}

async fn official_catalog_install_target_from_urls(
    module_name: &str,
    primary_url: &str,
    fallback_urls: &[String],
) -> Result<Option<CatalogInstallTarget>> {
    let mut errors = Vec::new();
    for catalog_url in std::iter::once(primary_url).chain(fallback_urls.iter().map(String::as_str))
    {
        let catalog = match read_json_reference_with_provenance(catalog_url).await {
            Ok(catalog) => catalog,
            Err(error) => {
                errors.push(format!("{catalog_url}: {error:#}"));
                continue;
            }
        };
        return catalog_install_target_for_module_with_provenance(
            &catalog.value,
            module_name,
            catalog.provenance,
        );
    }

    let attempted_urls = std::iter::once(primary_url)
        .chain(fallback_urls.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(", ");
    bail!(
        "Failed to resolve official module catalog from {attempted_urls}: {}",
        errors.join("; ")
    );
}

#[cfg(test)]
fn catalog_install_target_for_module(
    catalog: &Value,
    module_name: &str,
) -> Result<Option<CatalogInstallTarget>> {
    catalog_install_target_for_module_with_provenance(
        catalog,
        module_name,
        ManifestProvenance::Builtin,
    )
}

fn catalog_install_target_for_module_with_provenance(
    catalog: &Value,
    module_name: &str,
    provenance: ManifestProvenance,
) -> Result<Option<CatalogInstallTarget>> {
    let modules = catalog
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Module catalog modules must be an array"))?;
    for entry in modules {
        if catalog_entry_is_module_release(entry)
            && entry.get("name").and_then(Value::as_str) == Some(module_name)
        {
            let descriptor_reference = catalog_entry_manifest_reference(entry, module_name);
            return Ok(Some(CatalogInstallTarget::Descriptor {
                descriptor: entry.clone(),
                descriptor_reference,
                provenance: provenance.clone(),
            }));
        }

        if catalog_entry_is_linked(entry)
            && entry.get("name").and_then(Value::as_str) == Some(module_name)
        {
            let descriptor_reference = catalog_entry_manifest_reference(entry, module_name);
            let descriptor = linked_catalog_descriptor(entry, module_name, &descriptor_reference)?;
            return Ok(Some(CatalogInstallTarget::Descriptor {
                descriptor,
                descriptor_reference,
                provenance: provenance.clone(),
            }));
        }

        if let Some(manifest_reference) =
            catalog_service_manifest_reference_for_module(entry, module_name)
        {
            return Ok(Some(CatalogInstallTarget::ServiceManifest {
                manifest_reference: manifest_reference.to_owned(),
            }));
        }
    }
    Ok(None)
}

fn catalog_entry_manifest_reference(entry: &Value, module_name: &str) -> String {
    entry
        .get("manifestReference")
        .or_else(|| entry.get("manifest_reference"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(module_name)
        .to_owned()
}

fn linked_catalog_descriptor(
    entry: &Value,
    module_name: &str,
    descriptor_reference: &str,
) -> Result<Value> {
    if let Some(builtin_module_name) = descriptor_reference
        .strip_prefix("builtin:")
        .or_else(|| descriptor_reference.strip_prefix("linked:"))
        && let Some(descriptor) = builtin_linked_module_descriptor(builtin_module_name)
    {
        return Ok(descriptor);
    }
    if let Some(descriptor) = builtin_linked_module_descriptor(module_name) {
        return Ok(descriptor);
    }
    if entry.get("linked").is_some() {
        return Ok(entry.clone());
    }
    bail!(
        "Official catalog module `{module_name}` points to linked source `{descriptor_reference}`, but this lenso-cli version does not know how to install it"
    );
}

async fn install_module_descriptor(
    descriptor: &Value,
    descriptor_reference: &str,
    provenance: &ManifestProvenance,
    options: ServiceModuleInstallOptions,
) -> Result<()> {
    if is_module_release_descriptor(descriptor) {
        return install_module_release_descriptor(
            descriptor,
            descriptor_reference,
            provenance,
            options,
        )
        .await;
    }
    match parse_module_source(string_field(descriptor, "source")?)? {
        ModuleSource::Service => {
            let manifest_reference = descriptor
                .get("service")
                .and_then(|remote| {
                    remote
                        .get("manifest_url")
                        .or_else(|| remote.get("manifestUrl"))
                })
                .and_then(Value::as_str)
                .unwrap_or(descriptor_reference);
            add_service_module(manifest_reference, options).await
        }
        ModuleSource::Linked => {
            install_linked_module_descriptor(descriptor, descriptor_reference, provenance, options)
                .await
        }
    }
}

async fn install_module_release_descriptor(
    descriptor: &Value,
    descriptor_reference: &str,
    provenance: &ManifestProvenance,
    mut options: ServiceModuleInstallOptions,
) -> Result<()> {
    let release = validate_module_release_descriptor(descriptor.clone())?;
    let source = module_release_source(&release)?;
    if source == "linked" || source == "bundled" {
        if release.get("linked").is_some() {
            return install_linked_module_descriptor(
                &release,
                descriptor_reference,
                provenance,
                options,
            )
            .await;
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
    add_service_module_with_context(&service_reference, options, Some(&release_context)).await
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
        ModuleSource::Service => {
            update_service_module_from_receipt(
                module_name,
                manifest_reference,
                &receipt,
                options,
                repo_root,
            )
            .await
        }
        ModuleSource::Linked => {
            update_linked_module_from_receipt(manifest_reference, options, repo_root).await
        }
    }
}

async fn update_service_module_from_receipt(
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
        return add_service_module(
            manifest_reference,
            ServiceModuleInstallOptions {
                allow_incompatible: options.allow_incompatible,
                base_url: options
                    .base_url
                    .clone()
                    .or_else(|| service_receipt_base_url(receipt)),
                catalog_url: None,
                dry_run: options.dry_run,
                env_file: options.env_file,
                install_profiles: options.install_profiles,
                module_services_file: options.module_services_file,
                repo_root: Some(repo_root),
                run_install_commands: options.run_install_commands,
                source: "service".to_owned(),
            },
        )
        .await;
    }
    let manifest = validate_service_module_manifest(manifest)?;
    let manifest_name = string_field(&manifest, "name")?.trim();
    if manifest_name != module_name {
        bail!("Installed module `{module_name}` update resolved manifest for `{manifest_name}`");
    }

    add_service_module(
        manifest_reference,
        ServiceModuleInstallOptions {
            allow_incompatible: options.allow_incompatible,
            base_url: options.base_url.clone().or_else(|| {
                receipt
                    .get("baseUrl")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            }),
            catalog_url: None,
            dry_run: options.dry_run,
            env_file: options.env_file,
            install_profiles: options.install_profiles,
            module_services_file: options.module_services_file,
            repo_root: Some(repo_root),
            run_install_commands: options.run_install_commands,
            source: "service".to_owned(),
        },
    )
    .await
}

async fn update_linked_module_from_receipt(
    manifest_reference: &str,
    options: ModuleUpdateOptions,
    repo_root: PathBuf,
) -> Result<()> {
    if options.base_url.is_some() {
        bail!("--base-url only applies to service module updates");
    }

    install_module(
        module_update_reference(manifest_reference),
        ServiceModuleInstallOptions {
            allow_incompatible: options.allow_incompatible,
            base_url: None,
            catalog_url: None,
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

pub async fn add_service_module(
    manifest_reference: &str,
    options: ServiceModuleInstallOptions,
) -> Result<()> {
    add_service_module_with_context(manifest_reference, options, None).await
}

async fn add_service_module_with_context(
    manifest_reference: &str,
    options: ServiceModuleInstallOptions,
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
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let module_services_path = resolve_path(
        &repo_root,
        options
            .module_services_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    );
    let loaded = read_json_reference_with_provenance(manifest_reference).await?;
    let manifest = loaded.value;
    if is_service_package_manifest(&manifest) {
        let package = validate_service_package_manifest(manifest)?;
        let service_manifest_reference =
            service_package_manifest_reference(manifest_reference, &package)?;
        let service_loaded =
            read_json_reference_with_provenance(&service_manifest_reference).await?;
        let service_manifest = validate_service_manifest(service_loaded.value)?;
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
            &install_ledger_path,
            &module_services_path,
            None,
            module_release_context,
        )
        .await;
    }
    let manifest = validate_service_module_manifest(manifest)?;
    if let Some(issue) = service_module_manifest_compatibility_issue(&manifest)
        && !options.allow_incompatible
    {
        bail!("{issue}; rerun with --allow-incompatible to record an operator override");
    }
    let module_name = string_field(&manifest, "name")?.trim().to_owned();
    let base_url = derive_remote_base_url(options.base_url.as_deref(), manifest_reference)?;
    let install_env = service_module_install_env(&manifest)?;
    let install_commands = service_module_install_commands(&manifest)?;
    let install_services = service_module_install_services(&manifest, &module_name, &base_url)?;
    let env_file = apply_manifest_install_env(
        update_service_modules_env(&env_file_path, &module_name, &base_url)?,
        &install_env,
    );
    let module_services = update_service_module_services_file(
        &module_services_path,
        &module_name,
        &install_services,
    )?;
    let install_ledger = update_module_install_ledger(
        &install_ledger_path,
        service_module_install_ledger_entry(
            &module_name,
            manifest_reference,
            &base_url,
            &manifest,
            service_module_install_writes(
                &repo_root,
                &env_file_path,
                module_services
                    .as_ref()
                    .map(|_| module_services_path.as_path()),
            ),
            &install_env,
            &install_commands,
            &install_services,
        ),
    )?;

    if options.dry_run {
        println!("Module install dry run:");
        println!("- {}", display_relative(&repo_root, &env_file_path));
        println!("- {}", display_relative(&repo_root, &install_ledger_path));
        if module_services.is_some() {
            println!("- {}", display_relative(&repo_root, &module_services_path));
        }
        println!("- {module_name}={base_url}");
        println!("- install env vars: {}", install_env.len());
        println!("- install commands: {}", install_commands.len());
        println!("- install services: {}", install_services.len());
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
    println!("- {}", display_relative(&repo_root, &install_ledger_path));
    if module_services.is_some() {
        println!("- {}", display_relative(&repo_root, &module_services_path));
    }
    println!("SERVICE_MODULES: {module_name}={base_url}");
    println!("Install env vars: {}", install_env.len());
    println!("Install commands: {}", install_commands.len());
    println!("Install services: {}", install_services.len());

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
    options: &ServiceModuleInstallOptions,
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
        &install_ledger_path,
        &module_services_path,
        package_context,
        module_release_context,
    )
    .await
}

async fn read_service_or_package_manifest(
    reference: &str,
) -> Result<(
    String,
    Value,
    ManifestProvenance,
    Option<ServicePackageInstallContext>,
)> {
    let loaded = read_json_reference_with_provenance(reference).await?;
    let manifest = loaded.value;
    if is_service_package_manifest(&manifest) {
        let package = validate_service_package_manifest(manifest)?;
        let service_manifest_reference = service_package_manifest_reference(reference, &package)?;
        let service_loaded =
            read_json_reference_with_provenance(&service_manifest_reference).await?;
        let service_manifest = validate_service_manifest(service_loaded.value)?;
        ensure_service_package_matches_manifest(&package, &service_manifest)?;
        return Ok((
            service_manifest_reference,
            service_manifest,
            service_loaded.provenance,
            Some(ServicePackageInstallContext {
                manifest: package,
                reference: reference.to_owned(),
            }),
        ));
    }
    Ok((
        reference.to_owned(),
        validate_service_manifest(manifest)?,
        loaded.provenance,
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
    options: &ServiceModuleInstallOptions,
    repo_root: &Path,
    env_file_path: &Path,
    install_ledger_path: &Path,
    module_services_path: &Path,
    package_context: Option<&ServicePackageInstallContext>,
    module_release_context: Option<&ModuleReleaseInstallContext>,
) -> Result<()> {
    if let Some(issue) = service_module_manifest_compatibility_issue(&manifest)
        && !options.allow_incompatible
    {
        bail!("{issue}; rerun with --allow-incompatible to record an operator override");
    }

    let service_name = string_field(&manifest, "name")?.trim().to_owned();
    let base_url = derive_remote_base_url(options.base_url.as_deref(), manifest_reference)?;
    let install_env = service_module_install_env(&manifest)?;
    let install_commands = service_module_install_commands(&manifest)?;
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
        update_service_modules_env(env_file_path, &service_name, &base_url)?,
        &install_env,
    );
    let module_services = update_service_module_services_file(
        module_services_path,
        &service_name,
        &install_services,
    )?;

    let mut install_ledger = read_json_if_exists(install_ledger_path)?
        .unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
    let mut module_names = Vec::new();

    for module_manifest in &module_manifests {
        let module_name = string_field(module_manifest, "name")?.trim().to_owned();
        let module_base_url = service_module_base_url(&base_url, &module_name);
        let previous_manifest_snapshot =
            module_install_ledger_entry_value(&install_ledger, &module_name)
                .and_then(|entry| entry.get("serviceManifestSnapshot").cloned());
        let mut entry = service_module_install_ledger_entry(
            &module_name,
            manifest_reference,
            &module_base_url,
            module_manifest,
            service_module_install_writes(
                repo_root,
                env_file_path,
                module_services.as_ref().map(|_| module_services_path),
            ),
            &install_env,
            &install_commands,
            &install_services,
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
        println!("- {}", display_relative(repo_root, install_ledger_path));
        if module_services.is_some() {
            println!("- {}", display_relative(repo_root, module_services_path));
        }
        println!("- {service_name}={base_url}");
        println!("- provided modules: {}", module_names.join(", "));
        println!("- install env vars: {}", install_env.len());
        println!("- install commands: {}", install_commands.len());
        println!("- install services: {}", install_services.len());
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
    println!("- {}", display_relative(repo_root, install_ledger_path));
    if module_services.is_some() {
        println!("- {}", display_relative(repo_root, module_services_path));
    }
    println!("SERVICE_MODULES: {service_name}={base_url}");
    println!("Provided modules: {}", module_names.join(", "));
    println!("Install env vars: {}", install_env.len());
    println!("Install commands: {}", install_commands.len());
    println!("Install services: {}", install_services.len());

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

fn install_linked_module(module_name: &str, options: ServiceModuleInstallOptions) -> Result<()> {
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
    _provenance: &ManifestProvenance,
    options: ServiceModuleInstallOptions,
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
        if runtime_config_defaults.is_some() {
            println!(
                "- {}",
                display_relative(&repo_root, &runtime_config_defaults_path)
            );
        }
        println!("- {}", display_relative(&repo_root, &install_ledger_path));
        println!("- {module_name}");
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
    if runtime_config_defaults.is_some() {
        println!(
            "- {}",
            display_relative(&repo_root, &runtime_config_defaults_path)
        );
    }
    println!("- {}", display_relative(&repo_root, &install_ledger_path));
    println!("Next steps:");
    println!("- cargo run --bin migrate");
    println!("- restart the API and worker");

    Ok(())
}

pub async fn uninstall_module(
    module_name: &str,
    options: ServiceModuleUninstallOptions,
) -> Result<()> {
    match uninstall_module_source(module_name, &options)? {
        ModuleSource::Service => uninstall_service_module(module_name, options).await,
        ModuleSource::Linked => uninstall_linked_module(module_name, options),
    }
}

fn uninstall_module_source(
    module_name: &str,
    options: &ServiceModuleUninstallOptions,
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
        service_module_install_state_exists(module_name, &env_file_path, &module_services_path)?,
    )
}

pub async fn uninstall_service_module(
    module_name: &str,
    options: ServiceModuleUninstallOptions,
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
    let install_ledger_path = repo_root.join(MODULE_INSTALL_LEDGER_PATH);
    let module_services_path = resolve_path(
        &repo_root,
        options
            .module_services_file
            .as_deref()
            .unwrap_or_else(|| Path::new(".lenso/module-services.json")),
    );
    let target = service_uninstall_target(&install_ledger_path, module_name)?;
    for warning in service_uninstall_dependency_warnings(&install_ledger_path, &target)? {
        eprintln!("warning: {warning}");
    }
    let env_file = remove_service_module_from_env(&env_file_path, &target.provider_name)?;
    let install_ledger =
        remove_module_install_ledger_modules(&install_ledger_path, &target.module_names)?;
    let module_services =
        remove_service_module_services_file_module(&module_services_path, &target.provider_name)?;

    if options.dry_run {
        println!("Service uninstall dry run:");
        if env_file.is_some() {
            println!("- {}", display_relative(&repo_root, &env_file_path));
        }
        if install_ledger.is_some() {
            println!("- {}", display_relative(&repo_root, &install_ledger_path));
        }
        if module_services.is_some() {
            println!("- {}", display_relative(&repo_root, &module_services_path));
        }
        if env_file.is_none() && install_ledger.is_none() && module_services.is_none() {
            println!("- no local install state found");
        }
        return Ok(());
    }

    let changed = env_file.is_some() || install_ledger.is_some() || module_services.is_some();
    if let Some(env_file) = env_file {
        write_file(&env_file_path, env_file.as_bytes())?;
    }
    if let Some(install_ledger) = install_ledger {
        write_json(&install_ledger_path, &install_ledger)?;
    }
    if let Some(module_services) = module_services {
        write_json(&module_services_path, &module_services)?;
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
    let service_modules = service_module_entries_from_env_source(&env_source);
    let service_states = read_service_module_service_states(&module_services_path)?;
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

    for (module_name, base_url) in service_modules.iter().filter(|(module_name, _)| {
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
        } else if let Some(url) = service_module_manifest_url(base_url) {
            let manifest_ready = provider_service_ready_url(&client, &url).await;
            manifest_status = if manifest_ready {
                ModuleDoctorManifestStatus::Reachable
            } else {
                issue_count += 1;
                fix = Some(
                    "start the service or fix SERVICE_MODULES for this manifest URL".to_owned(),
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
        let has_source = service_modules.iter().any(|(name, _)| name == module_name);
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
                fix: Some("install the service or add it to SERVICE_MODULES".to_owned()),
            });
        }
    }

    for state in service_states
        .iter()
        .filter(|state| requested_module.is_none_or(|module_name| state.module_name == module_name))
    {
        let configured = service_modules
            .iter()
            .any(|(module_name, _)| module_name == &state.module_name);
        let enabled = module_enabled_from_env_source(&env_source, &state.module_name);

        for service in &state.services {
            let ready = provider_service_ready_url(&client, &service.ready_url).await;
            let lock_file_path = service_module_service_state_path(
                services_state_dir,
                &state.module_name,
                service,
                "lock",
            );
            let pid_file_path = service_module_service_state_path(
                services_state_dir,
                &state.module_name,
                service,
                "pid",
            );
            let lock_exists = lock_file_path.exists();
            let pid_exists = pid_file_path.exists();
            let status = service_module_service_doctor_status(
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
                fix: service_module_service_doctor_fix(status).map(ToOwned::to_owned),
            });
        }
    }

    let next_actions = sources
        .iter()
        .filter_map(|source| source.fix.clone())
        .chain(services.iter().filter_map(|service| service.fix.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(ModuleDoctorReport {
        artifact_version: SERVICE_DOCTOR_ARTIFACT_VERSION.to_owned(),
        issue_count,
        next_actions,
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
    let states = read_service_module_service_states(&module_services_path)?;
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
    let states = read_service_module_service_states(&module_services_path)?;
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

fn compose_service_export_source(state: &ServiceModuleServiceState) -> String {
    let mut source = "services:\n".to_owned();
    for service in &state.services {
        source.push_str(&compose_service_source(state, service));
    }
    source
}

fn compose_service_source(
    state: &ServiceModuleServiceState,
    service: &ServiceModuleServiceInstallSpec,
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

fn systemd_service_export_source(state: &ServiceModuleServiceState) -> String {
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

fn dockerfile_service_export_source(state: &ServiceModuleServiceState) -> String {
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

fn env_service_export_source(state: &ServiceModuleServiceState, receipt: Option<&Value>) -> String {
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
    let states = read_service_module_service_states(&module_services_path)?;
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
    let states = read_service_module_service_states(&module_services_path)?;
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
    let states = read_service_module_service_states(&module_services_path)?;
    let (module_name, service) =
        find_module_service(&states, &options.module_name, &options.service_name)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .context("build module service HTTP client")?;
    if provider_service_ready_url(&client, &service.ready_url).await {
        println!("{}/{} already ready", module_name, service.name);
        return Ok(());
    }

    let services_state_dir = module_services_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let lock_file_path =
        service_module_service_state_path(services_state_dir, &module_name, &service, "lock");
    let pid_file_path =
        service_module_service_state_path(services_state_dir, &module_name, &service, "pid");
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
    let states = read_service_module_service_states(&module_services_path)?;
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
    let states = read_service_module_service_states(&module_services_path)?;
    let (module_name, service) =
        find_module_service(&states, &options.module_name, &options.service_name)?;
    let services_state_dir = module_services_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let lock_file_path =
        service_module_service_state_path(services_state_dir, &module_name, &service, "lock");
    let pid_file_path =
        service_module_service_state_path(services_state_dir, &module_name, &service, "pid");
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
    states: &[ServiceModuleServiceState],
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
    states: &[ServiceModuleServiceState],
    module_name: &str,
    service_name: &str,
) -> Result<(String, ServiceModuleServiceInstallSpec)> {
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
    service: &ServiceModuleServiceInstallSpec,
) -> Result<ModuleServiceStatusReport> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(800))
        .build()
        .context("build module service HTTP client")?;
    let ready = provider_service_ready_url(&client, &service.ready_url).await;
    let lock_file_path =
        service_module_service_state_path(services_state_dir, module_name, service, "lock");
    let pid_file_path =
        service_module_service_state_path(services_state_dir, module_name, service, "pid");
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
        let (manifest_reference, manifest, _, _) =
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
    let contract_check = match check_contract_artifact_value(&manifest) {
        Ok(check) => check,
        Err(error) if options.json => {
            println!("{}", serde_json::to_string_pretty(&error)?);
            bail!("contract check failed");
        }
        Err(error) => return Err(error.into()),
    };
    let name = string_field(&manifest, "name")?.trim();
    let version = string_field(&manifest, "version")?.trim();
    let modules = manifest
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Service manifest modules must be an array"))?;
    let mut module_names = modules
        .iter()
        .filter_map(|module| module.get("name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    module_names.sort_unstable();
    module_names.dedup();
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
    let config = service_check_config_summary(
        &manifest,
        options.repo_root.as_deref(),
        options.env_file.as_deref(),
    )?;
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
                "artifactVersion": SERVICE_CHECK_ARTIFACT_VERSION,
                "declarations": declarations,
                "manifestReference": manifest_reference,
                "manifestUrl": manifest_url,
                "modules": module_names,
                "config": config,
                "detectedProtocol": contract_check.detected_protocol,
                "artifactKind": contract_check.artifact_kind,
                "semanticKind": contract_check.semantic_kind,
                "providerSemantics": contract_check.provider_semantics,
                "operations": operations,
                "probes": probes,
                "readyUrl": ready_url,
                "service": name,
                "status": "ok",
                "nextActions": [],
                "version": version,
            }))
            .context("serialize service manifest check")?
        );
    } else {
        println!("Service manifest ok: {name} {version}");
        println!(
            "Contract: {} ({})",
            contract_check.detected_protocol,
            contract_check.semantic_kind.as_str()
        );
        println!("Provided modules: {}", module_names.join(", "));
        println!(
            "Declared operations: routes={} actions={} runtime={} events={}",
            declarations["routes"],
            declarations["actions"],
            declarations["runtimeFunctions"],
            declarations["eventHandlers"]
        );
        println!("Service operations: {}", operations.len());
        let required_env = config["requiredEnv"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default();
        if required_env > 0 {
            println!("Required env: {required_env}");
        }
        if let Some(env_file) = config["envFile"].as_str() {
            println!("Env file: {env_file}");
        }
        if let Some(missing_env) = config["missingEnv"].as_array()
            && !missing_env.is_empty()
        {
            let names = missing_env
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            println!("Missing env: {names}");
        }
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
        if provider_service_ready_url(client, ready_url).await {
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
        let status = if provider_service_ready_url(&client, &url).await {
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
    let repo_root = machine_result(
        resolve_repo_root(options.repo_root.as_deref()),
        options.json,
        "service_repo_invalid",
        "Provide a valid Lenso host repository root and rerun the command.",
    )?;
    let receipt = machine_result(
        installed_service_receipt(&repo_root, &options.service_name),
        options.json,
        "service_not_installed",
        "Install the Service or repair its install receipt before diffing.",
    )?;
    let current = machine_result(
        receipt
        .get("serviceManifestSnapshot")
        .ok_or_else(|| {
            anyhow!(
                "Service `{}` has no manifest snapshot; reinstall or upgrade it once before diff",
                options.service_name
            )
        })
        .cloned(),
        options.json,
        "service_snapshot_missing",
        "Reinstall or upgrade the Service once to record a manifest snapshot.",
    )?;
    let (_, candidate, _, _) = machine_result(
        read_service_or_package_manifest(&options.manifest_reference).await,
        options.json,
        "service_candidate_invalid",
        "Fix the candidate Service artifact and rerun the diff.",
    )?;
    machine_result(
        ensure_service_name_matches(&candidate, &options.service_name),
        options.json,
        "service_identity_mismatch",
        "Use a candidate artifact with the installed Service name.",
    )?;
    let diff = service_manifest_diff(&current, &candidate);
    let report = json!({
        "artifactVersion": SERVICE_DIFF_ARTIFACT_VERSION,
        "approvalBoundaries": [{
            "id": "apply-service-upgrade",
            "category": "production_impacting",
            "action": format!("lenso service upgrade {} {}", options.service_name, options.manifest_reference),
            "required": true,
            "executed": false,
            "nextAction": "Obtain explicit operator approval before applying this upgrade."
        }],
        "diff": diff,
        "nextActions": ["Review the deterministic diff before planning or applying an upgrade."],
        "service": options.service_name,
    });

    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_service_manifest_diff(&options.service_name, &report["diff"]);
    }
    Ok(())
}

pub async fn upgrade_service(options: ServiceUpgradeOptions) -> Result<()> {
    if options.dry_run && options.json {
        let (_, candidate, _, package_context) = machine_result(
            read_service_or_package_manifest(&options.manifest_reference).await,
            true,
            "service_candidate_invalid",
            "Fix the candidate Service artifact and rerun the dry-run.",
        )?;
        machine_result(
            ensure_service_name_matches(&candidate, &options.service_name),
            true,
            "service_identity_mismatch",
            "Use a candidate artifact with the installed Service name.",
        )?;
        if let Some(issue) = service_module_manifest_compatibility_issue(&candidate)
            && !options.allow_incompatible
        {
            return machine_result(
                Err(anyhow!(issue)),
                true,
                "service_compatibility_blocked",
                "Resolve the compatibility issue or explicitly pass `--allow-incompatible` after approval.",
            );
        }
        if let Some(package) = package_context.as_ref() {
            machine_result(
                ensure_service_package_matches_manifest(&package.manifest, &candidate),
                true,
                "service_package_mismatch",
                "Regenerate the Service package so its identity and modules match the candidate manifest.",
            )?;
        }
        return diff_service(ServiceDiffOptions {
            json: true,
            manifest_reference: options.manifest_reference,
            repo_root: options.repo_root,
            service_name: options.service_name,
        })
        .await;
    }
    let (manifest_reference, candidate, _provenance, package_context) =
        read_service_or_package_manifest(&options.manifest_reference).await?;
    ensure_service_name_matches(&candidate, &options.service_name)?;
    let install_options = ServiceModuleInstallOptions {
        allow_incompatible: options.allow_incompatible,
        base_url: options.base_url,
        catalog_url: None,
        dry_run: options.dry_run,
        env_file: options.env_file,
        install_profiles: Vec::new(),
        module_services_file: options.module_services_file,
        repo_root: options.repo_root,
        run_install_commands: false,
        source: "service".to_owned(),
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
    let install_options = ServiceModuleInstallOptions {
        allow_incompatible: true,
        base_url: service_receipt_base_url(&receipt),
        catalog_url: None,
        dry_run: options.dry_run,
        env_file: options.env_file,
        install_profiles: Vec::new(),
        module_services_file: options.module_services_file,
        repo_root: options.repo_root,
        run_install_commands: false,
        source: "service".to_owned(),
    };
    add_service_manifest_with_options(&manifest_reference, previous, &install_options, None, None)
        .await
}

pub fn list_service_environments(options: ServiceEnvListOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let path = repo_root.join(SERVICE_ENVIRONMENTS_PATH);
    let file = read_service_environments_file(&path)?;
    let mut environments = file
        .get("environments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(service_name) = options.service_name.as_deref() {
        environments.retain(|environment| {
            environment.get("serviceName").and_then(Value::as_str) == Some(service_name)
        });
    }
    sort_service_environments(&mut environments);

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "version": 1,
                "environments": environments,
            }))?
        );
        return Ok(());
    }

    if environments.is_empty() {
        println!("No service environments configured.");
        return Ok(());
    }

    for environment in environments {
        let service_name = environment
            .get("serviceName")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let name = environment
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let target = environment
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let namespace = environment
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let image = environment
            .get("image")
            .and_then(Value::as_str)
            .unwrap_or("-");
        println!("{service_name}/{name}: target={target} namespace={namespace} image={image}");
    }

    Ok(())
}

pub fn add_service_environment(options: ServiceEnvAddOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let path = repo_root.join(SERVICE_ENVIRONMENTS_PATH);
    let mut file = read_service_environments_file(&path)?;
    let environment = service_environment_value(&options);
    upsert_service_environment(&mut file, environment.clone())?;
    write_json(&path, &file)?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&environment)?);
    } else {
        println!(
            "Configured service environment: {}/{}",
            options.service_name, options.environment_name
        );
        if let Some(manifest_reference) =
            environment.get("manifestReference").and_then(Value::as_str)
        {
            println!("manifest: {manifest_reference}");
        }
    }

    Ok(())
}

pub fn remove_service_environment(options: ServiceEnvRemoveOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let path = repo_root.join(SERVICE_ENVIRONMENTS_PATH);
    let mut file = read_service_environments_file(&path)?;
    let environments = service_environments_array_mut(&mut file)?;
    let before = environments.len();
    environments.retain(|environment| {
        !service_environment_matches(
            environment,
            &options.service_name,
            &options.environment_name,
        )
    });
    let removed = before != environments.len();
    if !removed {
        bail!(
            "Service environment not found: {}/{}",
            options.service_name,
            options.environment_name
        );
    }
    sort_service_environments(environments);

    if !options.dry_run {
        write_json(&path, &file)?;
    }

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "dryRun": options.dry_run,
                "removed": removed,
                "serviceName": options.service_name,
                "environment": options.environment_name,
            }))?
        );
    } else if options.dry_run {
        println!(
            "Would remove service environment: {}/{}",
            options.service_name, options.environment_name
        );
    } else {
        println!(
            "Removed service environment: {}/{}",
            options.service_name, options.environment_name
        );
    }

    Ok(())
}

pub fn verify_service_environment(options: ServiceEnvVerifyOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let path = repo_root.join(SERVICE_ENVIRONMENTS_PATH);
    let file = read_service_environments_file(&path)?;
    let environment = file
        .get("environments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|environment| {
            service_environment_matches(
                environment,
                &options.service_name,
                &options.environment_name,
            )
        })
        .cloned()
        .ok_or_else(|| {
            anyhow!(
                "Service environment not found: {}/{}",
                options.service_name,
                options.environment_name
            )
        })?;

    let checks = service_environment_checks(&repo_root, &environment);
    let ok = checks
        .iter()
        .all(|check| check.get("status").and_then(Value::as_str) == Some("ok"));

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "ok": ok,
                "environment": environment,
                "checks": checks,
            }))?
        );
    } else {
        println!(
            "Service environment: {}/{}",
            options.service_name, options.environment_name
        );
        for check in &checks {
            println!(
                "- [{}] {}{}",
                check.get("status").and_then(Value::as_str).unwrap_or("-"),
                check.get("name").and_then(Value::as_str).unwrap_or("-"),
                check
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(|detail| format!(": {detail}"))
                    .unwrap_or_default()
            );
        }
    }

    if ok {
        Ok(())
    } else {
        bail!("Service environment verify failed")
    }
}

fn read_service_environments_file(path: &Path) -> Result<Value> {
    let mut file =
        read_json_if_exists(path)?.unwrap_or_else(|| json!({ "version": 1, "environments": [] }));
    if !file.is_object() {
        bail!(
            "Service environments file must be a JSON object: {}",
            path.display()
        );
    }
    if !file.get("environments").is_some_and(Value::is_array) {
        file["environments"] = json!([]);
    }
    Ok(file)
}

fn service_environment_value(options: &ServiceEnvAddOptions) -> Value {
    let mut environment = Map::new();
    environment.insert("name".to_owned(), json!(options.environment_name));
    environment.insert("serviceName".to_owned(), json!(options.service_name));
    environment.insert("target".to_owned(), json!(options.target));
    insert_optional_string(&mut environment, "namespace", options.namespace.as_deref());
    insert_optional_string(
        &mut environment,
        "kubeContext",
        options.kube_context.as_deref(),
    );
    insert_optional_string(&mut environment, "image", options.image.as_deref());
    insert_optional_string(
        &mut environment,
        "publicBaseUrl",
        options.public_base_url.as_deref(),
    );
    let manifest_reference = options
        .manifest_reference
        .clone()
        .or_else(|| derived_service_manifest_reference(options.public_base_url.as_deref()));
    insert_optional_string(
        &mut environment,
        "manifestReference",
        manifest_reference.as_deref(),
    );
    environment.insert(
        "releaseTrack".to_owned(),
        json!(
            options
                .release_track
                .as_deref()
                .unwrap_or(&options.environment_name)
        ),
    );

    let mut config = Map::new();
    if let Some(replicas) = options.replicas {
        config.insert("replicas".to_owned(), json!(replicas));
    }
    if let Some(port) = options.port {
        config.insert("port".to_owned(), json!(port));
    }
    insert_optional_string(&mut config, "ingressHost", options.ingress_host.as_deref());
    if !config.is_empty() {
        environment.insert("config".to_owned(), Value::Object(config));
    }

    Value::Object(environment)
}

fn insert_optional_string(map: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        map.insert(key.to_owned(), json!(value));
    }
}

fn derived_service_manifest_reference(public_base_url: Option<&str>) -> Option<String> {
    public_base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("{}/lenso/service/v1/manifest", trim_trailing_slashes(value)))
}

fn upsert_service_environment(file: &mut Value, environment: Value) -> Result<()> {
    let service_name = environment
        .get("serviceName")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("serviceName is required"))?
        .to_owned();
    let name = environment
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("name is required"))?
        .to_owned();
    let environments = service_environments_array_mut(file)?;
    if let Some(existing) = environments
        .iter_mut()
        .find(|candidate| service_environment_matches(candidate, &service_name, &name))
    {
        *existing = environment;
    } else {
        environments.push(environment);
    }
    sort_service_environments(environments);
    Ok(())
}

fn service_environments_array_mut(file: &mut Value) -> Result<&mut Vec<Value>> {
    file.get_mut("environments")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Service environments file environments must be an array"))
}

fn sort_service_environments(environments: &mut [Value]) {
    environments.sort_by(|left, right| {
        service_environment_sort_key(left).cmp(&service_environment_sort_key(right))
    });
}

fn service_environment_sort_key(environment: &Value) -> (String, String) {
    (
        environment
            .get("serviceName")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        environment
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
    )
}

fn service_environment_matches(environment: &Value, service_name: &str, name: &str) -> bool {
    environment.get("serviceName").and_then(Value::as_str) == Some(service_name)
        && environment.get("name").and_then(Value::as_str) == Some(name)
}

fn service_environment_checks(repo_root: &Path, environment: &Value) -> Vec<Value> {
    let service_name = environment
        .get("serviceName")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let mut checks = Vec::new();

    match installed_service_receipt(repo_root, service_name) {
        Ok(_) => checks.push(service_env_check(
            "service_installed",
            "ok",
            "service install receipt found",
        )),
        Err(error) => checks.push(service_env_check(
            "service_installed",
            "error",
            &format!("install the service first ({error})"),
        )),
    }

    let target = environment
        .get("target")
        .and_then(Value::as_str)
        .unwrap_or("");
    if matches!(target, "kubernetes" | "operator") {
        checks.push(service_env_check("target", "ok", target));
        push_required_service_env_check(&mut checks, environment, "namespace");
        push_required_service_env_check(&mut checks, environment, "image");
        if service_environment_manifest_reference(environment).is_some() {
            checks.push(service_env_check(
                "manifest_reference",
                "ok",
                "manifest URL configured or derived",
            ));
        } else {
            checks.push(service_env_check(
                "manifest_reference",
                "error",
                "set --manifest-reference or --public-base-url",
            ));
        }
    } else {
        checks.push(service_env_check(
            "target",
            "error",
            "unsupported target; expected kubernetes or operator",
        ));
    }

    checks
}

fn push_required_service_env_check(checks: &mut Vec<Value>, environment: &Value, field: &str) {
    if environment
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        checks.push(service_env_check(field, "ok", "configured"));
    } else {
        checks.push(service_env_check(field, "error", "missing"));
    }
}

fn service_environment_manifest_reference(environment: &Value) -> Option<String> {
    environment
        .get("manifestReference")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            derived_service_manifest_reference(
                environment.get("publicBaseUrl").and_then(Value::as_str),
            )
        })
}

fn service_env_check(name: &str, status: &str, detail: &str) -> Value {
    json!({
        "name": name,
        "status": status,
        "detail": detail,
    })
}

pub fn export_service_deployment(options: ServiceDeployExportOptions) -> Result<()> {
    match options.target.as_str() {
        "kubernetes" => export_kubernetes_service_deployment(options),
        "operator" => export_operator_service_provider(options),
        other => bail!("Unsupported deployment target `{other}`; expected kubernetes or operator"),
    }
}

fn export_kubernetes_service_deployment(options: ServiceDeployExportOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let environment =
        find_service_environment(&repo_root, &options.service_name, &options.environment_name)?
            .unwrap_or_else(|| {
                json!({
                    "name": options.environment_name,
                    "serviceName": options.service_name,
                    "target": "kubernetes",
                })
            });
    let namespace = options
        .namespace
        .clone()
        .or_else(|| string_at(&environment, "/namespace"))
        .ok_or_else(|| {
            anyhow!("Kubernetes namespace is required; pass --namespace or configure service env")
        })?;
    let image = options
        .image
        .clone()
        .or_else(|| string_at(&environment, "/image"))
        .ok_or_else(|| {
            anyhow!("Kubernetes image is required; pass --image or configure service env")
        })?;
    let port = options
        .port
        .or_else(|| u16_at(&environment, "/config/port"))
        .unwrap_or(4100);
    let replicas = options
        .replicas
        .or_else(|| u32_at(&environment, "/config/replicas"))
        .unwrap_or(1);
    let ingress_host = options
        .ingress_host
        .clone()
        .or_else(|| string_at(&environment, "/config/ingressHost"));
    let include_hpa = options.hpa || bool_at(&environment, "/config/autoscaling").unwrap_or(false);
    let include_pdb =
        options.pdb || bool_at(&environment, "/config/disruptionBudget").unwrap_or(replicas > 1);
    let include_network_policy =
        options.network_policy || bool_at(&environment, "/config/networkPolicy").unwrap_or(false);
    let manifest_reference = service_environment_manifest_reference(&environment);
    let release = latest_service_release_for_env(
        &repo_root,
        &options.service_name,
        &options.environment_name,
    )?;
    let release_id = release
        .as_ref()
        .and_then(|release| release.get("id").and_then(Value::as_str))
        .unwrap_or("pending");
    let service_manifest = installed_service_receipt(&repo_root, &options.service_name)
        .ok()
        .and_then(|receipt| receipt.get("serviceManifestSnapshot").cloned())
        .unwrap_or(Value::Null);
    let modules = if service_manifest.is_null() {
        Vec::new()
    } else {
        service_module_name_set(&service_manifest)
            .into_iter()
            .collect()
    };
    let env_names = service_manifest_env_names(&service_manifest);
    let output_dir = resolve_path(&repo_root, &options.output_dir);
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("create directory {}", output_dir.display()))?;

    let deployment_name = kubernetes_name(&options.service_name);
    let context = KubernetesExportContext {
        deployment_name: &deployment_name,
        env_names: &env_names,
        image: &image,
        ingress_host: ingress_host.as_deref(),
        manifest_reference: manifest_reference.as_deref().unwrap_or(""),
        modules: &modules,
        namespace: &namespace,
        port,
        release_id,
        replicas,
        service_name: &options.service_name,
        environment_name: &options.environment_name,
    };

    write_file(
        &output_dir.join("deployment.yaml"),
        kubernetes_deployment_yaml(&context).as_bytes(),
    )?;
    write_file(
        &output_dir.join("service.yaml"),
        kubernetes_service_yaml(&context).as_bytes(),
    )?;
    write_file(
        &output_dir.join("configmap.yaml"),
        kubernetes_configmap_yaml(&context).as_bytes(),
    )?;
    write_file(
        &output_dir.join("secret.example.yaml"),
        kubernetes_secret_example_yaml(&context).as_bytes(),
    )?;
    if ingress_host.is_some() {
        write_file(
            &output_dir.join("ingress.yaml"),
            kubernetes_ingress_yaml(&context).as_bytes(),
        )?;
    }
    if include_hpa {
        write_file(
            &output_dir.join("hpa.yaml"),
            kubernetes_hpa_yaml(&context).as_bytes(),
        )?;
    }
    if include_pdb {
        write_file(
            &output_dir.join("pdb.yaml"),
            kubernetes_pdb_yaml(&context).as_bytes(),
        )?;
    }
    if include_network_policy {
        write_file(
            &output_dir.join("networkpolicy.yaml"),
            kubernetes_network_policy_yaml(&context).as_bytes(),
        )?;
    }
    write_file(
        &output_dir.join("kustomization.yaml"),
        kubernetes_kustomization_yaml(
            ingress_host.is_some(),
            include_hpa,
            include_pdb,
            include_network_policy,
        )
        .as_bytes(),
    )?;
    write_file(
        &output_dir.join("README.md"),
        kubernetes_export_readme(&context).as_bytes(),
    )?;

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "serviceName": options.service_name,
                "environment": options.environment_name,
                "target": "kubernetes",
                "outputDir": output_dir,
                "files": kubernetes_export_files(
                    ingress_host.is_some(),
                    include_hpa,
                    include_pdb,
                    include_network_policy,
                ),
            }))?
        );
    } else {
        println!(
            "Wrote Kubernetes deployment files: {}",
            output_dir.display()
        );
        println!("next: kubectl apply -k {}", output_dir.display());
        println!(
            "next: lenso service deploy status {} --env {} --write-state",
            options.service_name, options.environment_name
        );
        println!(
            "next: lenso service deploy wait {} --env {} --write-state",
            options.service_name, options.environment_name
        );
    }

    Ok(())
}

fn export_operator_service_provider(options: ServiceDeployExportOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let environment =
        find_service_environment(&repo_root, &options.service_name, &options.environment_name)?
            .unwrap_or_else(|| {
                json!({
                    "name": options.environment_name,
                    "serviceName": options.service_name,
                    "target": "operator",
                })
            });
    let namespace = options
        .namespace
        .clone()
        .or_else(|| string_at(&environment, "/namespace"))
        .ok_or_else(|| {
            anyhow!("Kubernetes namespace is required; pass --namespace or configure service env")
        })?;
    let image = options
        .image
        .clone()
        .or_else(|| string_at(&environment, "/image"))
        .ok_or_else(|| {
            anyhow!("Kubernetes image is required; pass --image or configure service env")
        })?;
    let port = options
        .port
        .or_else(|| u16_at(&environment, "/config/port"))
        .unwrap_or(4100);
    let replicas = options
        .replicas
        .or_else(|| u32_at(&environment, "/config/replicas"))
        .unwrap_or(1);
    let ingress_host = options
        .ingress_host
        .clone()
        .or_else(|| string_at(&environment, "/config/ingressHost"));
    let include_hpa = options.hpa || bool_at(&environment, "/config/autoscaling").unwrap_or(false);
    let include_pdb =
        options.pdb || bool_at(&environment, "/config/disruptionBudget").unwrap_or(replicas > 1);
    let include_network_policy =
        options.network_policy || bool_at(&environment, "/config/networkPolicy").unwrap_or(false);
    let manifest_reference = service_environment_manifest_reference(&environment);
    let release = latest_service_release_for_env(
        &repo_root,
        &options.service_name,
        &options.environment_name,
    )?;
    let release_id = release
        .as_ref()
        .and_then(|release| release.get("id").and_then(Value::as_str))
        .unwrap_or("pending");
    let service_manifest = installed_service_receipt(&repo_root, &options.service_name)
        .ok()
        .and_then(|receipt| receipt.get("serviceManifestSnapshot").cloned())
        .unwrap_or(Value::Null);
    let modules = if service_manifest.is_null() {
        Vec::new()
    } else {
        service_module_name_set(&service_manifest)
            .into_iter()
            .collect()
    };
    let env_names = service_manifest_env_names(&service_manifest);
    let output_dir = resolve_path(&repo_root, &options.output_dir);
    fs::create_dir_all(&output_dir)
        .with_context(|| format!("create directory {}", output_dir.display()))?;

    let deployment_name = kubernetes_name(&options.service_name);
    let context = KubernetesExportContext {
        deployment_name: &deployment_name,
        env_names: &env_names,
        image: &image,
        ingress_host: ingress_host.as_deref(),
        manifest_reference: manifest_reference.as_deref().unwrap_or(""),
        modules: &modules,
        namespace: &namespace,
        port,
        release_id,
        replicas,
        service_name: &options.service_name,
        environment_name: &options.environment_name,
    };

    write_file(
        &output_dir.join("lensoserviceprovider.yaml"),
        operator_provider_cr_yaml(&context, include_hpa, include_pdb, include_network_policy)
            .as_bytes(),
    )?;
    write_file(
        &output_dir.join("kustomization.yaml"),
        b"resources:\n  - lensoserviceprovider.yaml\n",
    )?;
    write_file(
        &output_dir.join("README.md"),
        operator_provider_export_readme(&context).as_bytes(),
    )?;

    if options.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "serviceName": options.service_name,
                "environment": options.environment_name,
                "target": "operator",
                "outputDir": output_dir,
                "files": ["lensoserviceprovider.yaml", "kustomization.yaml", "README.md"],
            }))?
        );
    } else {
        println!("Wrote LensoServiceProvider files: {}", output_dir.display());
        println!("next: kubectl apply -k {}", output_dir.display());
        println!(
            "next: lenso service deploy status {} --env {} --source operator --write-state",
            context.service_name, context.environment_name
        );
        println!(
            "next: lenso service deploy wait {} --env {} --source operator --write-state",
            context.service_name, context.environment_name
        );
    }

    Ok(())
}

pub fn status_service_deployment(options: ServiceDeployStatusOptions) -> Result<()> {
    let (repo_root, observation) = service_deployment_observation_for_options(&options)?;

    if options.write_state {
        upsert_service_deployment_observation(
            &repo_root.join(SERVICE_DEPLOYMENTS_PATH),
            observation.clone(),
        )?;
    }

    print_service_deployment_observation(&options, &observation)?;

    Ok(())
}

pub fn wait_service_deployment(options: ServiceDeployWaitOptions) -> Result<()> {
    let timeout = Duration::from_secs(options.timeout_seconds);
    let interval = Duration::from_secs(options.interval_seconds.max(1));
    let started = Instant::now();

    loop {
        let status_options = ServiceDeployStatusOptions {
            environment_name: options.environment_name.clone(),
            from_file: options.from_file.clone(),
            json: false,
            repo_root: options.repo_root.clone(),
            service_name: options.service_name.clone(),
            source: options.source.clone(),
            write_state: false,
        };
        let (repo_root, observation) = service_deployment_observation_for_options(&status_options)?;

        if options.write_state {
            upsert_service_deployment_observation(
                &repo_root.join(SERVICE_DEPLOYMENTS_PATH),
                observation.clone(),
            )?;
        }

        if service_deployment_wait_ready(&observation) {
            if options.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "status": "ready",
                        "observation": observation,
                    }))?
                );
            } else {
                println!(
                    "Service deployment ready: {}/{}",
                    options.service_name, options.environment_name
                );
                println!(
                    "state: {}",
                    observation
                        .get("state")
                        .and_then(Value::as_str)
                        .unwrap_or("-")
                );
                println!(
                    "drift: {}",
                    observation
                        .get("drift")
                        .and_then(Value::as_str)
                        .unwrap_or("-")
                );
            }
            return Ok(());
        }

        if service_deployment_wait_failed(&observation)
            || options.from_file.is_some()
            || started.elapsed() >= timeout
        {
            let state = observation
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("-");
            let drift = observation
                .get("drift")
                .and_then(Value::as_str)
                .unwrap_or("-");
            let next_action = observation
                .get("nextAction")
                .and_then(Value::as_str)
                .unwrap_or("refresh deployment status");
            bail!(
                "Service deployment is not ready: {}/{} state={state} drift={drift}; next: {next_action}",
                options.service_name,
                options.environment_name
            );
        }

        std::thread::sleep(interval);
    }
}

fn service_deployment_observation_for_options(
    options: &ServiceDeployStatusOptions,
) -> Result<(PathBuf, Value)> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let environment =
        find_service_environment(&repo_root, &options.service_name, &options.environment_name)?
            .ok_or_else(|| {
                anyhow!(
                    "Service environment not found: {}/{}",
                    options.service_name,
                    options.environment_name
                )
            })?;
    let observation = match options.source.as_str() {
        "kubernetes" => {
            let (deployment, service, ingress) =
                if let Some(from_file) = options.from_file.as_deref() {
                    let value = read_json(from_file)?;
                    (
                        value
                            .get("deployment")
                            .cloned()
                            .unwrap_or_else(|| value.clone()),
                        value.get("service").cloned(),
                        value.get("ingress").cloned(),
                    )
                } else {
                    let deployment = kubectl_get_deployment(&environment, &options.service_name)?;
                    let service =
                        kubectl_get_named(&environment, "service", &options.service_name).ok();
                    let ingress = string_at(&environment, "/config/ingressHost").and_then(|_| {
                        kubectl_get_named(&environment, "ingress", &options.service_name).ok()
                    });
                    (deployment, service, ingress)
                };
            service_deployment_observation(
                &repo_root,
                &options.service_name,
                &options.environment_name,
                &environment,
                &deployment,
                service.as_ref(),
                ingress.as_ref(),
            )?
        }
        "operator" => {
            let provider = if let Some(from_file) = options.from_file.as_deref() {
                read_json(from_file)?
            } else {
                kubectl_get_lenso_service_provider(&environment, &options.service_name)?
            };
            operator_service_deployment_observation(
                &repo_root,
                &options.service_name,
                &options.environment_name,
                &environment,
                &provider,
            )?
        }
        other => bail!("Unsupported deployment source `{other}`; expected kubernetes or operator"),
    };
    Ok((repo_root, observation))
}

fn print_service_deployment_observation(
    options: &ServiceDeployStatusOptions,
    observation: &Value,
) -> Result<()> {
    if options.json {
        println!("{}", serde_json::to_string_pretty(observation)?);
        return Ok(());
    }
    let state = observation
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let drift = observation
        .get("drift")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let next_action = observation
        .get("nextAction")
        .and_then(Value::as_str)
        .unwrap_or("-");
    println!(
        "Service deployment: {}/{}",
        options.service_name, options.environment_name
    );
    println!("state: {state}");
    println!("drift: {drift}");
    println!("next action: {next_action}");
    Ok(())
}

struct KubernetesExportContext<'a> {
    deployment_name: &'a str,
    env_names: &'a [String],
    image: &'a str,
    ingress_host: Option<&'a str>,
    manifest_reference: &'a str,
    modules: &'a [String],
    namespace: &'a str,
    port: u16,
    release_id: &'a str,
    replicas: u32,
    service_name: &'a str,
    environment_name: &'a str,
}

fn find_service_environment(
    repo_root: &Path,
    service_name: &str,
    environment_name: &str,
) -> Result<Option<Value>> {
    let path = repo_root.join(SERVICE_ENVIRONMENTS_PATH);
    let file = read_service_environments_file(&path)?;
    Ok(file
        .get("environments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|environment| {
            service_environment_matches(environment, service_name, environment_name)
        })
        .cloned())
}

fn kubernetes_deployment_yaml(context: &KubernetesExportContext<'_>) -> String {
    let labels = kubernetes_labels_yaml(context, 4);
    let pod_labels = kubernetes_labels_yaml(context, 8);
    let env_from = if context.env_names.is_empty() {
        String::new()
    } else {
        format!(
            "          envFrom:\n            - configMapRef:\n                name: {}-config\n            - secretRef:\n                name: {}-secrets\n                optional: true\n",
            context.deployment_name, context.deployment_name
        )
    };
    format!(
        "apiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: {name}\n  namespace: {namespace}\n  labels:\n{labels}  annotations:\n    lenso.dev/modules: {modules}\n    lenso.dev/release-id: {release_id}\n    lenso.dev/manifest-reference: {manifest_reference}\nspec:\n  replicas: {replicas}\n  selector:\n    matchLabels:\n      app.kubernetes.io/name: {name}\n  template:\n    metadata:\n      labels:\n{pod_labels}      annotations:\n        lenso.dev/release-id: {release_id}\n    spec:\n      containers:\n        - name: {name}\n          image: {image}\n          ports:\n            - containerPort: {port}\n          readinessProbe:\n            httpGet:\n              path: /lenso/service/v1/status\n              port: {port}\n          livenessProbe:\n            httpGet:\n              path: /lenso/service/v1/status\n              port: {port}\n{env_from}",
        name = context.deployment_name,
        namespace = context.namespace,
        labels = labels,
        modules = yaml_quote(&context.modules.join(",")),
        release_id = yaml_quote(context.release_id),
        manifest_reference = yaml_quote(context.manifest_reference),
        replicas = context.replicas,
        pod_labels = pod_labels,
        image = context.image,
        port = context.port,
        env_from = env_from
    )
}

fn kubernetes_service_yaml(context: &KubernetesExportContext<'_>) -> String {
    let labels = kubernetes_labels_yaml(context, 4);
    format!(
        "apiVersion: v1\nkind: Service\nmetadata:\n  name: {name}\n  namespace: {namespace}\n  labels:\n{labels}spec:\n  selector:\n    app.kubernetes.io/name: {name}\n  ports:\n    - name: http\n      port: {port}\n      targetPort: {port}\n",
        name = context.deployment_name,
        namespace = context.namespace,
        labels = labels,
        port = context.port
    )
}

fn kubernetes_configmap_yaml(context: &KubernetesExportContext<'_>) -> String {
    let mut data = String::new();
    for name in context.env_names {
        data.push_str(&format!("  {}: \"\"\n", kubernetes_env_key(name)));
    }
    if data.is_empty() {
        data.push_str("  LENSO_SERVICE_ENV: \"production\"\n");
    }
    format!(
        "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {name}-config\n  namespace: {namespace}\ndata:\n{data}",
        name = context.deployment_name,
        namespace = context.namespace,
        data = data
    )
}

fn kubernetes_secret_example_yaml(context: &KubernetesExportContext<'_>) -> String {
    format!(
        "apiVersion: v1\nkind: Secret\nmetadata:\n  name: {name}-secrets\n  namespace: {namespace}\ntype: Opaque\nstringData:\n  SERVICE_TOKEN: replace-me\n",
        name = context.deployment_name,
        namespace = context.namespace
    )
}

fn kubernetes_ingress_yaml(context: &KubernetesExportContext<'_>) -> String {
    let host = context.ingress_host.unwrap_or_default();
    format!(
        "apiVersion: networking.k8s.io/v1\nkind: Ingress\nmetadata:\n  name: {name}\n  namespace: {namespace}\nspec:\n  rules:\n    - host: {host}\n      http:\n        paths:\n          - path: /\n            pathType: Prefix\n            backend:\n              service:\n                name: {name}\n                port:\n                  number: {port}\n",
        name = context.deployment_name,
        namespace = context.namespace,
        host = host,
        port = context.port
    )
}

fn kubernetes_hpa_yaml(context: &KubernetesExportContext<'_>) -> String {
    format!(
        "apiVersion: autoscaling/v2\nkind: HorizontalPodAutoscaler\nmetadata:\n  name: {name}\n  namespace: {namespace}\nspec:\n  scaleTargetRef:\n    apiVersion: apps/v1\n    kind: Deployment\n    name: {name}\n  minReplicas: {replicas}\n  maxReplicas: {max_replicas}\n  metrics:\n    - type: Resource\n      resource:\n        name: cpu\n        target:\n          type: Utilization\n          averageUtilization: 70\n",
        name = context.deployment_name,
        namespace = context.namespace,
        replicas = context.replicas.max(1),
        max_replicas = (context.replicas.max(1) * 3).max(3)
    )
}

fn kubernetes_pdb_yaml(context: &KubernetesExportContext<'_>) -> String {
    format!(
        "apiVersion: policy/v1\nkind: PodDisruptionBudget\nmetadata:\n  name: {name}\n  namespace: {namespace}\nspec:\n  minAvailable: 1\n  selector:\n    matchLabels:\n      app.kubernetes.io/name: {name}\n",
        name = context.deployment_name,
        namespace = context.namespace
    )
}

fn kubernetes_network_policy_yaml(context: &KubernetesExportContext<'_>) -> String {
    format!(
        "apiVersion: networking.k8s.io/v1\nkind: NetworkPolicy\nmetadata:\n  name: {name}\n  namespace: {namespace}\nspec:\n  podSelector:\n    matchLabels:\n      app.kubernetes.io/name: {name}\n  policyTypes:\n    - Ingress\n  ingress:\n    - ports:\n        - protocol: TCP\n          port: {port}\n",
        name = context.deployment_name,
        namespace = context.namespace,
        port = context.port
    )
}

fn kubernetes_kustomization_yaml(
    include_ingress: bool,
    include_hpa: bool,
    include_pdb: bool,
    include_network_policy: bool,
) -> String {
    let mut resources = vec![
        "deployment.yaml",
        "service.yaml",
        "configmap.yaml",
        "secret.example.yaml",
    ];
    if include_ingress {
        resources.push("ingress.yaml");
    }
    if include_hpa {
        resources.push("hpa.yaml");
    }
    if include_pdb {
        resources.push("pdb.yaml");
    }
    if include_network_policy {
        resources.push("networkpolicy.yaml");
    }
    format!(
        "resources:\n{}\n",
        resources
            .into_iter()
            .map(|resource| format!("  - {resource}"))
            .collect::<Vec<_>>()
            .join("\n")
    )
}

fn kubernetes_export_readme(context: &KubernetesExportContext<'_>) -> String {
    format!(
        "# {service} {environment} Kubernetes Deployment\n\n```sh\nkubectl apply -k .\nlenso service deploy status {service} --env {environment} --write-state\nlenso service deploy wait {service} --env {environment} --write-state\n```\n",
        service = context.service_name,
        environment = context.environment_name
    )
}

fn kubernetes_export_files(
    include_ingress: bool,
    include_hpa: bool,
    include_pdb: bool,
    include_network_policy: bool,
) -> Vec<&'static str> {
    let mut files = vec![
        "deployment.yaml",
        "service.yaml",
        "configmap.yaml",
        "secret.example.yaml",
        "kustomization.yaml",
        "README.md",
    ];
    if include_ingress {
        files.push("ingress.yaml");
    }
    if include_hpa {
        files.push("hpa.yaml");
    }
    if include_pdb {
        files.push("pdb.yaml");
    }
    if include_network_policy {
        files.push("networkpolicy.yaml");
    }
    files
}

fn operator_provider_cr_yaml(
    context: &KubernetesExportContext<'_>,
    include_hpa: bool,
    include_pdb: bool,
    include_network_policy: bool,
) -> String {
    let modules = if context.modules.is_empty() {
        "  modules: []\n".to_owned()
    } else {
        format!(
            "  modules:\n{}\n",
            context
                .modules
                .iter()
                .map(|module| format!("    - {module}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };
    let env_from = if context.env_names.is_empty() {
        String::new()
    } else {
        format!(
            "  envFrom:\n    configMap: {}-config\n    secret: {}-secrets\n",
            context.deployment_name, context.deployment_name
        )
    };
    let ingress = context.ingress_host.map_or_else(String::new, |host| {
        format!("  ingress:\n    host: {host}\n")
    });
    let autoscaling = if include_hpa {
        format!(
            "  autoscaling:\n    enabled: true\n    minReplicas: {min}\n    maxReplicas: {max}\n    targetCpuUtilization: 70\n",
            min = context.replicas.max(1),
            max = (context.replicas.max(1) * 3).max(3)
        )
    } else {
        String::new()
    };
    let disruption_budget = if include_pdb {
        "  disruptionBudget:\n    enabled: true\n    minAvailable: 1\n".to_owned()
    } else {
        String::new()
    };
    let network_policy = if include_network_policy {
        "  networkPolicy:\n    enabled: true\n".to_owned()
    } else {
        String::new()
    };

    format!(
        "apiVersion: lenso.dev/v1alpha1\nkind: LensoServiceProvider\nmetadata:\n  name: {name}\n  namespace: {namespace}\n  labels:\n    app.kubernetes.io/part-of: lenso\n    app.kubernetes.io/component: service-provider\n    lenso.dev/service-provider: {service}\n    lenso.dev/environment: {environment}\nspec:\n  serviceName: {service}\n  environment: {environment}\n  image: {image}\n  releaseId: {release_id}\n  manifestReference: {manifest_reference}\n{modules}  replicas: {replicas}\n  port: {port}\n{env_from}{ingress}{autoscaling}{disruption_budget}{network_policy}",
        name = context.deployment_name,
        namespace = context.namespace,
        service = context.service_name,
        environment = context.environment_name,
        image = context.image,
        release_id = context.release_id,
        manifest_reference = context.manifest_reference,
        modules = modules,
        replicas = context.replicas,
        port = context.port,
        env_from = env_from,
        ingress = ingress,
        autoscaling = autoscaling,
        disruption_budget = disruption_budget,
        network_policy = network_policy
    )
}

fn operator_provider_export_readme(context: &KubernetesExportContext<'_>) -> String {
    format!(
        "# {service} {environment} LensoServiceProvider\n\n```sh\nkubectl apply -k .\nlenso service deploy status {service} --env {environment} --source operator --write-state\nlenso service deploy wait {service} --env {environment} --source operator --write-state\n```\n",
        service = context.service_name,
        environment = context.environment_name
    )
}

fn kubernetes_labels_yaml(context: &KubernetesExportContext<'_>, indent: usize) -> String {
    let prefix = " ".repeat(indent);
    format!(
        "{prefix}app.kubernetes.io/name: {name}\n{prefix}app.kubernetes.io/part-of: lenso\n{prefix}app.kubernetes.io/component: service-provider\n{prefix}lenso.dev/service-provider: {service}\n{prefix}lenso.dev/environment: {environment}\n",
        name = context.deployment_name,
        service = context.deployment_name,
        environment = context.environment_name
    )
}

fn kubectl_get_deployment(environment: &Value, service_name: &str) -> Result<Value> {
    kubectl_get_named(environment, "deployment", service_name)
}

fn kubectl_get_lenso_service_provider(environment: &Value, service_name: &str) -> Result<Value> {
    kubectl_get_named(environment, "lensoserviceprovider", service_name)
}

fn kubectl_get_named(environment: &Value, kind: &str, service_name: &str) -> Result<Value> {
    let namespace = string_at(environment, "/namespace")
        .ok_or_else(|| anyhow!("Kubernetes namespace is required for deploy status"))?;
    let name = kubernetes_name(service_name);
    let mut command = Command::new("kubectl");
    if let Some(context) = string_at(environment, "/kubeContext") {
        command.args(["--context", &context]);
    }
    command.args(["get", kind, &name, "-n", &namespace, "-o", "json"]);
    let output = command
        .output()
        .with_context(|| format!("run kubectl get {kind}"))?;
    if !output.status.success() {
        bail!(
            "kubectl get {kind} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    serde_json::from_slice(&output.stdout).with_context(|| format!("parse kubectl {kind} JSON"))
}

fn service_deployment_observation(
    repo_root: &Path,
    service_name: &str,
    environment_name: &str,
    environment: &Value,
    deployment: &Value,
    service: Option<&Value>,
    ingress: Option<&Value>,
) -> Result<Value> {
    let latest_release = latest_service_release_for_env(repo_root, service_name, environment_name)?;
    let host_release_id = latest_release
        .as_ref()
        .and_then(|release| release.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned);
    let host_candidate_version = latest_release.as_ref().and_then(|release| {
        release
            .pointer("/candidate/version")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    });
    let cluster_release_id = deployment
        .pointer("/metadata/annotations/lenso.dev~1release-id")
        .and_then(Value::as_str)
        .filter(|value| *value != "pending")
        .map(ToOwned::to_owned);
    let observed_image = deployment_container_image(deployment);
    let expected_image = string_at(environment, "/image");
    let state = kubernetes_deployment_state(deployment);
    let drift = service_deployment_drift(
        host_release_id.as_deref(),
        cluster_release_id.as_deref(),
        expected_image.as_deref(),
        observed_image.as_deref(),
    );
    let namespace = deployment
        .pointer("/metadata/namespace")
        .and_then(Value::as_str)
        .or_else(|| environment.get("namespace").and_then(Value::as_str))
        .unwrap_or("default");
    let deployment_name = deployment
        .pointer("/metadata/name")
        .and_then(Value::as_str)
        .unwrap_or(service_name);
    let desired_replicas = u32_at(deployment, "/spec/replicas");
    let ready_replicas = u32_at(deployment, "/status/readyReplicas");
    let available_replicas = u32_at(deployment, "/status/availableReplicas");
    let manifest_reference = deployment
        .pointer("/metadata/annotations/lenso.dev~1manifest-reference")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| service_environment_manifest_reference(environment));
    let service_endpoint = service
        .and_then(|service| service.pointer("/spec/clusterIP").and_then(Value::as_str))
        .filter(|cluster_ip| *cluster_ip != "None")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{deployment_name}.{namespace}.svc.cluster.local"));
    let ingress_host = ingress
        .and_then(first_ingress_host)
        .or_else(|| string_at(environment, "/config/ingressHost"));
    let next_action = service_deployment_next_action(&state, &drift);

    Ok(json!({
        "serviceName": service_name,
        "environment": environment_name,
        "target": "kubernetes",
        "observedAtUnixMs": current_time_millis()?,
        "state": state,
        "drift": drift,
        "cluster": {
            "namespace": namespace,
            "deployment": deployment_name,
            "readyReplicas": ready_replicas,
            "desiredReplicas": desired_replicas,
            "availableReplicas": available_replicas,
            "image": observed_image,
            "releaseId": cluster_release_id,
            "manifestReference": manifest_reference,
            "serviceEndpoint": service_endpoint,
            "ingressHost": ingress_host,
        },
        "host": {
            "releaseId": host_release_id,
            "candidateVersion": host_candidate_version,
        },
        "checks": [
            {
                "name": "deployment_rollout",
                "status": if state == "ready" { "ok" } else { "attention" },
                "detail": format!(
                    "{}/{} replicas ready",
                    ready_replicas.unwrap_or(0),
                    desired_replicas.unwrap_or(0)
                ),
            }
        ],
        "nextAction": next_action,
    }))
}

fn operator_service_deployment_observation(
    repo_root: &Path,
    service_name: &str,
    environment_name: &str,
    environment: &Value,
    provider: &Value,
) -> Result<Value> {
    let latest_release = latest_service_release_for_env(repo_root, service_name, environment_name)?;
    let host_release_id = latest_release
        .as_ref()
        .and_then(|release| release.get("id").and_then(Value::as_str))
        .map(ToOwned::to_owned);
    let host_candidate_version = latest_release.as_ref().and_then(|release| {
        release
            .pointer("/candidate/version")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
    });
    let status = provider.get("status").unwrap_or(&Value::Null);
    let state = status
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let observed_release_id = status
        .get("observedReleaseId")
        .and_then(Value::as_str)
        .filter(|value| *value != "pending")
        .map(ToOwned::to_owned);
    let observed_image = status
        .get("observedImage")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let expected_image = string_at(environment, "/image");
    let drift = service_deployment_drift(
        host_release_id.as_deref(),
        observed_release_id.as_deref(),
        expected_image.as_deref(),
        observed_image.as_deref(),
    );
    let namespace = provider
        .pointer("/metadata/namespace")
        .and_then(Value::as_str)
        .or_else(|| environment.get("namespace").and_then(Value::as_str))
        .unwrap_or("default");
    let resource = provider
        .pointer("/metadata/name")
        .and_then(Value::as_str)
        .unwrap_or(service_name);
    let ready_replicas = u32_at(status, "/readyReplicas");
    let desired_replicas = u32_at(status, "/desiredReplicas");
    let available_replicas = u32_at(status, "/availableReplicas");
    let manifest_reference = status
        .get("manifestReference")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| service_environment_manifest_reference(environment));
    let conditions = status
        .get("conditions")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let next_action = operator_deployment_next_action(&state, &drift);

    Ok(json!({
        "serviceName": service_name,
        "environment": environment_name,
        "target": "operator",
        "observedAtUnixMs": current_time_millis()?,
        "state": state,
        "drift": drift,
        "operator": {
            "resource": resource,
            "namespace": namespace,
            "observedGeneration": status.get("observedGeneration").and_then(Value::as_u64),
            "conditions": conditions,
        },
        "cluster": {
            "namespace": namespace,
            "deployment": resource,
            "readyReplicas": ready_replicas,
            "desiredReplicas": desired_replicas,
            "availableReplicas": available_replicas,
            "image": observed_image,
            "releaseId": observed_release_id,
            "manifestReference": manifest_reference,
        },
        "host": {
            "releaseId": host_release_id,
            "candidateVersion": host_candidate_version,
        },
        "checks": [
            {
                "name": "operator_reconcile",
                "status": if state == "ready" { "ok" } else { "attention" },
                "detail": format!(
                    "LensoServiceProvider/{resource} is {state}"
                ),
            }
        ],
        "nextAction": next_action,
    }))
}

fn kubernetes_deployment_state(deployment: &Value) -> String {
    if deployment_failed(deployment) {
        return "failed".to_owned();
    }
    let desired = u32_at(deployment, "/spec/replicas")
        .or_else(|| u32_at(deployment, "/status/replicas"))
        .unwrap_or(1);
    let ready = u32_at(deployment, "/status/readyReplicas").unwrap_or(0);
    if desired > 0 && ready == desired {
        "ready".to_owned()
    } else {
        "progressing".to_owned()
    }
}

fn deployment_failed(deployment: &Value) -> bool {
    deployment
        .pointer("/status/conditions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(|condition| {
            condition.get("type").and_then(Value::as_str) == Some("Progressing")
                && condition.get("status").and_then(Value::as_str) == Some("False")
                && condition.get("reason").and_then(Value::as_str)
                    == Some("ProgressDeadlineExceeded")
        })
}

fn service_deployment_drift(
    host_release_id: Option<&str>,
    cluster_release_id: Option<&str>,
    expected_image: Option<&str>,
    observed_image: Option<&str>,
) -> String {
    if let (Some(expected), Some(observed)) = (expected_image, observed_image)
        && expected != observed
    {
        return "image_drift".to_owned();
    }
    match (host_release_id, cluster_release_id) {
        (Some(host), Some(cluster)) if host == cluster => "in_sync".to_owned(),
        (Some(_), Some(_)) | (Some(_), None) => "host_ahead".to_owned(),
        (None, Some(_)) => "cluster_ahead".to_owned(),
        (None, None) => "unknown".to_owned(),
    }
}

fn service_deployment_next_action(state: &str, drift: &str) -> &'static str {
    match (state, drift) {
        ("ready", "in_sync") => "monitor rollout and Remote Calls",
        (_, "image_drift") => "export and apply manifests with the expected image",
        (_, "host_ahead") => "apply the Kubernetes manifests or refresh deployment status",
        (_, "cluster_ahead") => "check release ledger before promoting",
        ("failed", _) => "inspect Kubernetes rollout and pod events",
        ("progressing", _) => "wait for rollout or inspect Kubernetes deployment",
        _ => "refresh deployment status",
    }
}

fn operator_deployment_next_action(state: &str, drift: &str) -> &'static str {
    match (state, drift) {
        ("ready", "in_sync") => "monitor operator conditions, Remote Calls, and Runtime Story",
        (_, "image_drift") => "update the LensoServiceProvider image or release environment",
        (_, "host_ahead") => "wait for the operator to observe the latest release",
        (_, "cluster_ahead") => "check release ledger before promoting",
        ("failed", _) => "inspect LensoServiceProvider conditions and provider pods",
        ("progressing", _) => "wait for operator reconciliation or inspect provider pods",
        _ => "refresh operator status",
    }
}

fn service_deployment_wait_ready(observation: &Value) -> bool {
    observation.get("state").and_then(Value::as_str) == Some("ready")
        && matches!(
            observation.get("drift").and_then(Value::as_str),
            Some("in_sync") | Some("unknown") | None
        )
}

fn service_deployment_wait_failed(observation: &Value) -> bool {
    matches!(
        observation.get("state").and_then(Value::as_str),
        Some("failed") | Some("unhealthy")
    )
}

fn deployment_container_image(deployment: &Value) -> Option<String> {
    deployment
        .pointer("/spec/template/spec/containers")
        .and_then(Value::as_array)
        .and_then(|containers| containers.first())
        .and_then(|container| container.get("image"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn first_ingress_host(ingress: &Value) -> Option<String> {
    ingress
        .pointer("/spec/rules")
        .and_then(Value::as_array)
        .and_then(|rules| rules.first())
        .and_then(|rule| rule.get("host"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn upsert_service_deployment_observation(path: &Path, observation: Value) -> Result<()> {
    let mut file = read_json_if_exists(path)?
        .unwrap_or_else(|| json!({ "version": 2, "observations": [], "history": [] }));
    file["version"] = json!(2);
    if !file.get("observations").is_some_and(Value::is_array) {
        file["observations"] = json!([]);
    }
    if !file.get("history").is_some_and(Value::is_array) {
        file["history"] = json!([]);
    }
    let service_name = observation
        .get("serviceName")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("deployment observation serviceName is required"))?
        .to_owned();
    let environment_name = observation
        .get("environment")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("deployment observation environment is required"))?
        .to_owned();
    let observations = file
        .get_mut("observations")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("service deployment observations must be an array"))?;
    observations.retain(|candidate| {
        candidate.get("serviceName").and_then(Value::as_str) != Some(service_name.as_str())
            || candidate.get("environment").and_then(Value::as_str)
                != Some(environment_name.as_str())
    });
    observations.push(observation.clone());
    observations.sort_by(|left, right| {
        (
            left.get("serviceName")
                .and_then(Value::as_str)
                .unwrap_or(""),
            left.get("environment")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
            .cmp(&(
                right
                    .get("serviceName")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                right
                    .get("environment")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ))
    });
    let history = file
        .get_mut("history")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("service deployment history must be an array"))?;
    history.push(observation);
    history.sort_by_key(|entry| {
        (
            entry
                .get("observedAtUnixMs")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            entry
                .get("serviceName")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            entry
                .get("environment")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
        )
    });
    write_json(path, &file)
}

fn latest_service_release_for_env(
    repo_root: &Path,
    service_name: &str,
    environment_name: &str,
) -> Result<Option<Value>> {
    Ok(
        service_releases_for_env(repo_root, service_name, environment_name)?
            .into_iter()
            .next(),
    )
}

fn rollback_service_release_target(
    repo_root: &Path,
    service_name: &str,
    environment_name: &str,
    release_id: Option<&str>,
) -> Result<Value> {
    let releases = service_releases_for_env(repo_root, service_name, environment_name)?;
    if let Some(release_id) = release_id {
        return releases
            .into_iter()
            .find(|release| release.get("id").and_then(Value::as_str) == Some(release_id))
            .ok_or_else(|| {
                anyhow!("Release `{release_id}` not found for {service_name}/{environment_name}")
            });
    }
    releases
        .into_iter()
        .nth(1)
        .ok_or_else(|| anyhow!("No previous release found for {service_name}/{environment_name}"))
}

fn service_releases_for_env(
    repo_root: &Path,
    service_name: &str,
    environment_name: &str,
) -> Result<Vec<Value>> {
    let Some(ledger) = read_json_if_exists(&repo_root.join(SERVICE_RELEASE_LEDGER_PATH))? else {
        return Ok(Vec::new());
    };
    let mut releases = ledger
        .get("releases")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|release| release.get("serviceName").and_then(Value::as_str) == Some(service_name))
        .filter(|release| {
            release.pointer("/environment/name").and_then(Value::as_str) == Some(environment_name)
        })
        .cloned()
        .collect::<Vec<_>>();
    releases.sort_by_key(|release| {
        std::cmp::Reverse(
            release
                .get("appliedAtUnixMs")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        )
    });
    Ok(releases)
}

fn service_release_candidate_reference(release: &Value) -> Result<String> {
    release
        .pointer("/candidate/inputReference")
        .or_else(|| release.pointer("/candidate/packageReference"))
        .or_else(|| release.pointer("/candidate/manifestReference"))
        .and_then(Value::as_str)
        .filter(|reference| !reference.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("Release has no candidate manifest or package reference"))
}

fn write_or_print_release_plan(
    plan: &Value,
    output: Option<&Path>,
    json: bool,
    message: &str,
) -> Result<()> {
    if let Some(output) = output {
        write_json(output, plan)?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(plan)?);
    } else {
        print_service_release_plan(plan);
        if let Some(output) = output {
            println!("{message}: {}", output.display());
        }
    }
    Ok(())
}

fn service_manifest_env_names(manifest: &Value) -> Vec<String> {
    manifest
        .get("env")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|field| field.get("name").and_then(Value::as_str))
        .map(ToOwned::to_owned)
        .collect()
}

fn string_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn u32_at(value: &Value, pointer: &str) -> Option<u32> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn u16_at(value: &Value, pointer: &str) -> Option<u16> {
    value
        .pointer(pointer)
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
}

fn bool_at(value: &Value, pointer: &str) -> Option<bool> {
    value.pointer(pointer).and_then(Value::as_bool)
}

fn kubernetes_name(value: &str) -> String {
    let mut name = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while name.contains("--") {
        name = name.replace("--", "-");
    }
    name.trim_matches('-').to_owned()
}

fn kubernetes_env_key(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub async fn plan_service_release(options: ServiceReleasePlanOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let mut plan = build_service_release_plan(
        &repo_root,
        &options.service_name,
        &options.manifest_reference,
    )
    .await?;
    if let Some(environment_name) = options.environment_name.as_deref() {
        attach_service_release_environment(
            &repo_root,
            &mut plan,
            &options.service_name,
            environment_name,
        )?;
    }
    let policy = service_release_policy_from_plan(&plan)?;
    enforce_service_release_fail_on(&policy, options.fail_on.as_deref())?;
    plan["policy"] = policy;

    if let Some(output) = &options.output {
        write_json(output, &plan)?;
    }

    if options.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_service_release_plan(&plan);
        if let Some(output) = &options.output {
            println!("Wrote release plan: {}", output.display());
        }
    }

    Ok(())
}

pub fn check_service_release_plan(options: ServiceReleaseCheckOptions) -> Result<()> {
    let mut plan = read_json(&options.plan_file)?;
    validate_service_release_plan(&plan)?;
    validate_service_release_plan_environment(&plan, options.environment_name.as_deref())?;
    let policy = service_release_policy_from_plan(&plan)?;
    enforce_service_release_fail_on(&policy, options.fail_on.as_deref())?;
    plan["policy"] = policy;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_service_release_plan(&plan);
    }

    Ok(())
}

pub fn policy_check_service_release_plan(options: ServiceReleaseCheckOptions) -> Result<()> {
    let plan = read_json(&options.plan_file)?;
    validate_service_release_plan(&plan)?;
    validate_service_release_plan_environment(&plan, options.environment_name.as_deref())?;
    let policy = service_release_policy_from_plan(&plan)?;
    enforce_service_release_fail_on(&policy, options.fail_on.as_deref())?;

    if options.json {
        println!("{}", serde_json::to_string_pretty(&policy)?);
    } else {
        print_service_release_policy(&plan, &policy);
    }

    Ok(())
}

pub async fn apply_service_release_plan(options: ServiceReleaseApplyOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let plan = read_json(&options.plan_file)?;
    validate_service_release_plan(&plan)?;
    validate_service_release_plan_environment(&plan, options.environment_name.as_deref())?;
    let policy = service_release_policy_from_plan(&plan)?;
    if service_release_risk_rank(policy_risk(&policy)?) >= service_release_risk_rank("blocked") {
        bail!(
            "Service release policy risk is blocked; run `lenso service policy check` for details"
        );
    }
    let service_name = plan
        .pointer("/service/name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Service release plan service.name is required"))?
        .to_owned();
    let manifest_reference = plan
        .pointer("/candidate/inputReference")
        .or_else(|| plan.pointer("/candidate/manifestReference"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Service release plan candidate manifest reference is required"))?
        .to_owned();

    upgrade_service(ServiceUpgradeOptions {
        allow_incompatible: options.allow_incompatible,
        base_url: options.base_url,
        dry_run: options.dry_run,
        env_file: options.env_file,
        json: false,
        manifest_reference,
        module_services_file: options.module_services_file,
        repo_root: Some(repo_root.clone()),
        service_name,
    })
    .await?;

    if options.dry_run {
        println!("Service release apply dry run; release ledger not updated.");
    } else {
        append_service_release_ledger(&repo_root, &plan, &policy)?;
    }

    Ok(())
}

pub async fn promote_service_release(options: ServiceReleasePromoteOptions) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let release = latest_service_release_for_env(
        &repo_root,
        &options.service_name,
        &options.from_environment,
    )?
    .ok_or_else(|| {
        anyhow!(
            "No applied release found for {}/{}",
            options.service_name,
            options.from_environment
        )
    })?;
    let candidate_reference = service_release_candidate_reference(&release)?;
    let mut plan =
        build_service_release_plan(&repo_root, &options.service_name, &candidate_reference).await?;
    attach_service_release_environment(
        &repo_root,
        &mut plan,
        &options.service_name,
        &options.to_environment,
    )?;
    plan["promotion"] = json!({
        "from": options.from_environment,
        "to": options.to_environment,
        "sourceReleaseId": release.get("id").cloned().unwrap_or(Value::Null),
    });
    let policy = service_release_policy_from_plan(&plan)?;
    enforce_service_release_fail_on(&policy, options.fail_on.as_deref())?;
    plan["policy"] = policy;
    write_or_print_release_plan(
        &plan,
        options.output.as_deref(),
        options.json,
        "Wrote promotion release plan",
    )
}

pub async fn plan_service_release_rollback(
    options: ServiceReleaseRollbackPlanOptions,
) -> Result<()> {
    let repo_root = resolve_repo_root(options.repo_root.as_deref())?;
    let release = rollback_service_release_target(
        &repo_root,
        &options.service_name,
        &options.environment_name,
        options.release_id.as_deref(),
    )?;
    let candidate_reference = service_release_candidate_reference(&release)?;
    let mut plan =
        build_service_release_plan(&repo_root, &options.service_name, &candidate_reference).await?;
    attach_service_release_environment(
        &repo_root,
        &mut plan,
        &options.service_name,
        &options.environment_name,
    )?;
    plan["rollback"] = json!({
        "environment": options.environment_name,
        "targetReleaseId": release.get("id").cloned().unwrap_or(Value::Null),
    });
    let policy = service_release_policy_from_plan(&plan)?;
    plan["policy"] = policy;
    write_or_print_release_plan(
        &plan,
        options.output.as_deref(),
        options.json,
        "Wrote rollback release plan",
    )
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

async fn build_service_release_plan(
    repo_root: &Path,
    service_name: &str,
    candidate_reference: &str,
) -> Result<Value> {
    let receipt = installed_service_receipt(repo_root, service_name)?;
    let current = receipt
        .get("serviceManifestSnapshot")
        .ok_or_else(|| {
            anyhow!(
                "Service `{service_name}` has no manifest snapshot; reinstall or upgrade it once before planning a release"
            )
        })?
        .clone();
    let (manifest_reference, candidate, _, package_context) =
        read_service_or_package_manifest(candidate_reference).await?;
    ensure_service_name_matches(&candidate, service_name)?;
    let diff = service_manifest_diff(&current, &candidate);
    let compatibility_issue = service_module_manifest_compatibility_issue(&candidate);
    let package_reference = package_context
        .as_ref()
        .map(|package| package.reference.clone());
    let package_snapshot = package_context
        .as_ref()
        .map(|package| package.manifest.clone())
        .unwrap_or(Value::Null);
    let mut plan = json!({
        "artifactVersion": "lenso.service-release-plan.v1",
        "protocol": "lenso.service-release-plan.v1",
        "approvalBoundaries": [{
            "id": "apply-service-release",
            "category": "production_impacting",
            "action": format!("lenso service release apply <plan.json>"),
            "required": true,
            "executed": false,
            "nextAction": "Obtain explicit operator approval before applying this release."
        }],
        "service": {
            "name": service_name,
        },
        "current": service_release_manifest_summary(
            &current,
            receipt_manifest_reference(&receipt).as_deref(),
            receipt.get("servicePackage").and_then(|package| package.get("manifestReference")).and_then(Value::as_str),
        ),
        "candidate": service_release_manifest_summary(
            &candidate,
            Some(&manifest_reference),
            package_reference.as_deref(),
        ),
        "diff": diff,
        "restartRequired": service_release_restart_required(&diff),
    });
    plan["candidate"]["inputReference"] = json!(candidate_reference);
    plan["candidate"]["compatibilityIssue"] = compatibility_issue
        .map(Value::String)
        .unwrap_or(Value::Null);
    if package_snapshot != Value::Null {
        plan["candidate"]["packageSnapshot"] = package_snapshot;
    }
    let policy = service_release_policy_from_plan(&plan)?;
    plan["policy"] = policy.clone();
    plan["nextAction"] = json!(service_release_next_action(policy_risk(&policy)?));
    Ok(plan)
}

fn validate_service_release_plan(plan: &Value) -> Result<()> {
    let protocol = plan
        .get("protocol")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Service release plan protocol is required"))?;
    if protocol != "lenso.service-release-plan.v1" {
        bail!("Unsupported service release plan protocol: {protocol}");
    }
    plan.pointer("/service/name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Service release plan service.name is required"))?;
    plan.pointer("/candidate/manifestReference")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Service release plan candidate.manifestReference is required"))?;
    plan.get("diff")
        .ok_or_else(|| anyhow!("Service release plan diff is required"))?;
    Ok(())
}

fn attach_service_release_environment(
    repo_root: &Path,
    plan: &mut Value,
    service_name: &str,
    environment_name: &str,
) -> Result<()> {
    let environment = find_service_environment(repo_root, service_name, environment_name)?
        .ok_or_else(|| {
            anyhow!("Service environment not found: {service_name}/{environment_name}")
        })?;
    plan["environment"] = json!({
        "name": environment_name,
        "target": environment.get("target").cloned().unwrap_or_else(|| json!("kubernetes")),
        "namespace": environment.get("namespace").cloned().unwrap_or(Value::Null),
        "image": environment.get("image").cloned().unwrap_or(Value::Null),
        "manifestReference": service_environment_manifest_reference(&environment),
    });
    Ok(())
}

fn validate_service_release_plan_environment(plan: &Value, expected: Option<&str>) -> Result<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let actual = plan
        .pointer("/environment/name")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            anyhow!("Service release plan has no environment; rerun `lenso service release plan --env {expected}`")
        })?;
    if actual != expected {
        bail!("Service release plan environment is `{actual}`, expected `{expected}`");
    }
    Ok(())
}

fn service_release_manifest_summary(
    manifest: &Value,
    manifest_reference: Option<&str>,
    package_reference: Option<&str>,
) -> Value {
    let mut summary = json!({
        "manifestReference": manifest_reference.unwrap_or(""),
        "name": manifest.get("name").and_then(Value::as_str).unwrap_or(""),
        "version": manifest.get("version").and_then(Value::as_str).unwrap_or(""),
        "modules": service_module_name_set(manifest).into_iter().collect::<Vec<_>>(),
    });
    if let Some(package_reference) = package_reference {
        summary["packageReference"] = json!(package_reference);
    }
    if let Some(compatibility) = manifest.get("compatibility") {
        summary["compatibility"] = compatibility.clone();
    }
    summary
}

fn receipt_manifest_reference(receipt: &Value) -> Option<String> {
    receipt
        .get("service")
        .and_then(|service| service.get("manifestReference"))
        .or_else(|| receipt.get("manifestReference"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn service_release_policy_from_plan(plan: &Value) -> Result<Value> {
    validate_service_release_plan(plan)?;
    let diff = plan
        .get("diff")
        .ok_or_else(|| anyhow!("Service release plan diff is required"))?;
    let compatibility_issue = plan
        .pointer("/candidate/compatibilityIssue")
        .and_then(Value::as_str);
    Ok(service_release_policy_from_diff(diff, compatibility_issue))
}

fn service_release_policy_from_diff(diff: &Value, compatibility_issue: Option<&str>) -> Value {
    let mut issues = Vec::new();

    if let Some(issue) = compatibility_issue {
        push_release_issue(
            &mut issues,
            "blocked",
            "host_incompatible",
            issue.to_owned(),
        );
    } else if diff
        .get("compatibilityChanged")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        push_release_issue(
            &mut issues,
            "needs_attention",
            "compatibility_changed",
            "Service compatibility metadata changed; review host support before applying."
                .to_owned(),
        );
    }

    for module in json_string_list(&diff["modules"]["removed"]) {
        push_release_issue(
            &mut issues,
            "breaking",
            "module_removed",
            format!("Module `{module}` is removed by this release."),
        );
    }

    for env in json_string_list(&diff["env"]["added"]) {
        push_release_issue(
            &mut issues,
            "needs_attention",
            "env_added",
            format!("Environment value `{env}` is newly required by this release."),
        );
    }

    for config in json_string_list(&diff["config"]["added"]) {
        push_release_issue(
            &mut issues,
            "needs_attention",
            "config_added",
            format!("Runtime config `{config}` is newly declared by this release."),
        );
    }

    for change in diff
        .get("capabilities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let module = change.get("module").and_then(Value::as_str).unwrap_or("-");
        for capability in json_string_list(&change["removed"]) {
            push_release_issue(
                &mut issues,
                "breaking",
                "capability_removed",
                format!("Capability `{capability}` is removed from module `{module}`."),
            );
        }
    }

    for change in diff
        .get("operations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let module = change.get("module").and_then(Value::as_str).unwrap_or("-");
        for operation in json_string_list(&change["removed"]) {
            push_release_issue(
                &mut issues,
                "breaking",
                "operation_removed",
                format!("Operation `{operation}` is removed from module `{module}`."),
            );
        }
    }

    let risk = issues
        .iter()
        .filter_map(|issue| issue.get("level").and_then(Value::as_str))
        .max_by_key(|level| service_release_risk_rank(level))
        .unwrap_or("safe");

    json!({
        "risk": risk,
        "issues": issues,
    })
}

fn push_release_issue(issues: &mut Vec<Value>, level: &str, code: &str, message: String) {
    issues.push(json!({
        "code": code,
        "level": level,
        "message": message,
    }));
}

fn policy_risk(policy: &Value) -> Result<&str> {
    policy
        .get("risk")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Service release policy risk is required"))
}

fn enforce_service_release_fail_on(policy: &Value, fail_on: Option<&str>) -> Result<()> {
    let Some(fail_on) = fail_on else {
        return Ok(());
    };
    let fail_on = fail_on.trim();
    let risk = policy_risk(policy)?;
    if service_release_risk_rank(fail_on) == 0 && fail_on != "safe" {
        bail!(
            "Unknown service release risk threshold `{fail_on}`; expected safe, needs_attention, breaking, or blocked"
        );
    }
    if service_release_risk_rank(risk) >= service_release_risk_rank(fail_on) {
        bail!("Service release policy risk `{risk}` meets --fail-on {fail_on}");
    }
    Ok(())
}

fn service_release_risk_rank(risk: &str) -> u8 {
    match risk {
        "safe" => 0,
        "needs_attention" => 1,
        "breaking" => 2,
        "blocked" => 3,
        _ => 0,
    }
}

fn service_release_restart_required(diff: &Value) -> bool {
    !json_string_list(&diff["modules"]["added"]).is_empty()
        || !json_string_list(&diff["modules"]["removed"]).is_empty()
        || !json_string_list(&diff["env"]["added"]).is_empty()
        || !json_string_list(&diff["env"]["removed"]).is_empty()
        || !json_string_list(&diff["config"]["added"]).is_empty()
        || !json_string_list(&diff["config"]["removed"]).is_empty()
        || diff
            .get("compatibilityChanged")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || diff_array_has_changes(&diff["capabilities"])
        || diff_array_has_changes(&diff["operations"])
}

fn diff_array_has_changes(value: &Value) -> bool {
    value.as_array().into_iter().flatten().any(|change| {
        !json_string_list(&change["added"]).is_empty()
            || !json_string_list(&change["removed"]).is_empty()
    })
}

fn service_release_next_action(risk: &str) -> &'static str {
    match risk {
        "safe" => "Run `lenso service release apply <plan.json>` when ready.",
        "needs_attention" => {
            "Review required env/config, then run `lenso service release apply <plan.json>`."
        }
        "breaking" => "Review removed modules, capabilities, or operations before applying.",
        "blocked" => "Fix blocked policy issues before applying this release.",
        _ => "Run `lenso service policy check <plan.json>` before applying.",
    }
}

fn print_service_release_plan(plan: &Value) {
    let service_name = plan
        .pointer("/service/name")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let current_version = plan
        .pointer("/current/version")
        .and_then(Value::as_str)
        .unwrap_or("");
    let candidate_version = plan
        .pointer("/candidate/version")
        .and_then(Value::as_str)
        .unwrap_or("");
    let policy = plan.get("policy").unwrap_or(&Value::Null);
    let risk = policy.get("risk").and_then(Value::as_str).unwrap_or("safe");
    println!("Service release plan: {service_name}");
    if !current_version.is_empty() || !candidate_version.is_empty() {
        println!(
            "version: {} -> {}",
            if current_version.is_empty() {
                "-"
            } else {
                current_version
            },
            if candidate_version.is_empty() {
                "-"
            } else {
                candidate_version
            }
        );
    }
    if let Some(environment) = plan.get("environment") {
        let name = environment
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let target = environment
            .get("target")
            .and_then(Value::as_str)
            .unwrap_or("-");
        let namespace = environment
            .get("namespace")
            .and_then(Value::as_str)
            .unwrap_or("-");
        println!("environment: {name} ({target}/{namespace})");
    }
    println!("risk: {risk}");
    println!(
        "restart required: {}",
        plan.get("restartRequired")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );
    print_service_manifest_diff(service_name, &plan["diff"]);
    let issues = policy
        .get("issues")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !issues.is_empty() {
        println!("policy issues:");
        for issue in issues {
            println!(
                "- [{}] {}",
                issue.get("level").and_then(Value::as_str).unwrap_or("-"),
                issue.get("message").and_then(Value::as_str).unwrap_or("-")
            );
        }
    }
    if let Some(next_action) = plan.get("nextAction").and_then(Value::as_str) {
        println!("next action: {next_action}");
    }
}

fn print_service_release_policy(plan: &Value, policy: &Value) {
    let service_name = plan
        .pointer("/service/name")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let risk = policy.get("risk").and_then(Value::as_str).unwrap_or("safe");
    println!("Service policy check: {service_name}");
    println!("risk: {risk}");
    if let Some(issues) = policy.get("issues").and_then(Value::as_array)
        && !issues.is_empty()
    {
        println!("issues:");
        for issue in issues {
            println!(
                "- [{}] {}",
                issue.get("level").and_then(Value::as_str).unwrap_or("-"),
                issue.get("message").and_then(Value::as_str).unwrap_or("-")
            );
        }
    }
}

fn append_service_release_ledger(repo_root: &Path, plan: &Value, policy: &Value) -> Result<()> {
    let ledger_path = repo_root.join(SERVICE_RELEASE_LEDGER_PATH);
    let mut ledger = read_json_if_exists(&ledger_path)?
        .unwrap_or_else(|| json!({ "releases": [], "version": 1 }));
    let releases = ledger
        .get_mut("releases")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Service release ledger releases must be an array"))?;
    let service_name = plan
        .pointer("/service/name")
        .and_then(Value::as_str)
        .unwrap_or("-");
    let mut record = json!({
        "appliedAtUnixMs": current_time_millis()?,
        "id": uuid::Uuid::now_v7().to_string(),
        "planCreatedAtUnixMs": plan.get("createdAtUnixMs").cloned().unwrap_or(Value::Null),
        "protocol": "lenso.service-release-ledger.v1",
        "risk": policy_risk(policy)?,
        "rollbackTarget": plan.pointer("/current/manifestReference").cloned().unwrap_or(Value::Null),
        "serviceName": service_name,
        "current": plan.get("current").cloned().unwrap_or(Value::Null),
        "candidate": plan.get("candidate").cloned().unwrap_or(Value::Null),
        "diff": plan.get("diff").cloned().unwrap_or(Value::Null),
        "policy": policy,
    });
    if let Some(environment) = plan.get("environment") {
        record["environment"] = environment.clone();
    }
    releases.push(record);
    write_json(&ledger_path, &ledger)?;
    println!(
        "Recorded service release: {}",
        display_relative(repo_root, &ledger_path)
    );
    Ok(())
}

fn current_time_millis() -> Result<u64> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    u64::try_from(millis).context("system clock timestamp exceeds u64")
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

fn service_check_config_summary(
    manifest: &Value,
    repo_root: Option<&Path>,
    env_file: Option<&Path>,
) -> Result<Value> {
    let required_env = service_env_set(manifest).into_iter().collect::<Vec<_>>();
    let Some(env_file) = env_file else {
        return Ok(json!({
            "checked": false,
            "configuredEnv": [],
            "envFile": Value::Null,
            "missingEnv": [],
            "requiredEnv": required_env,
        }));
    };
    let repo_root = resolve_repo_root(repo_root)?;
    let env_file_path = resolve_path(&repo_root, env_file);
    let configured = env_keys_from_source(&read_text_if_exists(&env_file_path)?);
    let missing_env = required_env
        .iter()
        .filter(|key| !configured.contains(*key))
        .cloned()
        .collect::<Vec<_>>();
    Ok(json!({
        "checked": true,
        "configuredEnv": configured.into_iter().collect::<Vec<_>>(),
        "envFile": display_relative(&repo_root, &env_file_path),
        "missingEnv": missing_env,
        "requiredEnv": required_env,
    }))
}

fn env_keys_from_source(source: &str) -> BTreeSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let line = line.strip_prefix("export ").unwrap_or(line);
            (!line.starts_with('#'))
                .then(|| line.split_once('=').map(|(key, _)| key.trim().to_owned()))
                .flatten()
        })
        .filter(|key| !key.is_empty())
        .collect()
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

impl ConsoleUiScaffold {
    fn from_options(options: &ModuleCreateOptions, module_id: &str) -> Self {
        let label = options
            .label
            .clone()
            .unwrap_or_else(|| title_case(module_id));
        Self {
            capability: options
                .capability
                .clone()
                .unwrap_or_else(|| format!("{module_id}.read")),
            icon: options.icon.clone().unwrap_or_else(|| "Blocks".to_owned()),
            label,
            module_id: module_id.to_owned(),
            route: options
                .route
                .clone()
                .unwrap_or_else(|| format!("/{module_id}")),
            surface_name: options
                .surface_name
                .clone()
                .unwrap_or_else(|| "main".to_owned()),
        }
    }
}

fn queue_module_files(
    pending_writes: &mut PendingWrites,
    module_dir: &Path,
    module_id: &str,
    console_surface: Option<&ConsoleUiScaffold>,
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

fn module_manifest(module_id: &str, console_surface: Option<&ConsoleUiScaffold>) -> Result<String> {
    let imports = if console_surface.is_some() {
        "use platform_module::{ConsoleNavigation, ConsoleSurface, ConsoleSurfacePresentation, ConsoleWorkspaceRef, LinkedBinding, Module, ModuleManifest};"
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
            route: {}.to_owned(),
            presentation: ConsoleSurfacePresentation::Esm {{
                entry: {}.to_owned(),
            }},
            icon: Some({}.to_owned()),
            required_capabilities: vec![{}.to_owned()],
            navigation: Some(ConsoleNavigation {{
                workspace: ConsoleWorkspaceRef {{
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
            rust_string_literal(&console_surface.route),
            rust_string_literal(&console_surface.surface_name),
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
    console_surface: Option<&ConsoleUiScaffold>,
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
    console_surface: Option<&ConsoleUiScaffold>,
) -> Result<String> {
    let console_imports = if console_surface.is_some() {
        "use lenso::{ConsoleNavigation, ConsoleSurface, ConsoleSurfacePresentation, ConsoleWorkspaceRef};\n"
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
            route: {}.to_owned(),
            presentation: ConsoleSurfacePresentation::Esm {{
                entry: {}.to_owned(),
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
            rust_string_literal(&console_surface.route),
            rust_string_literal(&console_surface.surface_name),
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

fn queue_console_ui_artifact(
    pending_writes: &mut PendingWrites,
    module_dir: &Path,
    context: &ConsoleUiScaffold,
) -> Result<()> {
    let ui_dir = module_dir.join("console-ui");
    let package_name = format!("@lenso-module/{}/console-ui", context.module_id);
    queue_write(
        pending_writes,
        ui_dir.join("package.json"),
        json_string_pretty(&json!({
            "name": package_name,
            "version": "0.1.0",
            "private": true,
            "type": "module",
            "scripts": {
                "build": "vite build --config vite.config.ts",
                "dev": "vite --host 127.0.0.1",
                "typecheck": "tsc --noEmit"
            },
            "dependencies": {
                "@lenso/console-module-api": "^0.1.0",
                "@lenso/console-ui": "^0.1.0",
                "react": "^19.0.0"
            },
            "devDependencies": {
                "@types/react": "^19.0.0",
                "typescript": "7.0.2",
                "vite": "^8.0.0"
            }
        }))?,
    );
    queue_write(
        pending_writes,
        ui_dir.join("vite.config.ts"),
        "import { defineConfig } from \"vite\";\n\nexport default defineConfig({\n  build: {\n    lib: { entry: \"src/main.tsx\", formats: [\"es\"], fileName: () => \"main.js\" },\n    rollupOptions: { external: [\"react\"] },\n  },\n});\n".to_owned(),
    );
    queue_write(
        pending_writes,
        ui_dir.join("src/main.tsx"),
        format!(
            r##"import type {{ ConsoleModuleManifest }} from "@lenso/console-module-api";
import {{ defineConsoleManifest }} from "@lenso/console-module-api";
import {{ ConsolePage, defineConsoleUiModule }} from "@lenso/console-ui";
import {{ type FC }} from "react";
import "./style.css";

const manifest: ConsoleModuleManifest = defineConsoleManifest({{
  protocol: "lenso.console-module.v1",
  moduleId: {},
  hostApi: "^1.0.0",
  consoleUi: "^1.0.0",
  surfaces: [{{
    id: {},
    path: {},
    label: {},
    area: {},
    requiredCapabilities: [{}],
    icon: {},
  }}],
}});

const MainSurface: FC = () => <ConsolePage>{}</ConsolePage>;

export default defineConsoleUiModule({{
  manifest,
  surfaces: {{ {}: MainSurface }},
}});
"##,
            serde_json::to_string(&context.module_id)?,
            serde_json::to_string(&context.surface_name)?,
            serde_json::to_string(&context.route)?,
            serde_json::to_string(&context.label)?,
            serde_json::to_string(&console_surface_area(&context.route))?,
            serde_json::to_string(&context.capability)?,
            serde_json::to_string(&context.icon)?,
            serde_json::to_string(&context.label)?,
            serde_json::to_string(&context.surface_name)?,
        ),
    );
    queue_write(
        pending_writes,
        ui_dir.join("src/style.css"),
        "/* Module-owned Console styles. Import shared Console tokens here. */\n".to_owned(),
    );
    queue_write(
        pending_writes,
        ui_dir.join("tsconfig.json"),
        json_string_pretty(&json!({
            "compilerOptions": {
                "jsx": "react-jsx",
                "lib": ["ES2022", "DOM"],
                "module": "ESNext",
                "moduleResolution": "Bundler",
                "strict": true,
                "target": "ES2022"
            },
            "include": ["src", "vite.config.ts"]
        }))?,
    );
    queue_write(
        pending_writes,
        ui_dir.join("lenso.console-ui.json"),
        json_string_pretty(&json!({
            "schema": "lenso.console-ui-esm.v1",
            "format": "console_ui_esm",
            "protocolMajor": 1,
            "moduleId": context.module_id,
            "hostApi": "^1.0.0",
            "consoleUi": "^1.0.0",
            "entry": "dist/main.js",
            "entries": [{
                "name": context.surface_name,
                "path": "dist/main.js"
            }, {
                "name": format!("{}-style", context.surface_name),
                "path": "dist/style.css"
            }],
            "styleAssets": [{
                "path": "dist/style.css",
                "order": 0
            }],
            "manifest": {
                "protocol": "lenso.console-module.v1",
                "moduleId": context.module_id,
                "hostApi": "^1.0.0",
                "consoleUi": "^1.0.0",
                "surfaces": [{
                    "id": context.surface_name,
                    "path": context.route,
                    "label": context.label,
                    "area": console_surface_area(&context.route),
                    "requiredCapabilities": [context.capability],
                    "icon": context.icon
                }]
            },
            "requestedPermissions": [{
                "permissionId": context.capability,
                "operations": ["read"],
                "resources": [context.module_id]
            }]
        }))?,
    );
    Ok(())
}

fn console_surface_area(route: &str) -> &'static str {
    match route.split('/').nth(1) {
        Some("operations") => "operations",
        Some("data") => "data",
        Some("configuration") => "configuration",
        _ => "runtime",
    }
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
            "service-module-example =",
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

fn validate_service_module_manifest(manifest: Value) -> Result<Value> {
    if !manifest.is_object() {
        bail!("Service module manifest must be a JSON object");
    }
    let name = string_field(&manifest, "name")?;
    if name.trim().is_empty() {
        bail!("Service module manifest name is required");
    }
    let version = string_field(&manifest, "version")?;
    if version.trim().is_empty() {
        bail!("Service module manifest version is required");
    }
    if manifest.get("source").and_then(Value::as_str) != Some("service") {
        bail!("Service module manifest source must be service");
    }
    if !manifest.get("capabilities").is_some_and(Value::is_array) {
        bail!("Service module manifest capabilities must be an array");
    }
    if !manifest.get("console").is_some_and(Value::is_array) {
        bail!("Service module manifest console must be an array");
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
    if manifest.get("bridgeProtocol").is_some() || manifest.get("bridge_protocol").is_some() {
        bail!("Module release cannot declare the retired Console Bridge contract");
    }
    if let Some(console_surfaces) = manifest.get("console").and_then(Value::as_array) {
        if console_surfaces.iter().any(|surface| {
            surface
                .get("presentation")
                .and_then(|presentation| presentation.get("kind"))
                .and_then(Value::as_str)
                == Some("isolated")
        }) {
            bail!("Module release cannot declare retired isolated Console surfaces");
        }
    }
    if let Some(console_artifact) = manifest
        .get("consoleUiArtifact")
        .or_else(|| manifest.get("console_ui_artifact"))
    {
        validate_console_ui_artifact(console_artifact, name)?;
    }
    if source == "service" {
        module_release_provider(&manifest)?;
    } else if let Some(provider) = manifest.get("provider")
        && !provider.is_object()
    {
        bail!("Module release provider must be an object");
    }
    Ok(manifest)
}

fn validate_console_ui_artifact(artifact: &Value, module_name: &str) -> Result<()> {
    if !artifact.is_object() {
        bail!("Console UI artifact must be an object");
    }
    if artifact.get("format").and_then(Value::as_str) != Some("console_ui_esm") {
        bail!("Console UI artifact format must be console_ui_esm");
    }
    if artifact
        .get("protocolMajor")
        .or_else(|| artifact.get("protocol_major"))
        .and_then(Value::as_u64)
        != Some(1)
    {
        bail!("Console UI artifact protocol major must be 1");
    }
    if artifact.get("bridgeProtocol").is_some() || artifact.get("bridge_protocol").is_some() {
        bail!("Console UI artifact cannot declare the retired Console Bridge contract");
    }
    let entry = artifact
        .get("entry")
        .and_then(Value::as_str)
        .filter(|entry| !entry.trim().is_empty())
        .ok_or_else(|| anyhow!("Console UI artifact entry is required"))?;
    let entries = artifact
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Console UI artifact entries must be an array"))?;
    if entries.is_empty() {
        bail!("Console UI artifact entries must not be empty");
    }
    let mut entry_paths = BTreeSet::new();
    for item in entries {
        let path = item
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Console UI artifact entry path is required"))?;
        if !valid_console_artifact_path(path) || !entry_paths.insert(path) {
            bail!("Console UI artifact entries require unique relative paths without traversal");
        }
    }
    if !entry_paths.contains(entry) {
        bail!("Console UI artifact entry must be declared by entries");
    }
    let style_assets = artifact
        .get("styleAssets")
        .or_else(|| artifact.get("style_assets"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Console UI artifact styleAssets must be an array"))?;
    let mut style_paths = BTreeSet::new();
    for item in style_assets {
        let path = item
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Console UI style asset path is required"))?;
        if !valid_console_artifact_path(path) || !style_paths.insert(path) {
            bail!("Console UI style assets require unique relative paths without traversal");
        }
        if !entry_paths.contains(path) {
            bail!("Console UI style assets must be declared by entries");
        }
    }
    let manifest = artifact
        .get("manifest")
        .ok_or_else(|| anyhow!("Console UI artifact manifest is required"))?;
    if manifest.get("protocol").and_then(Value::as_str) != Some("lenso.console-module.v1") {
        bail!("Console Module manifest protocol must be lenso.console-module.v1");
    }
    let artifact_module_id = manifest
        .get("moduleId")
        .or_else(|| manifest.get("module_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Console Module manifest moduleId is required"))?;
    if artifact_module_id != module_name {
        bail!("Console Module manifest moduleId must match the Module Release");
    }
    for field in ["hostApi", "consoleUi"] {
        if manifest.get(field).and_then(Value::as_str).is_none() {
            bail!("Console Module manifest {field} compatibility range is required");
        }
    }
    if manifest
        .get("surfaces")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        bail!("Console Module manifest surfaces must not be empty");
    }
    Ok(())
}

fn valid_console_artifact_path(path: &str) -> bool {
    !path.trim().is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
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
            object.insert("source".to_owned(), json!("service"));
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
            validate_service_module_manifest(module_manifest)
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
) -> Result<Vec<ServiceModuleServiceInstallSpec>> {
    let mut services = service_module_install_services(manifest, service_name, base_url)?;
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

fn service_uninstall_target(
    install_ledger_path: &Path,
    requested_name: &str,
) -> Result<ServiceUninstallTarget> {
    let Some(ledger) = read_json_if_exists(install_ledger_path)? else {
        return Ok(ServiceUninstallTarget {
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
        return Ok(ServiceUninstallTarget {
            provider_name: provider_name.to_owned(),
            module_names: service_receipt_module_names(modules, provider_name),
        });
    }

    let module_names = service_receipt_module_names(modules, requested_name);
    if module_names.is_empty() {
        Ok(ServiceUninstallTarget {
            provider_name: requested_name.to_owned(),
            module_names: vec![requested_name.to_owned()],
        })
    } else {
        Ok(ServiceUninstallTarget {
            provider_name: requested_name.to_owned(),
            module_names,
        })
    }
}

fn service_uninstall_dependency_warnings(
    install_ledger_path: &Path,
    target: &ServiceUninstallTarget,
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

fn service_module_manifest_compatibility_issue(manifest: &Value) -> Option<String> {
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
    if let Some(protocol_version) = compatibility
        .get("providerProtocolVersion")
        .or_else(|| compatibility.get("provider_protocol_version"))
        .and_then(Value::as_str)
        && protocol_version != PROVIDER_PROTOCOL_VERSION
    {
        return Some(format!(
            "{module_name} requires Provider protocol {protocol_version}; host supports {PROVIDER_PROTOCOL_VERSION}"
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
    Ok(read_json_reference_with_provenance(reference).await?.value)
}

async fn read_json_reference_with_provenance(reference: &str) -> Result<LoadedJsonReference> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        let url = reqwest::Url::parse(reference)
            .with_context(|| format!("parse module manifest URL {reference}"))?;
        let response = reqwest::get(url.clone())
            .await
            .with_context(|| format!("fetch module manifest {reference}"))?;
        if !response.status().is_success() {
            bail!(
                "Failed to fetch module manifest: {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("")
            );
        }
        let value = response
            .json::<Value>()
            .await
            .context("parse service module manifest JSON")?;
        return Ok(LoadedJsonReference {
            value,
            provenance: ManifestProvenance::Network,
        });
    }
    let path = if let Some(file_path) = reference.strip_prefix("file://") {
        PathBuf::from(file_path)
    } else {
        PathBuf::from(reference)
    };
    let path = fs::canonicalize(&path)
        .with_context(|| format!("canonicalize local manifest {}", path.display()))?;
    Ok(LoadedJsonReference {
        value: read_json(&path)?,
        provenance: ManifestProvenance::Local,
    })
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
    bail!("Service module base URL is required unless the manifest URL ends with /manifest");
}

fn update_service_modules_env(
    env_file_path: &Path,
    module_name: &str,
    base_url: &str,
) -> Result<String> {
    let source = read_text_if_exists(env_file_path)?;
    let current_value = source
        .lines()
        .find_map(|line| line.strip_prefix("SERVICE_MODULES="))
        .unwrap_or_default();
    let mut entries = parse_service_module_entries(current_value);
    entries.retain(|(name, _)| name != module_name);
    entries.push((module_name.to_owned(), base_url.to_owned()));
    Ok(upsert_env_value(
        &source,
        "SERVICE_MODULES",
        &format_service_module_entries(&entries),
    ))
}

fn remove_service_module_from_env(
    env_file_path: &Path,
    module_name: &str,
) -> Result<Option<String>> {
    if !env_file_path.exists() {
        return Ok(None);
    }
    Ok(remove_service_module_from_env_source(
        &read_text(env_file_path)?,
        module_name,
    ))
}

fn remove_service_module_from_env_source(source: &str, module_name: &str) -> Option<String> {
    let current_value = source
        .lines()
        .find_map(|line| line.strip_prefix("SERVICE_MODULES="))?;
    let mut entries = parse_service_module_entries(current_value);
    let original_len = entries.len();
    entries.retain(|(name, _)| name != module_name);
    if entries.len() == original_len {
        return None;
    }
    let next_value = format_service_module_entries(&entries);
    Some(if next_value.is_empty() {
        remove_env_value(source, "SERVICE_MODULES")
    } else {
        upsert_env_value(source, "SERVICE_MODULES", &next_value)
    })
}

fn service_module_install_state_exists(
    module_name: &str,
    env_file_path: &Path,
    module_services_path: &Path,
) -> Result<bool> {
    let env_source = read_text_if_exists(env_file_path)?;
    if service_module_entries_from_env_source(&env_source)
        .iter()
        .any(|(name, _)| name == module_name)
    {
        return Ok(true);
    }

    Ok(read_service_module_service_states(module_services_path)?
        .iter()
        .any(|state| state.module_name == module_name))
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

fn service_module_install_ledger_entry(
    module_name: &str,
    manifest_reference: &str,
    base_url: &str,
    manifest: &Value,
    writes: Vec<Value>,
    install_env: &[(String, String)],
    install_commands: &[InstallCommandSpec],
    install_services: &[ServiceModuleServiceInstallSpec],
) -> Value {
    let mut entry = json!({
        "baseUrl": base_url,
        "enabled": true,
        "install": {
            "commands": install_command_receipts(install_commands),
            "env": install_env_receipts(install_env),
            "services": install_service_receipts(install_services),
        },
        "manifestReference": manifest_reference,
        "moduleName": module_name,
        "source": "service",
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

fn service_module_install_writes(
    repo_root: &Path,
    env_file_path: &Path,
    module_services_path: Option<&Path>,
) -> Vec<Value> {
    let mut writes = vec![json!({
        "kind": "env",
        "key": "SERVICE_MODULES",
        "path": display_relative(repo_root, env_file_path),
    })];
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

fn install_service_receipts(install_services: &[ServiceModuleServiceInstallSpec]) -> Vec<Value> {
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

fn uninstall_linked_module(
    module_name: &str,
    options: ServiceModuleUninstallOptions,
) -> Result<()> {
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
        return Ok(ModuleSource::Service);
    }

    if builtin_linked_module_descriptor(module_name).is_some()
        || linked_module_enabled_env_exists(env_source, module_name)
    {
        return Ok(ModuleSource::Linked);
    }

    Ok(ModuleSource::Service)
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
enum ManifestProvenance {
    Local,
    Network,
    Builtin,
}

#[derive(Debug, Clone)]
struct LoadedJsonReference {
    value: Value,
    provenance: ManifestProvenance,
}

fn update_service_module_services_file(
    services_file_path: &Path,
    module_name: &str,
    install_services: &[ServiceModuleServiceInstallSpec],
) -> Result<Option<Value>> {
    let existed = services_file_path.exists();
    let mut state = read_json_if_exists(services_file_path)?
        .unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
    let modules = state
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow!("Service module services file modules must be an array"))?;
    let original_len = modules.len();
    modules.retain(|entry| entry.get("moduleName").and_then(Value::as_str) != Some(module_name));
    if !install_services.is_empty() {
        modules.push(json!({
            "moduleName": module_name,
            "services": service_module_service_plans(install_services),
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

fn remove_service_module_services_file_module(
    services_file_path: &Path,
    module_name: &str,
) -> Result<Option<Value>> {
    read_json_if_exists(services_file_path)?.map_or(Ok(None), |mut state| {
        let modules = state
            .get_mut("modules")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| anyhow!("Service module services file modules must be an array"))?;
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

fn service_module_install_env(manifest: &Value) -> Result<Vec<(String, String)>> {
    let Some(env) = manifest
        .get("install")
        .and_then(|install| install.get("env"))
    else {
        return Ok(Vec::new());
    };
    let object = env
        .as_object()
        .ok_or_else(|| anyhow!("Service module manifest install.env must be an object"))?;
    let mut values = Vec::new();
    for (key, value) in object {
        let key = key.trim();
        if key.is_empty() {
            bail!("Service module manifest install.env keys must be non-empty");
        }
        if key == "SERVICE_MODULES" {
            bail!("Service module manifest install.env must not override SERVICE_MODULES");
        }
        let value = value
            .as_str()
            .ok_or_else(|| anyhow!("Service module manifest install.env.{key} must be a string"))?;
        values.push((key.to_owned(), value.to_owned()));
    }
    Ok(values)
}

fn service_module_install_commands(manifest: &Value) -> Result<Vec<InstallCommandSpec>> {
    let Some(commands) = manifest
        .get("install")
        .and_then(|install| install.get("commands"))
    else {
        return Ok(Vec::new());
    };
    let commands = commands
        .as_array()
        .ok_or_else(|| anyhow!("Service module manifest install.commands must be an array"))?;
    commands
        .iter()
        .map(|entry| match entry {
            Value::String(command) => install_command_spec(command, None),
            Value::Object(object) => {
                let command = object
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        anyhow!("Service module manifest install.commands[].command is required")
                    })?;
                let cwd = object
                    .get("cwd")
                    .map(|value| {
                        value.as_str().map(ToOwned::to_owned).ok_or_else(|| {
                            anyhow!(
                                "Service module manifest install.commands[].cwd must be a string"
                            )
                        })
                    })
                    .transpose()?;
                install_command_spec(command, cwd)
            }
            _ => {
                bail!("Service module manifest install.commands entries must be strings or objects")
            }
        })
        .collect()
}

fn service_module_install_services(
    manifest: &Value,
    module_name: &str,
    base_url: &str,
) -> Result<Vec<ServiceModuleServiceInstallSpec>> {
    let Some(services) = manifest
        .get("install")
        .and_then(|install| install.get("services"))
    else {
        return Ok(Vec::new());
    };
    let services = services
        .as_array()
        .ok_or_else(|| anyhow!("Service module manifest install.services must be an array"))?;
    services
        .iter()
        .map(|entry| {
            let object = entry.as_object().ok_or_else(|| {
                anyhow!("Service module manifest install.services entries must be objects")
            })?;
            let command = object
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    anyhow!("Service module manifest install.services[].command is required")
                })?
                .trim();
            if command.is_empty() {
                bail!("Service module manifest install service command must be non-empty");
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
            Ok(ServiceModuleServiceInstallSpec {
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
                                "Service module manifest install.services[].cwd must be a string"
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
        bail!("Service module manifest install command must be non-empty");
    }
    Ok(InstallCommandSpec {
        command: command.to_owned(),
        cwd: cwd
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()),
    })
}

#[cfg(test)]
fn install_service_plans(install_services: &[ServiceModuleServiceInstallSpec]) -> Vec<Value> {
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

fn service_module_service_plans(
    install_services: &[ServiceModuleServiceInstallSpec],
) -> Vec<Value> {
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

fn read_service_module_service_states(
    services_file_path: &Path,
) -> Result<Vec<ServiceModuleServiceState>> {
    let Some(value) = read_json_if_exists(services_file_path)? else {
        return Ok(Vec::new());
    };
    parse_service_module_service_states(&value)
}

fn parse_service_module_service_states(value: &Value) -> Result<Vec<ServiceModuleServiceState>> {
    let modules = value
        .get("modules")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Service module services file modules must be an array"))?;
    let mut states = Vec::new();
    for module in modules {
        let module_name = module
            .get("moduleName")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Service module services file moduleName must be a string"))?
            .trim();
        if module_name.is_empty() {
            bail!("Service module services file moduleName must be non-empty");
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
            service_specs.push(ServiceModuleServiceInstallSpec {
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
        states.push(ServiceModuleServiceState {
            module_name: module_name.to_owned(),
            services: service_specs,
        });
    }
    Ok(states)
}

async fn provider_service_ready_url(client: &reqwest::Client, ready_url: &str) -> bool {
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
    service: &ServiceModuleServiceInstallSpec,
    lock_file_path: &Path,
    pid_file_path: &Path,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_millis(service.ready_timeout_ms);
    loop {
        if provider_service_ready_url(client, &service.ready_url).await {
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

fn service_module_service_doctor_status(
    configured: bool,
    enabled: bool,
    auto_start: bool,
    ready: bool,
    lock_exists: bool,
    pid_exists: bool,
) -> ServiceModuleServiceDoctorStatus {
    if !configured {
        return ServiceModuleServiceDoctorStatus::NotConfigured;
    }
    if !enabled {
        return ServiceModuleServiceDoctorStatus::Disabled;
    }
    if ready {
        return ServiceModuleServiceDoctorStatus::Ready;
    }
    if !auto_start {
        return ServiceModuleServiceDoctorStatus::ManualNotReady;
    }
    if lock_exists || pid_exists {
        return ServiceModuleServiceDoctorStatus::StaleState;
    }
    ServiceModuleServiceDoctorStatus::NotReady
}

fn service_module_service_doctor_fix(
    status: ServiceModuleServiceDoctorStatus,
) -> Option<&'static str> {
    match status {
        ServiceModuleServiceDoctorStatus::Ready => None,
        ServiceModuleServiceDoctorStatus::Disabled => {
            Some("enable the module if this service should run")
        }
        ServiceModuleServiceDoctorStatus::ManualNotReady => {
            Some("start this service manually or set autoStart=true in the manifest")
        }
        ServiceModuleServiceDoctorStatus::NotConfigured => {
            Some("install the module or remove its service entry")
        }
        ServiceModuleServiceDoctorStatus::NotReady => {
            Some("start the service command or restart the API/worker")
        }
        ServiceModuleServiceDoctorStatus::StaleState => {
            Some("restart the API/worker; remove stale lock/pid files if it remains stuck")
        }
    }
}

fn module_service_log_path(repo_root: &Path, module_name: &str, service_name: &str) -> PathBuf {
    repo_root
        .join(".lenso/service-logs")
        .join(service_module_service_state_segment(module_name))
        .join(format!(
            "{}.log",
            service_module_service_state_segment(service_name)
        ))
}

fn tail_lines(contents: &str, tail: usize) -> Vec<&str> {
    let lines = contents.lines().collect::<Vec<_>>();
    lines[lines.len().saturating_sub(tail)..].to_vec()
}

fn service_module_service_state_path(
    services_state_dir: &Path,
    module_name: &str,
    service: &ServiceModuleServiceInstallSpec,
    extension: &str,
) -> PathBuf {
    services_state_dir.join(format!(
        "remote-{}-{}.{}",
        service_module_service_state_segment(module_name),
        service_module_service_state_segment(&service.name),
        extension
    ))
}

fn service_module_service_state_segment(value: &str) -> String {
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

async fn read_install_descriptor(reference: &str) -> Result<Option<LoadedJsonReference>> {
    if let Some(descriptor) = builtin_linked_module_descriptor(reference) {
        return Ok(Some(LoadedJsonReference {
            value: descriptor,
            provenance: ManifestProvenance::Builtin,
        }));
    }

    if !looks_like_json_reference(reference) {
        return Ok(None);
    }

    let descriptor = read_json_reference_with_provenance(reference).await?;
    Ok(
        (descriptor.value.get("source").is_some() && !is_service_manifest(&descriptor.value))
            .then_some(descriptor),
    )
}

fn builtin_linked_module_descriptor(reference: &str) -> Option<Value> {
    match reference.trim() {
        "auth" => Some(json!({
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
        })),
        "auth-oauth" => Some(json!({
            "name": "auth-oauth",
            "source": "linked",
            "dependencies": ["auth"],
            "linked": {
                "call": "auth_oauth::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-auth-oauth",
                    "version": "0.1.0"
                }
            }
        })),
        "auth-anonymous" => Some(json!({
            "name": "auth-anonymous",
            "source": "linked",
            "dependencies": ["auth"],
            "linked": {
                "call": "auth_anonymous::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-auth-anonymous",
                    "version": "0.1.0"
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
        "auth-phone" => Some(json!({
            "name": "auth-phone",
            "source": "linked",
            "dependencies": ["auth", "auth-password"],
            "linked": {
                "call": "builtins::auth_phone()"
            }
        })),
        "auth-github" => Some(json!({
            "name": "auth-github",
            "source": "linked",
            "dependencies": ["auth", "auth-oauth"],
            "linked": {
                "call": "auth_github::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-auth-github",
                    "version": "0.1.0"
                }
            }
        })),
        "auth-google" => Some(json!({
            "name": "auth-google",
            "source": "linked",
            "dependencies": ["auth", "auth-oauth"],
            "linked": {
                "call": "auth_google::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-auth-google",
                    "version": "0.1.0"
                }
            }
        })),
        "auth-oidc" => Some(json!({
            "name": "auth-oidc",
            "source": "linked",
            "dependencies": ["auth"],
            "linked": {
                "call": "auth_oidc::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-auth-oidc",
                    "version": "0.1.0"
                }
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
        "organization" => Some(json!({
            "name": "organization",
            "source": "linked",
            "dependencies": ["auth"],
            "linked": {
                "call": "organization::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-organization",
                    "version": "0.1.1"
                }
            }
        })),
        "audit-log" => Some(json!({
            "name": "audit-log",
            "source": "linked",
            "capabilities": ["audit_log.events.read"],
            "linked": {
                "call": "audit_log::module::linked_module()",
                "cargo": {
                    "package": "lenso-module-audit-log",
                    "version": "0.1.0"
                }
            }
        })),
        _ => None,
    }
}

fn builtin_linked_module_names() -> &'static [&'static str] {
    &[
        "auth",
        "auth-oauth",
        "auth-anonymous",
        "auth-password",
        "auth-phone",
        "auth-github",
        "auth-google",
        "auth-oidc",
        "auth-device",
        "organization",
        "audit-log",
    ]
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
        if key == "SERVICE_MODULES" {
            bail!(
                "Linked module descriptor install profile `{profile}` env must not override SERVICE_MODULES"
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
        "service" => Ok(ModuleSource::Service),
        other => bail!("Unsupported module source `{other}`; expected `service` or `linked`"),
    }
}

fn parse_service_module_entries(value: &str) -> Vec<(String, String)> {
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

fn service_module_entries_from_env_source(source: &str) -> Vec<(String, String)> {
    let current_value = source
        .lines()
        .find_map(|line| line.strip_prefix("SERVICE_MODULES="))
        .unwrap_or_default();
    parse_service_module_entries(current_value)
}

fn service_module_manifest_url(base_url: &str) -> Option<String> {
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

fn format_service_module_entries(entries: &[(String, String)]) -> String {
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
        .ok_or_else(|| anyhow!("Service module manifest {key} is required"))
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

    #[test]
    fn generated_console_scaffold_uses_the_framework_esm_contract() {
        let context = ConsoleUiScaffold {
            capability: "billing.read".to_owned(),
            icon: "boxes".to_owned(),
            label: "Billing".to_owned(),
            module_id: "acme/billing".to_owned(),
            route: "/data/billing".to_owned(),
            surface_name: "billing".to_owned(),
        };
        let manifest = module_manifest(&context.module_id, Some(&context)).unwrap();
        assert!(manifest.contains("ConsoleSurfacePresentation::Esm"));
        assert!(!manifest.contains("Console Bridge"));
        assert!(!manifest.contains("CONSOLE_BRIDGE"));

        let mut pending = PendingWrites::new();
        let root = PathBuf::from("modules/acme-billing");
        queue_console_ui_artifact(&mut pending, &root, &context).unwrap();
        let descriptor = pending
            .get(&root.join("console-ui/lenso.console-ui.json"))
            .expect("Console ESM descriptor should be scaffolded");
        let descriptor: Value = serde_json::from_str(descriptor).unwrap();
        assert_eq!(descriptor["format"], "console_ui_esm");
        assert_eq!(descriptor["protocolMajor"], 1);
        assert_eq!(descriptor["entry"], "dist/main.js");
        assert_eq!(descriptor["styleAssets"][0]["path"], "dist/style.css");
        assert_eq!(descriptor["manifest"]["surfaces"][0]["area"], "data");
        assert_eq!(
            descriptor["manifest"]["protocol"],
            "lenso.console-module.v1"
        );
        assert!(descriptor.get("bridgeProtocol").is_none());
    }

    #[test]
    fn module_release_validation_rejects_retired_console_bridge_artifacts() {
        let retired = json!({
            "protocol": "lenso.module-release.v1",
            "name": "billing",
            "version": "0.1.0",
            "source": "linked",
            "capabilities": [],
            "dependencies": [],
            "consoleUiArtifact": {
                "format": "isolated_web",
                "protocolMajor": 1,
                "entry": "dist/index.html",
                "entries": [{"name": "billing", "path": "dist/index.html"}],
                "styleAssets": [],
                "bridgeProtocol": "lenso.console-bridge.v1",
                "manifest": {
                    "protocol": "lenso.console-module.v1",
                    "moduleId": "billing",
                    "hostApi": "^1.0.0",
                    "consoleUi": "^1.0.0",
                    "surfaces": [{
                        "id": "billing",
                        "path": "/billing",
                        "label": "Billing",
                        "area": "runtime"
                    }]
                }
            }
        });
        let error = validate_module_release_descriptor(retired).unwrap_err();
        assert!(error.to_string().contains("console_ui_esm"));
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
    fn service_environment_upsert_derives_manifest_and_sorts() {
        let mut file = json!({ "version": 1, "environments": [] });
        let prod = service_environment_value(&ServiceEnvAddOptions {
            environment_name: "prod".to_owned(),
            image: Some("ghcr.io/acme/support-suite-provider:0.4.0".to_owned()),
            ingress_host: Some("support.example.com".to_owned()),
            json: false,
            kube_context: None,
            manifest_reference: None,
            namespace: Some("lenso-prod".to_owned()),
            port: Some(4110),
            public_base_url: Some("https://support.example.com/".to_owned()),
            release_track: None,
            replicas: Some(2),
            repo_root: None,
            service_name: "support-suite-provider".to_owned(),
            target: "kubernetes".to_owned(),
        });
        let staging = service_environment_value(&ServiceEnvAddOptions {
            environment_name: "staging".to_owned(),
            image: Some("ghcr.io/acme/support-suite-provider:0.4.0".to_owned()),
            ingress_host: None,
            json: false,
            kube_context: None,
            manifest_reference: None,
            namespace: Some("lenso-staging".to_owned()),
            port: None,
            public_base_url: None,
            release_track: None,
            replicas: None,
            repo_root: None,
            service_name: "support-suite-provider".to_owned(),
            target: "kubernetes".to_owned(),
        });

        upsert_service_environment(&mut file, prod).unwrap();
        upsert_service_environment(&mut file, staging).unwrap();

        assert_eq!(file["environments"][0]["name"], "prod");
        assert_eq!(
            file["environments"][0]["manifestReference"],
            "https://support.example.com/lenso/service/v1/manifest"
        );
        assert_eq!(file["environments"][0]["releaseTrack"], "prod");
        assert_eq!(file["environments"][0]["config"]["replicas"], 2);
        assert_eq!(file["environments"][1]["name"], "staging");
    }

    #[test]
    fn service_environment_verify_accepts_operator_targets() {
        let root = std::env::temp_dir().join(format!(
            "lenso-cli-service-env-operator-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".lenso")).unwrap();
        write_json(
            &root.join(MODULE_INSTALL_LEDGER_PATH),
            &json!({
                "version": 1,
                "modules": [
                    {
                        "moduleName": "support-suite-provider",
                        "source": "service",
                        "serviceManifestSnapshot": {
                            "protocol": "lenso.service.v1",
                            "name": "support-suite-provider",
                            "version": "0.4.0",
                            "modules": []
                        }
                    }
                ]
            }),
        )
        .unwrap();
        let checks = service_environment_checks(
            &root,
            &json!({
                "name": "prod",
                "serviceName": "support-suite-provider",
                "target": "operator",
                "namespace": "lenso-prod",
                "image": "ghcr.io/acme/support-suite-provider:0.4.0",
                "manifestReference": "https://support.example.com/lenso/service/v1/manifest"
            }),
        );

        assert!(
            checks
                .iter()
                .all(|check| { check.get("status").and_then(Value::as_str) == Some("ok") })
        );
        assert!(checks.iter().any(|check| {
            check.get("name").and_then(Value::as_str) == Some("target")
                && check.get("detail").and_then(Value::as_str) == Some("operator")
        }));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn kubernetes_deployment_state_and_drift_are_computed() {
        let deployment = json!({
            "metadata": {
                "name": "support-suite-provider",
                "namespace": "lenso-staging",
                "annotations": {
                    "lenso.dev/release-id": "rel_1"
                }
            },
            "spec": {
                "replicas": 2,
                "template": {
                    "spec": {
                        "containers": [
                            { "name": "support-suite-provider", "image": "ghcr.io/acme/support-suite-provider:0.4.0" }
                        ]
                    }
                }
            },
            "status": {
                "readyReplicas": 2,
                "availableReplicas": 2
            }
        });

        assert_eq!(kubernetes_deployment_state(&deployment), "ready");
        assert_eq!(
            service_deployment_drift(
                Some("rel_1"),
                Some("rel_1"),
                Some("ghcr.io/acme/support-suite-provider:0.4.0"),
                deployment_container_image(&deployment).as_deref(),
            ),
            "in_sync"
        );
        assert_eq!(
            service_deployment_drift(
                Some("rel_1"),
                Some("rel_1"),
                Some("ghcr.io/acme/support-suite-provider:0.4.1"),
                deployment_container_image(&deployment).as_deref(),
            ),
            "image_drift"
        );
        assert_eq!(
            kubernetes_export_files(true, true, true, true),
            vec![
                "deployment.yaml",
                "service.yaml",
                "configmap.yaml",
                "secret.example.yaml",
                "kustomization.yaml",
                "README.md",
                "ingress.yaml",
                "hpa.yaml",
                "pdb.yaml",
                "networkpolicy.yaml"
            ]
        );
    }

    #[test]
    fn service_deployment_wait_readiness_uses_provider_neutral_observation() {
        assert!(service_deployment_wait_ready(&json!({
            "state": "ready",
            "drift": "in_sync"
        })));
        assert!(service_deployment_wait_ready(&json!({
            "state": "ready",
            "drift": "unknown"
        })));
        assert!(!service_deployment_wait_ready(&json!({
            "state": "ready",
            "drift": "host_ahead"
        })));
        assert!(service_deployment_wait_failed(&json!({
            "state": "failed",
            "drift": "in_sync"
        })));
    }

    #[test]
    fn service_deployment_ledger_keeps_current_and_history() {
        let root = std::env::temp_dir().join(format!(
            "lenso-cli-deployment-ledger-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let path = root.join(".lenso/service-deployments.json");

        upsert_service_deployment_observation(
            &path,
            json!({
                "serviceName": "support-suite-provider",
                "environment": "staging",
                "target": "operator",
                "observedAtUnixMs": 1,
                "state": "progressing",
                "drift": "host_ahead"
            }),
        )
        .unwrap();
        upsert_service_deployment_observation(
            &path,
            json!({
                "serviceName": "support-suite-provider",
                "environment": "staging",
                "target": "operator",
                "observedAtUnixMs": 2,
                "state": "ready",
                "drift": "in_sync"
            }),
        )
        .unwrap();

        let ledger: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(ledger["version"], 2);
        assert_eq!(ledger["observations"].as_array().unwrap().len(), 1);
        assert_eq!(ledger["observations"][0]["state"], "ready");
        assert_eq!(ledger["history"].as_array().unwrap().len(), 2);
        assert_eq!(ledger["history"][0]["state"], "progressing");
        assert_eq!(ledger["history"][1]["state"], "ready");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn latest_service_release_for_env_ignores_newer_other_env_release() {
        let root =
            std::env::temp_dir().join(format!("lenso-cli-release-env-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".lenso")).unwrap();
        fs::write(
            root.join(".lenso/service-releases.json"),
            json!({
                "version": 1,
                "releases": [
                    {
                        "id": "rel_staging",
                        "serviceName": "support-suite-provider",
                        "appliedAtUnixMs": 1,
                        "environment": {"name": "staging", "target": "operator"},
                        "candidate": {"version": "0.4.0"}
                    },
                    {
                        "id": "rel_prod",
                        "serviceName": "support-suite-provider",
                        "appliedAtUnixMs": 2,
                        "environment": {"name": "prod", "target": "operator"},
                        "candidate": {"version": "0.5.0"}
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();

        let release =
            latest_service_release_for_env(&root, "support-suite-provider", "staging").unwrap();

        assert_eq!(
            release
                .as_ref()
                .and_then(|release| release.get("id"))
                .and_then(Value::as_str),
            Some("rel_staging")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn service_deploy_export_operator_writes_provider_cr() {
        let root =
            std::env::temp_dir().join(format!("lenso-cli-operator-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".lenso")).unwrap();
        fs::write(
            root.join(".lenso/service-environments.json"),
            json!({
                "version": 1,
                "environments": [{
                    "name": "staging",
                    "serviceName": "support-suite-provider",
                    "target": "operator",
                    "namespace": "lenso-staging",
                    "image": "ghcr.io/acme/support-suite-provider:0.4.0",
                    "manifestReference": "https://support-staging.example.com/lenso/service/v1/manifest",
                    "config": {
                        "port": 4110,
                        "replicas": 2,
                        "ingressHost": "support-staging.example.com",
                        "autoscaling": true,
                        "disruptionBudget": true,
                        "networkPolicy": true
                    }
                }]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            root.join(".lenso/service-releases.json"),
            json!({
                "version": 1,
                "releases": [
                    {
                        "id": "rel_staging",
                        "serviceName": "support-suite-provider",
                        "appliedAtUnixMs": 1,
                        "environment": {"name": "staging", "target": "operator"},
                        "candidate": {"version": "0.4.0"}
                    },
                    {
                        "id": "rel_prod",
                        "serviceName": "support-suite-provider",
                        "appliedAtUnixMs": 2,
                        "environment": {"name": "prod", "target": "operator"},
                        "candidate": {"version": "0.5.0"}
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();
        let output_dir = root.join("dist/operator/staging");

        export_service_deployment(ServiceDeployExportOptions {
            environment_name: "staging".to_owned(),
            image: None,
            ingress_host: None,
            json: false,
            hpa: false,
            namespace: None,
            network_policy: false,
            output_dir: output_dir.clone(),
            pdb: false,
            port: None,
            replicas: None,
            repo_root: Some(root.clone()),
            service_name: "support-suite-provider".to_owned(),
            target: "operator".to_owned(),
        })
        .unwrap();

        let cr = fs::read_to_string(output_dir.join("lensoserviceprovider.yaml")).unwrap();
        assert!(cr.contains("kind: LensoServiceProvider"));
        assert!(cr.contains("serviceName: support-suite-provider"));
        assert!(cr.contains("releaseId: rel_staging"));
        assert!(cr.contains("targetCpuUtilization: 70"));
        assert!(output_dir.join("kustomization.yaml").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn service_deploy_status_operator_maps_crd_status_to_observation() {
        let root =
            std::env::temp_dir().join(format!("lenso-cli-operator-status-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".lenso")).unwrap();
        fs::write(
            root.join(".lenso/service-environments.json"),
            json!({
                "version": 1,
                "environments": [{
                    "name": "staging",
                    "serviceName": "support-suite-provider",
                    "target": "operator",
                    "namespace": "lenso-staging",
                    "image": "ghcr.io/acme/support-suite-provider:0.4.0"
                }]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            root.join(".lenso/service-releases.json"),
            json!({
                "version": 1,
                "releases": [
                    {
                        "id": "rel_staging",
                        "serviceName": "support-suite-provider",
                        "appliedAtUnixMs": 1,
                        "environment": {"name": "staging", "target": "operator"},
                        "candidate": {"version": "0.4.0"}
                    },
                    {
                        "id": "rel_prod",
                        "serviceName": "support-suite-provider",
                        "appliedAtUnixMs": 2,
                        "environment": {"name": "prod", "target": "operator"},
                        "candidate": {"version": "0.5.0"}
                    }
                ]
            })
            .to_string(),
        )
        .unwrap();
        let fixture = root.join("operator-status.json");
        fs::write(
            &fixture,
            json!({
                "apiVersion": "lenso.dev/v1alpha1",
                "kind": "LensoServiceProvider",
                "metadata": {
                    "name": "support-suite-provider",
                    "namespace": "lenso-staging",
                    "generation": 3
                },
                "status": {
                    "state": "ready",
                    "observedGeneration": 3,
                    "observedReleaseId": "rel_staging",
                    "observedImage": "ghcr.io/acme/support-suite-provider:0.4.0",
                    "readyReplicas": 2,
                    "desiredReplicas": 2,
                    "availableReplicas": 2,
                    "manifestReference": "https://support-staging.example.com/lenso/service/v1/manifest",
                    "conditions": [{
                        "type": "Ready",
                        "status": "True",
                        "reason": "DeploymentAvailable",
                        "message": "2/2 replicas are ready.",
                        "lastTransitionTime": "2026-06-29T00:00:00Z"
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();

        status_service_deployment(ServiceDeployStatusOptions {
            environment_name: "staging".to_owned(),
            from_file: Some(fixture),
            json: false,
            repo_root: Some(root.clone()),
            service_name: "support-suite-provider".to_owned(),
            source: "operator".to_owned(),
            write_state: true,
        })
        .unwrap();

        let observations: Value = serde_json::from_str(
            &fs::read_to_string(root.join(".lenso/service-deployments.json")).unwrap(),
        )
        .unwrap();
        let observation = &observations["observations"][0];
        assert_eq!(observation["target"], "operator");
        assert_eq!(
            observation["operator"]["resource"],
            "support-suite-provider"
        );
        assert_eq!(observation["host"]["releaseId"], "rel_staging");
        assert_eq!(observation["drift"], "in_sync");
        assert_eq!(observations["version"], 2);
        assert_eq!(observations["history"].as_array().unwrap().len(), 1);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn service_release_environment_must_match_requested_env() {
        let plan = json!({
            "protocol": "lenso.service-release-plan.v1",
            "service": { "name": "support-suite-provider" },
            "candidate": { "manifestReference": "./lenso.service.json" },
            "diff": {},
            "environment": { "name": "staging", "target": "kubernetes" }
        });

        validate_service_release_plan_environment(&plan, Some("staging")).unwrap();
        assert!(validate_service_release_plan_environment(&plan, Some("prod")).is_err());
    }

    #[test]
    fn service_release_candidate_reference_prefers_input_then_package() {
        let release = json!({
            "candidate": {
                "inputReference": "./dist/lenso.service-package.json",
                "packageReference": "./fallback/lenso.service-package.json",
                "manifestReference": "./fallback/lenso.service.json"
            }
        });
        assert_eq!(
            service_release_candidate_reference(&release).unwrap(),
            "./dist/lenso.service-package.json"
        );

        let release = json!({
            "candidate": {
                "packageReference": "./fallback/lenso.service-package.json",
                "manifestReference": "./fallback/lenso.service.json"
            }
        });
        assert_eq!(
            service_release_candidate_reference(&release).unwrap(),
            "./fallback/lenso.service-package.json"
        );
    }

    #[test]
    fn env_service_modules_are_upserted() {
        let source = "APP_ENV=local\nSERVICE_MODULES=crm=http://old\nRUST_LOG=info\n";
        let updated = upsert_env_value(
            source,
            "SERVICE_MODULES",
            &format_service_module_entries(&[
                ("crm".to_owned(), "http://old".to_owned()),
                ("billing".to_owned(), "http://new".to_owned()),
            ]),
        );

        assert!(updated.contains("APP_ENV=local"));
        assert!(updated.contains("RUST_LOG=info"));
        assert!(updated.contains("SERVICE_MODULES=crm=http://old,billing=http://new"));
    }

    #[test]
    fn env_service_modules_are_removed() {
        let source = "APP_ENV=local\nSERVICE_MODULES=crm=http://old,billing=http://new\n";
        let updated = remove_service_module_from_env_source(source, "crm").unwrap();

        assert!(updated.contains("APP_ENV=local"));
        assert!(updated.contains("SERVICE_MODULES=billing=http://new"));
        assert!(!updated.contains("crm=http://old"));
    }

    #[test]
    fn env_service_modules_line_is_removed_when_empty() {
        let source = "APP_ENV=local\nSERVICE_MODULES=crm=http://old\n";
        let updated = remove_service_module_from_env_source(source, "crm").unwrap();

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
        assert_eq!(
            parse_module_source("service").unwrap(),
            ModuleSource::Service
        );
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
        assert!(should_resolve_service_catalog_entry(ModuleSource::Service));
        assert!(!should_resolve_service_catalog_entry(ModuleSource::Linked));
    }

    #[test]
    fn uninstall_source_infers_linked_for_builtin_when_service_is_absent() {
        assert_eq!(
            infer_uninstall_module_source("auth", "", false).unwrap(),
            ModuleSource::Linked
        );
    }

    #[test]
    fn uninstall_source_prefers_service_install_state() {
        assert_eq!(
            infer_uninstall_module_source("auth", "", true).unwrap(),
            ModuleSource::Service
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
                { "moduleName": "crm", "source": "service" },
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
                        "source": "service"
                    },
                    {
                        "moduleName": "support-sla",
                        "service": { "name": "support-service" },
                        "source": "service"
                    },
                    {
                        "moduleName": "crm",
                        "source": "service"
                    }
                ],
                "version": 1
            }),
        )
        .unwrap();

        let by_module = service_uninstall_target(&path, "support-ticket").unwrap();
        let by_provider = service_uninstall_target(&path, "support-service").unwrap();
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
                        "source": "service"
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
    fn builtin_auth_descriptor_declares_linked_source() {
        let descriptor = builtin_linked_module_descriptor("auth").expect("auth descriptor");

        assert_eq!(descriptor["name"], "auth");
        assert_eq!(descriptor["source"], "linked");
        assert_eq!(descriptor["linked"]["call"], "builtins::auth()");
    }

    #[test]
    fn builtin_auth_phone_descriptor_declares_facade_linked_source() {
        let descriptor =
            builtin_linked_module_descriptor("auth-phone").expect("auth-phone descriptor");

        assert_eq!(descriptor["source"], "linked");
        assert_eq!(descriptor["dependencies"], json!(["auth", "auth-password"]));
        assert_eq!(descriptor["linked"]["call"], "builtins::auth_phone()");
        assert!(descriptor["linked"].get("cargo").is_none());
    }

    #[test]
    fn catalog_auth_phone_entry_resolves_builtin_descriptor() {
        let catalog = json!({
            "version": 1,
            "modules": [{
                "name": "auth-phone",
                "version": "0.1.1",
                "source": "linked",
                "manifestReference": "builtin:auth-phone",
                "dependencies": ["auth", "auth-password"]
            }]
        });

        let target = catalog_install_target_for_module(&catalog, "auth-phone")
            .unwrap()
            .expect("catalog target");
        let CatalogInstallTarget::Descriptor {
            descriptor,
            descriptor_reference,
            provenance: _,
        } = target
        else {
            panic!("expected linked descriptor target");
        };

        assert_eq!(descriptor_reference, "builtin:auth-phone");
        assert_eq!(descriptor["dependencies"], json!(["auth", "auth-password"]));
        assert_eq!(descriptor["linked"]["call"], "builtins::auth_phone()");
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
    fn builtin_auth_oauth_provider_descriptors_declare_external_linked_crates() {
        let oauth = builtin_linked_module_descriptor("auth-oauth").expect("auth-oauth descriptor");
        let github =
            builtin_linked_module_descriptor("auth-github").expect("auth-github descriptor");
        let google =
            builtin_linked_module_descriptor("auth-google").expect("auth-google descriptor");
        let oidc = builtin_linked_module_descriptor("auth-oidc").expect("auth-oidc descriptor");

        assert_eq!(oauth["dependencies"], json!(["auth"]));
        assert_eq!(
            oauth["linked"]["call"],
            "auth_oauth::module::linked_module()"
        );
        assert_eq!(github["dependencies"], json!(["auth", "auth-oauth"]));
        assert_eq!(
            github["linked"]["cargo"]["package"],
            "lenso-module-auth-github"
        );
        assert_eq!(
            google["linked"]["call"],
            "auth_google::module::linked_module()"
        );
        assert_eq!(oidc["linked"]["call"], "auth_oidc::module::linked_module()");
    }

    #[test]
    fn builtin_organization_descriptor_declares_external_linked_crate() {
        let descriptor =
            builtin_linked_module_descriptor("organization").expect("organization descriptor");

        assert_eq!(descriptor["source"], "linked");
        assert_eq!(descriptor["dependencies"], json!(["auth"]));
        assert_eq!(
            descriptor["linked"]["call"],
            "organization::module::linked_module()"
        );
        assert_eq!(
            descriptor["linked"]["cargo"],
            json!({
                "package": "lenso-module-organization",
                "version": "0.1.1"
            })
        );
    }

    #[test]
    fn builtin_audit_log_descriptor_declares_external_linked_crate() {
        let descriptor =
            builtin_linked_module_descriptor("audit-log").expect("audit-log descriptor");

        assert_eq!(descriptor["source"], "linked");
        assert_eq!(descriptor["capabilities"], json!(["audit_log.events.read"]));
        assert_eq!(
            descriptor["linked"]["call"],
            "audit_log::module::linked_module()"
        );
        assert_eq!(
            descriptor["linked"]["cargo"],
            json!({
                "package": "lenso-module-audit-log",
                "version": "0.1.0"
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
                    { "command": "pnpm install", "cwd": "../module-ui" }
                ]
            }
        });
        let env = service_module_install_env(&manifest).unwrap();
        let commands = service_module_install_commands(&manifest).unwrap();
        let command_plan = install_command_plans(&commands, false);

        assert_eq!(
            env,
            vec![("CRM_API_URL".to_owned(), "http://crm".to_owned())]
        );
        assert_eq!(commands[0].command, "just migrate");
        assert_eq!(commands[1].cwd.as_deref(), Some("../module-ui"));
        assert_eq!(
            command_plan[0].get("status").and_then(Value::as_str),
            Some("requires_manual_run")
        );
    }

    #[test]
    fn manifest_install_env_cannot_override_service_modules() {
        let manifest = json!({
            "install": {
                "env": {
                    "SERVICE_MODULES": "crm=http://other"
                }
            }
        });

        assert!(service_module_install_env(&manifest).is_err());
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
        let services = service_module_install_services(
            &manifest,
            "crm",
            "http://127.0.0.1:4100/lenso/module/v1",
        )
        .unwrap();
        let service_file = update_service_module_services_file(
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
    fn service_module_service_states_are_parsed() {
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
        let states = parse_service_module_service_states(&state).unwrap();

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
            service_module_service_doctor_status(true, true, true, false, true, true),
            ServiceModuleServiceDoctorStatus::StaleState
        );
        assert_eq!(
            service_module_service_doctor_status(true, true, false, false, false, false),
            ServiceModuleServiceDoctorStatus::ManualNotReady
        );
        assert!(
            service_module_service_doctor_status(true, true, false, false, false, false).is_issue()
        );
        assert_eq!(
            service_module_service_doctor_status(false, true, true, true, false, false),
            ServiceModuleServiceDoctorStatus::NotConfigured
        );
    }

    #[test]
    fn network_manifest_url_only_checks_http_sources() {
        assert_eq!(
            service_module_manifest_url("https://example.com/lenso/module/v1"),
            Some("https://example.com/lenso/module/v1/manifest".to_owned())
        );
        assert_eq!(
            service_module_manifest_url("https://example.com/lenso/module/v1/manifest"),
            Some("https://example.com/lenso/module/v1/manifest".to_owned())
        );
        assert_eq!(
            service_module_manifest_url("grpc://example.com:50051"),
            None
        );
    }

    #[test]
    fn service_manifest_compatibility_blocks_unsupported_provider_protocol() {
        let manifest = json!({
            "compatibility": {
                "providerProtocolVersion": "99"
            },
            "name": "billing"
        });

        let issue = service_module_manifest_compatibility_issue(&manifest).unwrap();

        assert!(issue.contains("requires Provider protocol 99"));
    }

    #[test]
    fn service_install_receipt_keeps_service_metadata() {
        let manifest = json!({
            "compatibility": { "providerProtocolVersion": "1" },
            "deployment": { "target": "container-paas" },
            "service": { "name": "api", "statusUrl": "http://127.0.0.1:4100/status" }
        });

        let receipt = service_module_install_ledger_entry(
            "billing",
            "http://127.0.0.1:4100/manifest",
            "http://127.0.0.1:4100",
            &manifest,
            Vec::new(),
            &[],
            &[],
            &[],
        );

        assert_eq!(receipt["service"]["name"], json!("api"));
        assert_eq!(receipt["deployment"]["target"], json!("container-paas"));
        assert_eq!(
            receipt["compatibility"]["providerProtocolVersion"],
            json!("1")
        );
    }

    #[test]
    fn service_manifest_modules_become_service_module_manifests() {
        let manifest = validate_service_manifest(json!({
            "compatibility": { "providerProtocolVersion": "1" },
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
        assert_eq!(modules[0]["source"], json!("service"));
        assert_eq!(modules[0]["version"], json!("0.1.0"));
        assert_eq!(
            modules[0]["compatibility"]["providerProtocolVersion"],
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

        add_service_module(
            &package_dir
                .join("lenso.service-package.json")
                .to_string_lossy(),
            ServiceModuleInstallOptions {
                allow_incompatible: false,
                base_url: Some("http://127.0.0.1:4110/lenso/service/v1".to_owned()),
                catalog_url: None,
                dry_run: false,
                env_file: None,
                install_profiles: Vec::new(),
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                run_install_commands: false,
                source: "service".to_owned(),
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
            ServiceModuleInstallOptions {
                allow_incompatible: false,
                base_url: Some("http://127.0.0.1:4110/lenso/service/v1".to_owned()),
                catalog_url: None,
                dry_run: false,
                env_file: None,
                install_profiles: Vec::new(),
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                run_install_commands: false,
                source: "service".to_owned(),
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
    async fn module_install_resolves_service_release_from_catalog_url() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-module-release-registry-{}",
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
        let catalog_path = repo_root.join("official-catalog.json");
        write_json(
            &catalog_path,
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
            ServiceModuleInstallOptions {
                allow_incompatible: false,
                base_url: None,
                catalog_url: Some(catalog_path.to_string_lossy().to_string()),
                dry_run: false,
                env_file: None,
                install_profiles: Vec::new(),
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                run_install_commands: false,
                source: "service".to_owned(),
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
    async fn module_install_resolves_linked_module_from_official_catalog() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-official-catalog-install-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(repo_root.join("src")).unwrap();
        write_file(
            &repo_root.join("Cargo.toml"),
            b"[package]\nname = \"host\"\n\n[dependencies]\nlenso = { version = \"0.3.16\", features = [\"host\"] }\n",
        )
        .unwrap();
        write_file(
            &repo_root.join("src/lib.rs"),
            b"mod modules;\n\nuse lenso::host::prelude::*;\n\npub fn host_composition() -> HostComposition {\n    HostBuilder::new()\n        .linked_module(modules::app::linked_module())\n        .build()\n}\n",
        )
        .unwrap();
        let catalog_path = repo_root.join("official-catalog.json");
        write_json(
            &catalog_path,
            &json!({
                "version": 1,
                "modules": [
                    {
                        "name": "auth-github",
                        "version": "0.1.0",
                        "source": "linked",
                        "manifestReference": "builtin:auth-github",
                        "dependencies": ["auth", "auth-oauth"]
                    }
                ]
            }),
        )
        .unwrap();

        install_module(
            "auth-github",
            ServiceModuleInstallOptions {
                allow_incompatible: false,
                base_url: None,
                catalog_url: Some(catalog_path.to_string_lossy().to_string()),
                dry_run: false,
                env_file: None,
                install_profiles: Vec::new(),
                module_services_file: None,
                repo_root: Some(repo_root.clone()),
                run_install_commands: false,
                source: "service".to_owned(),
            },
        )
        .await
        .unwrap();

        let env = read_text(&repo_root.join(".env")).unwrap();
        assert!(env.contains("LENSO_MODULE_AUTH_ENABLED=true"));
        assert!(env.contains("LENSO_MODULE_AUTH_OAUTH_ENABLED=true"));
        assert!(env.contains("LENSO_MODULE_AUTH_GITHUB_ENABLED=true"));
        let cargo_toml = read_text(&repo_root.join("Cargo.toml")).unwrap();
        assert!(cargo_toml.contains("lenso-module-auth-oauth = \"0.1.0\""));
        assert!(cargo_toml.contains("lenso-module-auth-github = \"0.1.0\""));
        let host_lib = read_text(&repo_root.join("src/lib.rs")).unwrap();
        assert!(host_lib.contains(".linked_module(builtins::auth())"));
        assert!(host_lib.contains(".linked_module(auth_oauth::module::linked_module())"));
        assert!(host_lib.contains(".linked_module(auth_github::module::linked_module())"));
        let ledger = read_json(&repo_root.join(MODULE_INSTALL_LEDGER_PATH)).unwrap();
        let entry = ledger["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["moduleName"] == "auth-github")
            .unwrap();
        assert_eq!(entry["manifestReference"], json!("builtin:auth-github"));
        assert_eq!(entry["dependencies"], json!(["auth", "auth-oauth"]));
        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn auth_phone_install_orders_dependencies_and_is_idempotent() {
        let repo_root = std::env::temp_dir().join(format!(
            "lenso-auth-phone-catalog-install-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(repo_root.join("src")).unwrap();
        write_file(
            &repo_root.join("Cargo.toml"),
            b"[package]\nname = \"host\"\n\n[dependencies]\nlenso = { version = \"0.3.18\", features = [\"host\"] }\n",
        )
        .unwrap();
        write_file(
            &repo_root.join("src/lib.rs"),
            b"mod modules;\n\nuse lenso::host::prelude::*;\n\npub fn host_composition() -> HostComposition {\n    HostBuilder::new()\n        .linked_module(modules::app::linked_module())\n        .build()\n}\n",
        )
        .unwrap();
        let catalog_path = repo_root.join("official-catalog.json");
        write_json(
            &catalog_path,
            &json!({
                "version": 1,
                "modules": [{
                    "name": "auth-phone",
                    "version": "0.1.1",
                    "source": "linked",
                    "manifestReference": "builtin:auth-phone",
                    "dependencies": ["auth", "auth-password"]
                }]
            }),
        )
        .unwrap();

        for _ in 0..2 {
            install_module(
                "auth-phone",
                ServiceModuleInstallOptions {
                    allow_incompatible: false,
                    base_url: None,
                    catalog_url: Some(catalog_path.to_string_lossy().to_string()),
                    dry_run: false,
                    env_file: None,
                    install_profiles: Vec::new(),
                    module_services_file: None,
                    repo_root: Some(repo_root.clone()),
                    run_install_commands: false,
                    source: "service".to_owned(),
                },
            )
            .await
            .unwrap();
        }

        let host_lib = read_text(&repo_root.join("src/lib.rs")).unwrap();
        let auth = ".linked_module(builtins::auth())";
        let password = ".linked_module(builtins::auth_password())";
        let phone = ".linked_module(builtins::auth_phone())";
        assert_eq!(host_lib.matches(auth).count(), 1);
        assert_eq!(host_lib.matches(password).count(), 1);
        assert_eq!(host_lib.matches(phone).count(), 1);
        assert!(host_lib.find(auth) < host_lib.find(password));
        assert!(host_lib.find(password) < host_lib.find(phone));

        fs::remove_dir_all(repo_root).ok();
    }

    #[tokio::test]
    async fn official_catalog_uses_fallback_when_primary_is_challenged() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer);
                let request = String::from_utf8_lossy(&buffer);
                if request.starts_with("GET /primary") {
                    stream
                        .write_all(
                            b"HTTP/1.1 403 Forbidden\r\nContent-Type: text/html\r\nContent-Length: 14\r\n\r\nJust a moment!",
                        )
                        .unwrap();
                } else {
                    let body = json!({
                        "version": 1,
                        "modules": [
                            {
                                "name": "audit-log",
                                "version": "0.1.0",
                                "source": "linked",
                                "manifestReference": "builtin:audit-log"
                            }
                        ]
                    })
                    .to_string();
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .unwrap();
                }
            }
        });

        let target = official_catalog_install_target_from_urls(
            "audit-log",
            &format!("http://{addr}/primary"),
            &[format!("http://{addr}/fallback")],
        )
        .await
        .unwrap()
        .expect("catalog target");

        server.join().unwrap();

        match target {
            CatalogInstallTarget::Descriptor {
                descriptor,
                descriptor_reference,
                provenance: _,
            } => {
                assert_eq!(descriptor["name"], json!("audit-log"));
                assert_eq!(descriptor_reference, "builtin:audit-log");
            }
            CatalogInstallTarget::ServiceManifest { .. } => {
                panic!("expected linked descriptor");
            }
        }
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
                "protocol": "lenso.service.v1",
                "version": "0.1.0"
            }),
        )
        .unwrap();

        let result = check_service_manifest_reference(
            manifest_path.to_str().unwrap(),
            ServiceManifestCheckOptions {
                cwd: None,
                env_file: None,
                json: true,
                manifest_url: None,
                operation: None,
                ready_timeout_ms: 10_000,
                ready_url: None,
                repo_root: None,
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
    fn service_release_policy_prioritizes_blocking_and_breaking_changes() {
        let diff = json!({
            "capabilities": [
                {
                    "added": [],
                    "module": "support-ticket",
                    "removed": ["support.write"]
                }
            ],
            "compatibilityChanged": false,
            "config": {
                "added": ["support.mode"],
                "removed": []
            },
            "env": {
                "added": ["SUPPORT_API_KEY"],
                "removed": []
            },
            "modules": {
                "added": [],
                "removed": []
            },
            "operations": [
                {
                    "added": [],
                    "module": "support-ticket",
                    "removed": ["route:DELETE /tickets/{id}"]
                }
            ],
        });

        let breaking = service_release_policy_from_diff(&diff, None);
        assert_eq!(breaking["risk"], json!("breaking"));

        let blocked = service_release_policy_from_diff(&diff, Some("Remote protocol is newer"));
        assert_eq!(blocked["risk"], json!("blocked"));
        assert_eq!(blocked["issues"][0]["code"], json!("host_incompatible"));
    }

    #[test]
    fn service_check_config_summary_reports_missing_env_from_explicit_file() {
        let root = std::env::temp_dir().join(format!(
            "lenso-service-config-summary-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(".env"), "PORT=4110\n").unwrap();
        let manifest = json!({
            "env": [{ "name": "SUPPORT_API_KEY" }],
            "modules": [{ "name": "support-ticket" }],
            "name": "support-suite-provider",
            "requiredEnv": ["PORT"],
            "version": "0.1.0"
        });

        let config =
            service_check_config_summary(&manifest, Some(&root), Some(Path::new(".env"))).unwrap();

        assert_eq!(config["checked"], json!(true));
        assert_eq!(config["requiredEnv"], json!(["PORT", "SUPPORT_API_KEY"]));
        assert_eq!(config["configuredEnv"], json!(["PORT"]));
        assert_eq!(config["missingEnv"], json!(["SUPPORT_API_KEY"]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn module_install_ledger_upserts_multiple_service_modules() {
        let ledger = upsert_module_install_ledger_entry(
            json!({ "modules": [], "version": 1 }),
            json!({ "moduleName": "support-ticket", "source": "service" }),
        )
        .unwrap();
        let ledger = upsert_module_install_ledger_entry(
            ledger,
            json!({ "moduleName": "support-sla", "source": "service" }),
        )
        .unwrap();
        let ledger = upsert_module_install_ledger_entry(
            ledger,
            json!({ "moduleName": "support-ticket", "source": "service", "enabled": true }),
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
        let state = ServiceModuleServiceState {
            module_name: "support-ticket".to_owned(),
            services: vec![ServiceModuleServiceInstallSpec {
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
            ServiceModuleServiceState {
                module_name: "crm".to_owned(),
                services: vec![ServiceModuleServiceInstallSpec {
                    name: "api".to_owned(),
                    command: "pnpm dev".to_owned(),
                    cwd: None,
                    ready_url: "http://127.0.0.1:4100/readyz".to_owned(),
                    ready_timeout_ms: 10_000,
                    auto_start: true,
                }],
            },
            ServiceModuleServiceState {
                module_name: "billing".to_owned(),
                services: vec![ServiceModuleServiceInstallSpec {
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
    fn service_module_service_state_path_sanitizes_names() {
        let service = ServiceModuleServiceInstallSpec {
            name: "API Worker".to_owned(),
            command: "node server.mjs".to_owned(),
            cwd: None,
            ready_url: "http://127.0.0.1:4100/lenso/module/v1/manifest".to_owned(),
            ready_timeout_ms: 10_000,
            auto_start: true,
        };
        let path =
            service_module_service_state_path(Path::new(".lenso"), "CRM Module", &service, "lock");

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
    fn manifest_url_derives_base_url() {
        let base = derive_remote_base_url(
            None,
            "https://example.com/lenso/module/v1/manifest?debug=1#hash",
        )
        .unwrap();

        assert_eq!(base, "https://example.com/lenso/module/v1");
    }
}
