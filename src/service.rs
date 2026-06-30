use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ServiceCreateArgs, ServiceDevArgs, ServiceLanguage, ServicePackageArgs, host, module};

type PendingWrites = BTreeMap<PathBuf, String>;
const SERVICE_WORKSPACE_PROTOCOL: &str = "lenso.service-workspace.v1";
const DEFAULT_SERVICE_WORKSPACE_FILE: &str = "lenso.workspace.json";
const LEGACY_SERVICE_WORKSPACE_FILE: &str = ".lenso/services.json";
const SERVICE_WORKSPACE_CHECK_TIMEOUT_MS: u64 = 2_000;

#[derive(Debug, Clone)]
pub(crate) struct ServiceCreateOptions {
    pub(crate) dry_run: bool,
    pub(crate) lang: ServiceLanguage,
    pub(crate) name: String,
    pub(crate) no_workspace: bool,
    pub(crate) output_dir: Option<PathBuf>,
    pub(crate) port: u16,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceDevOptions {
    pub(crate) module_services_file: Option<PathBuf>,
    pub(crate) no_workspace: bool,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) separate_worker: bool,
    pub(crate) skip_db: bool,
    pub(crate) skip_migrate: bool,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServicePackageOptions {
    pub(crate) check: bool,
    pub(crate) json: bool,
    pub(crate) manifest: String,
    pub(crate) output_dir: PathBuf,
    pub(crate) service_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceWorkspaceInitOptions {
    pub(crate) force: bool,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceWorkspaceAddOptions {
    pub(crate) command: String,
    pub(crate) cwd: PathBuf,
    pub(crate) lang: ServiceLanguage,
    pub(crate) manifest: String,
    pub(crate) modules: Vec<String>,
    pub(crate) name: String,
    pub(crate) ready_url: String,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceWorkspaceListOptions {
    pub(crate) json: bool,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceWorkspaceCheckOptions {
    pub(crate) json: bool,
    pub(crate) service_name: Option<String>,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceWorkspaceExportOptions {
    pub(crate) output: Option<PathBuf>,
    pub(crate) workspace_file: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServiceWorkspaceInstallReference {
    pub(crate) base_url: Option<String>,
    pub(crate) manifest_reference: String,
}

impl From<&ServiceCreateArgs> for ServiceCreateOptions {
    fn from(args: &ServiceCreateArgs) -> Self {
        Self {
            dry_run: args.dry_run,
            lang: args.lang,
            name: args.name.clone(),
            no_workspace: args.no_workspace,
            output_dir: args.output_dir.clone(),
            port: args.port,
            workspace_file: args.workspace_file.clone(),
        }
    }
}

impl From<&ServiceDevArgs> for ServiceDevOptions {
    fn from(args: &ServiceDevArgs) -> Self {
        Self {
            module_services_file: args.module_services_file.clone(),
            no_workspace: args.no_workspace,
            repo_root: args.repo_root.clone(),
            separate_worker: args.separate_worker,
            skip_db: args.skip_db,
            skip_migrate: args.skip_migrate,
            workspace_file: args.workspace_file.clone(),
        }
    }
}

impl From<&ServicePackageArgs> for ServicePackageOptions {
    fn from(args: &ServicePackageArgs) -> Self {
        Self {
            check: args.check,
            json: args.json,
            manifest: args.manifest.clone(),
            output_dir: args.output_dir.clone(),
            service_dir: args.service_dir.clone(),
        }
    }
}

pub(crate) fn create_service(options: ServiceCreateOptions) -> Result<()> {
    match options.lang {
        ServiceLanguage::Rust => create_rust_service(options),
        ServiceLanguage::Ts => create_ts_service(options),
    }
}

pub(crate) async fn dev_service(options: ServiceDevOptions) -> Result<()> {
    let repo_root = options
        .repo_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    if !options.no_workspace {
        start_service_workspace_services(repo_root, options.workspace_file.as_deref()).await?;
    }
    module::start_declared_module_services(
        options.repo_root.as_deref(),
        options.module_services_file.as_deref(),
    )
    .await?;
    host::serve(
        options.repo_root.as_deref(),
        options.skip_db,
        options.skip_migrate,
        options.separate_worker,
    )
    .await
}

pub(crate) async fn package_service(options: ServicePackageOptions) -> Result<()> {
    let plan = service_package_plan(&options).await?;
    if !options.check {
        let manifest_source =
            serde_json::to_string_pretty(&plan.manifest).context("serialize service manifest")?;
        let package_source =
            serde_json::to_string_pretty(&service_package_manifest(&plan.metadata))
                .context("serialize service package manifest")?;
        write_file(
            &plan.service_manifest_output,
            format!("{manifest_source}\n").as_bytes(),
        )?;
        write_file(
            &plan.package_manifest_output,
            format!("{package_source}\n").as_bytes(),
        )?;
        for release in &plan.module_release_outputs {
            let contract_source =
                serde_json::to_string_pretty(&module_contract_manifest(&release.module))
                    .context("serialize module contract manifest")?;
            let release_source = serde_json::to_string_pretty(&module_release_manifest(
                &plan.metadata,
                &release.module,
            ))
            .context("serialize module release manifest")?;
            write_file(
                &release.contract_path,
                format!("{contract_source}\n").as_bytes(),
            )?;
            write_file(&release.path, format!("{release_source}\n").as_bytes())?;
        }
    }
    print_service_package_report(&plan, options.check, options.json)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServicePackageModuleMetadata {
    capabilities: Vec<String>,
    dependencies: Vec<String>,
    name: String,
    version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServicePackageMetadata {
    modules: Vec<ServicePackageModuleMetadata>,
    name: String,
    version: String,
}

#[derive(Debug)]
struct ModuleReleaseOutput {
    contract_path: PathBuf,
    module: ServicePackageModuleMetadata,
    path: PathBuf,
}

#[derive(Debug)]
struct ServicePackagePlan {
    manifest: Value,
    manifest_reference: String,
    metadata: ServicePackageMetadata,
    module_release_outputs: Vec<ModuleReleaseOutput>,
    package_dir: PathBuf,
    package_manifest_output: PathBuf,
    service_manifest_output: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceWorkspace {
    protocol: String,
    #[serde(default)]
    services: Vec<ServiceWorkspaceService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceWorkspaceService {
    name: String,
    lang: String,
    cwd: String,
    manifest: String,
    command: String,
    ready_url: String,
    #[serde(default = "default_auto_start")]
    auto_start: bool,
    #[serde(default = "default_ready_timeout_ms")]
    ready_timeout_ms: u64,
    #[serde(default)]
    modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceWorkspaceCheckReport {
    workspace_file: String,
    status: String,
    services: Vec<ServiceWorkspaceCheckService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceWorkspaceCheckService {
    name: String,
    lang: String,
    cwd: String,
    manifest: String,
    ready_url: String,
    auto_start: bool,
    modules: Vec<String>,
    cwd_exists: bool,
    manifest_reachable: bool,
    ready: bool,
    status: String,
    issues: Vec<String>,
}

pub(crate) fn init_service_workspace(options: ServiceWorkspaceInitOptions) -> Result<()> {
    let path = service_workspace_path(options.workspace_file.as_deref())?;
    if path.exists() && !options.force {
        bail!(
            "Service workspace already exists: {}. Use --force to replace it.",
            path.display()
        );
    }
    write_service_workspace(&path, &empty_service_workspace())?;
    println!("Created service workspace {}.", path.display());
    Ok(())
}

pub(crate) fn add_service_workspace_entry(options: ServiceWorkspaceAddOptions) -> Result<()> {
    let path = service_workspace_path(options.workspace_file.as_deref())?;
    let workspace_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let modules = if options.modules.is_empty() {
        vec![provided_module_name(&slugify(&options.name))]
    } else {
        options.modules
    };
    let service = ServiceWorkspaceService {
        name: slugify(&options.name),
        lang: service_language_label(options.lang).to_owned(),
        cwd: display_relative(workspace_dir, &absolutize_from(workspace_dir, &options.cwd)),
        manifest: options.manifest,
        command: options.command,
        ready_url: options.ready_url,
        auto_start: true,
        ready_timeout_ms: default_ready_timeout_ms(),
        modules,
    };
    upsert_service_workspace_service(&path, service)?;
    println!("Updated service workspace {}.", path.display());
    Ok(())
}

pub(crate) fn list_service_workspace(options: ServiceWorkspaceListOptions) -> Result<()> {
    let path = service_workspace_read_path(options.workspace_file.as_deref())?;
    let workspace = read_service_workspace(&path)?;
    if options.json {
        println!("{}", json_string_pretty(&serde_json::to_value(workspace)?)?);
        return Ok(());
    }
    if workspace.services.is_empty() {
        println!("No services in {}.", path.display());
        return Ok(());
    }
    for service in workspace.services {
        println!(
            "{}\t{}\t{}\t{}",
            service.name, service.lang, service.cwd, service.ready_url
        );
    }
    Ok(())
}

pub(crate) async fn check_service_workspace(options: ServiceWorkspaceCheckOptions) -> Result<()> {
    let path = service_workspace_read_path(options.workspace_file.as_deref())?;
    let workspace = read_service_workspace(&path)?;
    let selected_services = workspace
        .services
        .iter()
        .filter(|service| match options.service_name.as_deref() {
            Some(name) => service.name == name,
            None => true,
        })
        .collect::<Vec<_>>();
    if selected_services.is_empty() {
        if let Some(name) = options.service_name {
            bail!("Service `{name}` was not found in {}", path.display());
        }
    }

    let workspace_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut services = Vec::new();
    for service in selected_services {
        services.push(check_service_workspace_service(workspace_dir, service).await?);
    }
    let status = if services.iter().all(|service| service.status == "ready") {
        "ready"
    } else {
        "needs_attention"
    }
    .to_owned();
    let report = ServiceWorkspaceCheckReport {
        workspace_file: path_string(&path),
        status,
        services,
    };

    if options.json {
        println!("{}", json_string_pretty(&report)?);
    } else {
        print_service_workspace_check_report(&report);
    }

    if report.status != "ready" {
        bail!("Service workspace check found services that need attention");
    }
    Ok(())
}

pub(crate) fn export_service_workspace(options: ServiceWorkspaceExportOptions) -> Result<()> {
    let path = service_workspace_read_path(options.workspace_file.as_deref())?;
    let workspace = read_service_workspace(&path)?;
    let state = service_workspace_module_services_json(&workspace);
    let source = format!("{}\n", json_string_pretty(&state)?);
    if let Some(output) = options.output {
        let current_dir = std::env::current_dir().context("resolve current directory")?;
        let output = absolutize_from(&current_dir, &output);
        write_file(&output, source.as_bytes())?;
        println!("Exported service workspace state to {}.", output.display());
        return Ok(());
    }
    print!("{source}");
    Ok(())
}

async fn start_service_workspace_services(
    repo_root: &Path,
    workspace_file: Option<&Path>,
) -> Result<()> {
    let path = service_workspace_read_path_from(repo_root, workspace_file);
    if !path.exists() {
        return Ok(());
    }
    let workspace = read_service_workspace(&path)?;
    if workspace.services.is_empty() {
        return Ok(());
    }
    let state_path = absolutize_from(
        repo_root,
        Path::new(".lenso/service-workspace-services.json"),
    );
    let state = service_workspace_module_services_json(&workspace);
    write_file(&state_path, json_string_pretty(&state)?.as_bytes())?;
    module::start_declared_module_services(Some(repo_root), Some(&state_path)).await
}

async fn check_service_workspace_service(
    workspace_dir: &Path,
    service: &ServiceWorkspaceService,
) -> Result<ServiceWorkspaceCheckService> {
    let service_dir = absolutize_from(workspace_dir, Path::new(&service.cwd));
    let cwd_exists = service_dir.is_dir();
    let mut issues = Vec::new();
    if !cwd_exists {
        issues.push(format!(
            "service cwd does not exist: {}",
            service_dir.display()
        ));
    }

    let (manifest_reachable, manifest_issue) =
        check_service_manifest_reachable(&service_dir, &service.manifest).await?;
    if let Some(issue) = manifest_issue {
        issues.push(issue);
    }

    let (ready, ready_issue) = check_http_ok(&service.ready_url, workspace_check_timeout()).await?;
    if let Some(issue) = ready_issue {
        issues.push(issue);
    }

    let status = if !cwd_exists {
        "missing_cwd"
    } else if !manifest_reachable {
        "manifest_unreachable"
    } else if !ready {
        "service_not_ready"
    } else {
        "ready"
    }
    .to_owned();

    Ok(ServiceWorkspaceCheckService {
        name: service.name.clone(),
        lang: service.lang.clone(),
        cwd: path_string(&service_dir),
        manifest: service.manifest.clone(),
        ready_url: service.ready_url.clone(),
        auto_start: service.auto_start,
        modules: service.modules.clone(),
        cwd_exists,
        manifest_reachable,
        ready,
        status,
        issues,
    })
}

async fn check_service_manifest_reachable(
    service_dir: &Path,
    manifest: &str,
) -> Result<(bool, Option<String>)> {
    if is_http_reference(manifest) {
        let (ok, issue) = check_http_json(manifest, workspace_check_timeout()).await?;
        return Ok((
            ok,
            issue.map(|issue| format!("manifest unreachable: {issue}")),
        ));
    }

    let manifest_path = absolutize_from(service_dir, Path::new(manifest));
    let source = match fs::read_to_string(&manifest_path) {
        Ok(source) => source,
        Err(error) => {
            return Ok((
                false,
                Some(format!(
                    "manifest unreadable: {} ({error})",
                    manifest_path.display()
                )),
            ));
        }
    };
    if let Err(error) = serde_json::from_str::<Value>(&source) {
        return Ok((
            false,
            Some(format!(
                "manifest is not valid JSON: {} ({error})",
                manifest_path.display()
            )),
        ));
    }
    Ok((true, None))
}

async fn check_http_json(url: &str, timeout: Duration) -> Result<(bool, Option<String>)> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("build service workspace check client")?;
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) => return Ok((false, Some(error.to_string()))),
    };
    let status = response.status();
    if !status.is_success() {
        return Ok((false, Some(format!("HTTP {status}"))));
    }
    match response.json::<Value>().await {
        Ok(_) => Ok((true, None)),
        Err(error) => Ok((false, Some(format!("invalid JSON response ({error})")))),
    }
}

async fn check_http_ok(url: &str, timeout: Duration) -> Result<(bool, Option<String>)> {
    if !is_http_reference(url) {
        return Ok((false, Some(format!("ready URL is not HTTP: {url}"))));
    }
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .context("build service workspace check client")?;
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) => return Ok((false, Some(format!("ready URL failed: {error}")))),
    };
    let status = response.status();
    if status.is_success() {
        Ok((true, None))
    } else {
        Ok((false, Some(format!("ready URL returned HTTP {status}"))))
    }
}

fn workspace_check_timeout() -> Duration {
    Duration::from_millis(SERVICE_WORKSPACE_CHECK_TIMEOUT_MS)
}

fn print_service_workspace_check_report(report: &ServiceWorkspaceCheckReport) {
    println!("Service workspace: {}", report.workspace_file);
    if report.services.is_empty() {
        println!("No services declared.");
        return;
    }
    for service in &report.services {
        println!("{}: {}", service.name, service.status);
        println!("  lang: {}", service.lang);
        println!("  cwd: {}", service.cwd);
        println!("  manifest: {}", service.manifest);
        println!("  readyUrl: {}", service.ready_url);
        if !service.modules.is_empty() {
            println!("  modules: {}", service.modules.join(", "));
        }
        for issue in &service.issues {
            println!("  issue: {issue}");
        }
    }
}

fn service_workspace_module_services_json(workspace: &ServiceWorkspace) -> Value {
    serde_json::json!({
        "version": 1,
        "modules": workspace.services.iter().map(|service| {
            serde_json::json!({
                "moduleName": service.name,
                "services": [
                    {
                        "name": service.name,
                        "command": service.command,
                        "cwd": service.cwd,
                        "readyUrl": service.ready_url,
                        "autoStart": service.auto_start,
                        "readyTimeoutMs": service.ready_timeout_ms,
                    }
                ],
            })
        }).collect::<Vec<_>>(),
    })
}

fn queue_service_workspace_update(
    pending_writes: &mut PendingWrites,
    scaffold: &ServiceScaffold,
    options: &ServiceCreateOptions,
    lang: &str,
    command: &str,
) -> Result<()> {
    if options.no_workspace {
        return Ok(());
    }
    let path = service_workspace_path(options.workspace_file.as_deref())?;
    let workspace_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut workspace = read_service_workspace(&path)?;
    upsert_service_workspace(
        &mut workspace,
        ServiceWorkspaceService {
            name: scaffold.service_name.clone(),
            lang: lang.to_owned(),
            cwd: display_relative(workspace_dir, &scaffold.target_dir),
            manifest: "lenso.service.json".to_owned(),
            command: command.to_owned(),
            ready_url: scaffold.service_status_url.clone(),
            auto_start: true,
            ready_timeout_ms: default_ready_timeout_ms(),
            modules: vec![scaffold.module_name.clone()],
        },
    );
    pending_writes.insert(path, format!("{}\n", json_string_pretty(&workspace)?));
    Ok(())
}

fn upsert_service_workspace_service(path: &Path, service: ServiceWorkspaceService) -> Result<()> {
    let mut workspace = read_service_workspace(path)?;
    upsert_service_workspace(&mut workspace, service);
    write_service_workspace(path, &workspace)
}

fn upsert_service_workspace(workspace: &mut ServiceWorkspace, service: ServiceWorkspaceService) {
    if let Some(existing) = workspace
        .services
        .iter_mut()
        .find(|existing| existing.name == service.name)
    {
        *existing = service;
        return;
    }
    workspace.services.push(service);
    workspace
        .services
        .sort_by(|left, right| left.name.cmp(&right.name));
}

fn read_service_workspace(path: &Path) -> Result<ServiceWorkspace> {
    if !path.exists() {
        return Ok(empty_service_workspace());
    }
    let source = fs::read_to_string(path)
        .with_context(|| format!("read service workspace {}", path.display()))?;
    let workspace: ServiceWorkspace = serde_json::from_str(&source)
        .with_context(|| format!("parse service workspace {}", path.display()))?;
    if workspace.protocol != SERVICE_WORKSPACE_PROTOCOL {
        bail!(
            "Service workspace {} uses unsupported protocol `{}`",
            path.display(),
            workspace.protocol
        );
    }
    Ok(workspace)
}

fn write_service_workspace(path: &Path, workspace: &ServiceWorkspace) -> Result<()> {
    write_file(
        path,
        format!("{}\n", json_string_pretty(workspace)?).as_bytes(),
    )
}

fn empty_service_workspace() -> ServiceWorkspace {
    ServiceWorkspace {
        protocol: SERVICE_WORKSPACE_PROTOCOL.to_owned(),
        services: Vec::new(),
    }
}

fn service_workspace_path(workspace_file: Option<&Path>) -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    Ok(service_workspace_path_from(&current_dir, workspace_file))
}

fn service_workspace_read_path(workspace_file: Option<&Path>) -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    Ok(service_workspace_read_path_from(
        &current_dir,
        workspace_file,
    ))
}

fn service_workspace_path_from(base: &Path, workspace_file: Option<&Path>) -> PathBuf {
    absolutize_from(
        base,
        workspace_file.unwrap_or_else(|| Path::new(DEFAULT_SERVICE_WORKSPACE_FILE)),
    )
}

fn service_workspace_read_path_from(base: &Path, workspace_file: Option<&Path>) -> PathBuf {
    if workspace_file.is_some() {
        return service_workspace_path_from(base, workspace_file);
    }
    let default_path = service_workspace_path_from(base, None);
    if default_path.exists() {
        return default_path;
    }
    let legacy_path = absolutize_from(base, Path::new(LEGACY_SERVICE_WORKSPACE_FILE));
    if legacy_path.exists() {
        return legacy_path;
    }
    default_path
}

pub(crate) fn infer_workspace_base_url_for_manifest(
    manifest_reference: &str,
    repo_root: Option<&Path>,
    workspace_file: Option<&Path>,
) -> Result<Option<String>> {
    if is_http_reference(manifest_reference) {
        return Ok(None);
    }
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let repo_root = repo_root
        .map(|path| absolutize_from(&current_dir, path))
        .unwrap_or_else(|| current_dir.clone());
    let workspace_path = service_workspace_read_path_from(&repo_root, workspace_file);
    if !workspace_path.exists() {
        return Ok(None);
    }

    let workspace = read_service_workspace(&workspace_path)?;
    let workspace_dir = workspace_path.parent().unwrap_or_else(|| Path::new("."));
    let requested_manifest = comparable_path(&absolutize_from(
        &current_dir,
        Path::new(manifest_reference),
    ));
    for service in &workspace.services {
        if is_http_reference(&service.manifest) {
            continue;
        }
        let service_manifest = comparable_path(&service_workspace_manifest_path(
            workspace_dir,
            &service.cwd,
            &service.manifest,
        ));
        if service_manifest == requested_manifest {
            return Ok(service_workspace_base_url(service));
        }
    }

    Ok(None)
}

pub(crate) fn resolve_workspace_install_reference(
    reference: &str,
    repo_root: Option<&Path>,
    workspace_file: Option<&Path>,
) -> Result<Option<ServiceWorkspaceInstallReference>> {
    if is_http_reference(reference) {
        return Ok(None);
    }
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let repo_root = repo_root
        .map(|path| absolutize_from(&current_dir, path))
        .unwrap_or_else(|| current_dir.clone());
    let workspace_path = service_workspace_read_path_from(&repo_root, workspace_file);
    if !workspace_path.exists() {
        return Ok(None);
    }

    let workspace = read_service_workspace(&workspace_path)?;
    let Some(service) = workspace
        .services
        .iter()
        .find(|service| service.name == reference)
    else {
        return Ok(None);
    };

    let workspace_dir = workspace_path.parent().unwrap_or_else(|| Path::new("."));
    let manifest_reference = if is_http_reference(&service.manifest) {
        service.manifest.clone()
    } else {
        path_string(&service_workspace_manifest_path(
            workspace_dir,
            &service.cwd,
            &service.manifest,
        ))
    };
    Ok(Some(ServiceWorkspaceInstallReference {
        base_url: service_workspace_base_url(service),
        manifest_reference,
    }))
}

fn service_workspace_manifest_path(
    workspace_dir: &Path,
    service_cwd: &str,
    manifest: &str,
) -> PathBuf {
    let service_dir = absolutize_from(workspace_dir, Path::new(service_cwd));
    absolutize_from(&service_dir, Path::new(manifest))
}

fn service_workspace_base_url(service: &ServiceWorkspaceService) -> Option<String> {
    service_base_url_from_ready_url(&service.ready_url)
        .or_else(|| service_manifest_base_url(&service.manifest))
}

fn service_manifest_base_url(manifest: &str) -> Option<String> {
    if !is_http_reference(manifest) {
        return None;
    }
    let mut url = reqwest::Url::parse(manifest).ok()?;
    let path = url.path().trim_end_matches('/').to_owned();
    let base_path = path.strip_suffix("/manifest")?.to_owned();
    url.set_path(&base_path);
    url.set_query(None);
    url.set_fragment(None);
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn service_base_url_from_ready_url(ready_url: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(ready_url).ok()?;
    let path = url.path().trim_end_matches('/').to_owned();
    let base_path = ["/status", "/ready", "/health", "/healthz"]
        .iter()
        .find_map(|suffix| path.strip_suffix(suffix))
        .map(ToOwned::to_owned)?;
    url.set_path(&base_path);
    url.set_query(None);
    url.set_fragment(None);
    Some(url.as_str().trim_end_matches('/').to_owned())
}

fn comparable_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn service_language_label(lang: ServiceLanguage) -> &'static str {
    match lang {
        ServiceLanguage::Rust => "rust",
        ServiceLanguage::Ts => "ts",
    }
}

const fn default_auto_start() -> bool {
    true
}

const fn default_ready_timeout_ms() -> u64 {
    10_000
}

async fn service_package_plan(options: &ServicePackageOptions) -> Result<ServicePackagePlan> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let service_dir = absolutize_from(&current_dir, &options.service_dir);
    if !service_dir.is_dir() {
        bail!(
            "Service provider directory does not exist: {}",
            service_dir.display()
        );
    }
    let (manifest, manifest_reference) =
        read_service_package_manifest(&options.manifest, &service_dir).await?;
    let metadata = service_package_metadata(&manifest)?;
    let output_dir = absolutize_from(&service_dir, &options.output_dir);
    let package_dir = output_dir.join(&metadata.name);
    let module_release_outputs = metadata
        .modules
        .iter()
        .map(|module| {
            let module_dir = package_dir
                .join("modules")
                .join(module_release_path_segment(&module.name)?);
            Ok(ModuleReleaseOutput {
                contract_path: module_dir.join("lenso.module.json"),
                module: module.clone(),
                path: module_dir.join("lenso.module-release.json"),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ServicePackagePlan {
        manifest,
        manifest_reference,
        package_manifest_output: package_dir.join("lenso.service-package.json"),
        service_manifest_output: package_dir.join("lenso.service.json"),
        module_release_outputs,
        package_dir,
        metadata,
    })
}

async fn read_service_package_manifest(
    reference: &str,
    service_dir: &Path,
) -> Result<(Value, String)> {
    if is_http_reference(reference) {
        let response = reqwest::get(reference)
            .await
            .with_context(|| format!("fetch service manifest {reference}"))?
            .error_for_status()
            .with_context(|| format!("fetch service manifest {reference}"))?;
        let manifest = response
            .json::<Value>()
            .await
            .with_context(|| format!("parse service manifest {reference}"))?;
        return Ok((manifest, reference.to_owned()));
    }

    let manifest_path = absolutize_from(service_dir, Path::new(reference));
    let manifest_source = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read service manifest {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&manifest_source)
        .with_context(|| format!("parse service manifest {}", manifest_path.display()))?;
    Ok((manifest, path_string(&manifest_path)))
}

fn is_http_reference(reference: &str) -> bool {
    reference.starts_with("http://") || reference.starts_with("https://")
}

fn service_package_metadata(manifest: &Value) -> Result<ServicePackageMetadata> {
    if !manifest.is_object() {
        bail!("Service manifest must be a JSON object");
    }
    let name = required_manifest_string(manifest, "name", "Service manifest name")?;
    let version = required_manifest_string(manifest, "version", "Service manifest version")?;
    let Some(raw_modules) = manifest.get("modules").and_then(Value::as_array) else {
        bail!("Service manifest modules must be an array");
    };
    if raw_modules.is_empty() {
        bail!("Service manifest modules must not be empty");
    }

    let mut seen_modules = BTreeSet::new();
    let mut modules = Vec::new();
    for module in raw_modules {
        if !module.is_object() {
            bail!("Service manifest module entries must be objects");
        }
        let module_name = required_manifest_string(module, "name", "Service manifest module name")?;
        if !seen_modules.insert(module_name.to_owned()) {
            bail!("Service manifest module `{module_name}` is declared more than once");
        }
        let version = module
            .get("version")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(version)
            .to_owned();
        modules.push(ServicePackageModuleMetadata {
            capabilities: optional_manifest_string_array(module, "capabilities")?,
            dependencies: optional_manifest_string_array(module, "dependencies")?,
            name: module_name.to_owned(),
            version,
        });
    }

    Ok(ServicePackageMetadata {
        modules,
        name: name.to_owned(),
        version: version.to_owned(),
    })
}

fn optional_manifest_string_array(value: &Value, field: &str) -> Result<Vec<String>> {
    let Some(raw_values) = value.get(field) else {
        return Ok(Vec::new());
    };
    let Some(values) = raw_values.as_array() else {
        bail!("Service manifest module {field} must be an array");
    };
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    anyhow::anyhow!("Service manifest module {field}[{index}] is required")
                })
        })
        .collect()
}

fn required_manifest_string<'a>(value: &'a Value, field: &str, label: &str) -> Result<&'a str> {
    let Some(raw_value) = value.get(field).and_then(Value::as_str) else {
        bail!("{label} is required");
    };
    let value = raw_value.trim();
    if value.is_empty() {
        bail!("{label} is required");
    }
    Ok(value)
}

fn service_package_manifest(metadata: &ServicePackageMetadata) -> Value {
    serde_json::json!({
        "protocol": "lenso.service-package.v1",
        "name": metadata.name,
        "version": metadata.version,
        "serviceManifest": "lenso.service.json",
        "modules": module_names(&metadata.modules),
    })
}

fn module_contract_manifest(module: &ServicePackageModuleMetadata) -> Value {
    serde_json::json!({
        "protocol": "lenso.module.v1",
        "name": module.name,
        "version": module.version,
        "source": "service",
        "summary": format!("{} module", module.name),
        "capabilities": module.capabilities,
        "dependencies": module.dependencies,
    })
}

fn module_release_manifest(
    metadata: &ServicePackageMetadata,
    module: &ServicePackageModuleMetadata,
) -> Value {
    serde_json::json!({
        "protocol": "lenso.module-release.v1",
        "name": module.name,
        "version": module.version,
        "source": "service",
        "summary": format!("{} module from {}", module.name, metadata.name),
        "provider": {
            "name": metadata.name,
            "servicePackage": "../../lenso.service-package.json"
        },
        "capabilities": module.capabilities,
        "dependencies": module.dependencies,
    })
}

fn module_names(modules: &[ServicePackageModuleMetadata]) -> Vec<String> {
    modules.iter().map(|module| module.name.clone()).collect()
}

fn module_release_path_segment(module_name: &str) -> Result<&str> {
    if module_name.contains('/')
        || module_name.contains('\\')
        || module_name == "."
        || module_name == ".."
    {
        bail!("Service manifest module `{module_name}` cannot be used as a release path");
    }
    Ok(module_name)
}

fn print_service_package_report(plan: &ServicePackagePlan, check: bool, json: bool) -> Result<()> {
    if json {
        let status = if check { "checked" } else { "packaged" };
        let mut files = vec![
            path_string(&plan.service_manifest_output),
            path_string(&plan.package_manifest_output),
        ];
        files.extend(
            plan.module_release_outputs
                .iter()
                .flat_map(|release| {
                    [
                        path_string(&release.contract_path),
                        path_string(&release.path),
                    ]
                })
                .collect::<Vec<_>>(),
        );
        let module_releases = plan
            .module_release_outputs
            .iter()
            .map(|release| {
                serde_json::json!({
                    "contractPath": path_string(&release.contract_path),
                    "module": release.module.name,
                    "path": path_string(&release.path),
                })
            })
            .collect::<Vec<_>>();
        let report = serde_json::json!({
            "status": status,
            "name": plan.metadata.name,
            "version": plan.metadata.version,
            "modules": module_names(&plan.metadata.modules),
            "manifestReference": plan.manifest_reference,
            "packageDir": path_string(&plan.package_dir),
            "moduleReleases": module_releases,
            "files": files,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&report).context("serialize service package report")?
        );
        return Ok(());
    }

    if check {
        println!("Service package check ok.");
    } else {
        println!(
            "Packaged service {}@{}.",
            plan.metadata.name, plan.metadata.version
        );
    }
    println!(
        "- modules: {}",
        module_names(&plan.metadata.modules).join(", ")
    );
    println!("- manifest: {}", plan.manifest_reference);
    println!("- package: {}", plan.package_dir.display());
    if !check {
        println!("- wrote: {}", plan.service_manifest_output.display());
        println!("- wrote: {}", plan.package_manifest_output.display());
        for release in &plan.module_release_outputs {
            println!("- wrote: {}", release.contract_path.display());
            println!("- wrote: {}", release.path.display());
        }
    }
    Ok(())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn create_ts_service(options: ServiceCreateOptions) -> Result<()> {
    let scaffold = service_scaffold(&options)?;
    let mut pending_writes = PendingWrites::new();
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("package.json"),
        include_str!("../templates/service-ts/package.json.tmpl"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("pnpm-workspace.yaml"),
        include_str!("../templates/service-ts/pnpm-workspace.yaml.tmpl"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("src/server.ts"),
        include_str!("../templates/service-ts/src/server.ts"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("src/service.ts"),
        include_str!("../templates/service-ts/src/service.ts"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("lenso.service.json"),
        include_str!("../templates/service-ts/lenso.service.json.tmpl"),
        &scaffold,
    );
    queue_service_workspace_update(&mut pending_writes, &scaffold, &options, "ts", "pnpm start")?;
    finish_service_create(&scaffold, pending_writes, options.dry_run, "pnpm install")
}

fn create_rust_service(options: ServiceCreateOptions) -> Result<()> {
    let scaffold = service_scaffold(&options)?;
    let mut pending_writes = PendingWrites::new();
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("Cargo.toml"),
        include_str!("../templates/service-rust/Cargo.toml.tmpl"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("src/main.rs"),
        include_str!("../templates/service-rust/src/main.rs"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("lenso.service.json"),
        include_str!("../templates/service-rust/lenso.service.json.tmpl"),
        &scaffold,
    );
    queue_service_workspace_update(
        &mut pending_writes,
        &scaffold,
        &options,
        "rust",
        "cargo run",
    )?;
    finish_service_create(&scaffold, pending_writes, options.dry_run, "cargo check")
}

fn finish_service_create(
    scaffold: &ServiceScaffold,
    pending_writes: PendingWrites,
    dry_run: bool,
    check_command: &str,
) -> Result<()> {
    if dry_run {
        println!("Service dry run:");
        for path in pending_writes.keys() {
            println!("- {}", display_relative(&scaffold.output_root, path));
        }
        return Ok(());
    }

    write_pending_files(&pending_writes)?;
    println!("Created service {}.", scaffold.service_name);
    println!("Next steps:");
    println!("- cd {}", scaffold.target_dir_display);
    println!("- {check_command}");
    println!("- lenso service verify");
    println!("- lenso service package --check");
    println!(
        "- {}",
        local_service_install_command(&scaffold.service_name, &scaffold.repo_root_display)
    );
    println!(
        "- lenso service release plan {} ./dist/lenso-service/{}/lenso.service-package.json --repo-root {} --output .lenso/{}.release-plan.json",
        scaffold.service_name,
        scaffold.service_name,
        scaffold.repo_root_display,
        scaffold.service_name
    );
    println!(
        "- lenso service policy check .lenso/{}.release-plan.json --fail-on breaking",
        scaffold.service_name
    );
    println!(
        "- lenso service release apply .lenso/{}.release-plan.json --repo-root {}",
        scaffold.service_name, scaffold.repo_root_display
    );
    if let Some(note) = &scaffold.publish_note {
        println!("- {note}");
    }
    Ok(())
}

#[derive(Debug)]
struct ServiceScaffold {
    crate_name: String,
    lenso_service_dependency: String,
    local_service_base_url: String,
    module_name: String,
    output_root: PathBuf,
    package_name: String,
    pnpm_workspace_overrides: String,
    publish_note: Option<String>,
    remote_module_kit_dependency: String,
    repo_root_display: String,
    service_cwd: String,
    service_kit_dependency: String,
    service_label: String,
    service_name: String,
    service_port: u16,
    service_status_url: String,
    target_dir: PathBuf,
    target_dir_display: String,
}

fn service_scaffold(options: &ServiceCreateOptions) -> Result<ServiceScaffold> {
    let service_name = slugify(&options.name);
    if service_name.is_empty() {
        bail!("Service name is required");
    }
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let output_root = options
        .output_dir
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| current_dir.clone());
    let output_root = absolutize_from(&current_dir, &output_root);
    let target_dir = output_root.join(&service_name);
    if target_dir.exists() {
        bail!("Service directory already exists: {}", target_dir.display());
    }
    let module_name = provided_module_name(&service_name);
    let dependencies = service_dependencies();
    let local_service_base_url = format!("http://127.0.0.1:{}/lenso/service/v1", options.port);
    Ok(ServiceScaffold {
        crate_name: snake_case(&service_name),
        lenso_service_dependency: dependencies.lenso_service_dependency,
        local_service_base_url: local_service_base_url.clone(),
        module_name: module_name.clone(),
        output_root,
        package_name: service_name.clone(),
        pnpm_workspace_overrides: dependencies.pnpm_workspace_overrides,
        publish_note: dependencies.publish_note,
        remote_module_kit_dependency: dependencies.remote_module_kit_dependency,
        repo_root_display: current_dir.to_string_lossy().to_string(),
        service_cwd: json_string(&display_relative(&current_dir, &target_dir)),
        service_kit_dependency: dependencies.service_kit_dependency,
        service_label: label_from_slug(&module_name),
        service_name,
        service_port: options.port,
        service_status_url: format!("{local_service_base_url}/status"),
        target_dir_display: display_relative(&current_dir, &target_dir),
        target_dir,
    })
}

fn queue_template(
    pending_writes: &mut PendingWrites,
    path: PathBuf,
    template: &str,
    scaffold: &ServiceScaffold,
) {
    let contents = render_template(template, scaffold);
    pending_writes.insert(path, contents);
}

fn render_template(template: &str, scaffold: &ServiceScaffold) -> String {
    template
        .replace("{{service_name}}", &scaffold.service_name)
        .replace("{{service_label}}", &scaffold.service_label)
        .replace("{{service_port}}", &scaffold.service_port.to_string())
        .replace(
            "{{local_service_base_url}}",
            &scaffold.local_service_base_url,
        )
        .replace("{{service_status_url}}", &scaffold.service_status_url)
        .replace("{{module_name}}", &scaffold.module_name)
        .replace("{{package_name}}", &scaffold.package_name)
        .replace("{{crate_name}}", &scaffold.crate_name)
        .replace("{{repo_root_display}}", &scaffold.repo_root_display)
        .replace(
            "{{service_kit_dependency}}",
            &scaffold.service_kit_dependency,
        )
        .replace(
            "{{remote_module_kit_dependency}}",
            &scaffold.remote_module_kit_dependency,
        )
        .replace("{{service_cwd}}", &scaffold.service_cwd)
        .replace(
            "{{pnpm_workspace_overrides}}",
            &scaffold.pnpm_workspace_overrides,
        )
        .replace(
            "{{lenso_service_dependency}}",
            &scaffold.lenso_service_dependency,
        )
}

#[derive(Debug)]
struct ServiceDependencyPlan {
    lenso_service_dependency: String,
    pnpm_workspace_overrides: String,
    publish_note: Option<String>,
    remote_module_kit_dependency: String,
    service_kit_dependency: String,
}

fn service_dependencies() -> ServiceDependencyPlan {
    let Some(framework_root) = find_framework_root() else {
        return ServiceDependencyPlan {
            lenso_service_dependency: "lenso-service = \"0.1.0\"".to_owned(),
            pnpm_workspace_overrides: String::new(),
            publish_note: Some(
                "@lenso/service-kit and lenso-service must be published, or replace dependencies with local paths.".to_owned(),
            ),
            remote_module_kit_dependency: json_string("0.1.3"),
            service_kit_dependency: json_string("0.1.0"),
        };
    };

    let service_kit = framework_root.join("lenso-runtime-console/packages/service-kit");
    let remote_module_kit = framework_root.join("lenso-runtime-console/packages/remote-module-kit");
    let lenso_service = framework_root.join("lenso/crates/lenso-service");
    ServiceDependencyPlan {
        lenso_service_dependency: format!(
            "lenso-service = {{ path = \"{}\" }}",
            toml_string(&lenso_service)
        ),
        pnpm_workspace_overrides: format!(
            "overrides:\n  \"@lenso/remote-module-kit\": {}\n",
            json_string(&format!("file:{}", remote_module_kit.display()))
        ),
        publish_note: None,
        remote_module_kit_dependency: json_string(&format!("file:{}", remote_module_kit.display())),
        service_kit_dependency: json_string(&format!("file:{}", service_kit.display())),
    }
}

fn find_framework_root() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        if current
            .join("lenso-runtime-console/packages/service-kit")
            .is_dir()
            && current.join("lenso/crates/lenso-service").is_dir()
        {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization is infallible")
}

fn json_string_pretty<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).context("serialize JSON")
}

fn toml_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn local_service_install_command(service_name: &str, repo_root: &str) -> String {
    format!(
        "lenso service install {} --repo-root {}",
        shell_arg(service_name),
        shell_arg(repo_root)
    )
}

fn shell_arg(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "/._-:".contains(character))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn provided_module_name(service_name: &str) -> String {
    service_name
        .strip_suffix("-provider")
        .or_else(|| service_name.strip_suffix("-service"))
        .filter(|name| !name.is_empty())
        .unwrap_or(service_name)
        .to_owned()
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

fn label_from_slug(value: &str) -> String {
    value
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn absolutize_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn display_relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn write_pending_files(pending_writes: &PendingWrites) -> Result<()> {
    for (path, contents) in pending_writes {
        write_file(path, contents.as_bytes())?;
    }
    Ok(())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn test_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lenso-cli-service-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn scaffold() -> ServiceScaffold {
        ServiceScaffold {
            crate_name: "support_suite_provider".to_owned(),
            lenso_service_dependency: "lenso-service = \"0.1.0\"".to_owned(),
            local_service_base_url: "http://127.0.0.1:4110/lenso/service/v1".to_owned(),
            module_name: "support-suite".to_owned(),
            output_root: PathBuf::from("/tmp/services"),
            package_name: "support-suite-provider".to_owned(),
            pnpm_workspace_overrides: String::new(),
            publish_note: None,
            remote_module_kit_dependency: json_string("0.1.3"),
            repo_root_display: "/tmp/host".to_owned(),
            service_cwd: json_string("../services/support-suite-provider"),
            service_kit_dependency: json_string("0.1.0"),
            service_label: "Support Suite".to_owned(),
            service_name: "support-suite-provider".to_owned(),
            service_port: 4110,
            service_status_url: "http://127.0.0.1:4110/lenso/service/v1/status".to_owned(),
            target_dir: PathBuf::from("/tmp/services/support-suite-provider"),
            target_dir_display: "/tmp/services/support-suite-provider".to_owned(),
        }
    }

    #[test]
    fn install_command_uses_service_name_and_repo_root() {
        let scaffold = scaffold();
        let command =
            local_service_install_command(&scaffold.service_name, &scaffold.repo_root_display);

        assert_eq!(
            command,
            "lenso service install support-suite-provider --repo-root /tmp/host"
        );
        assert!(!command.contains("--base-url"));
    }

    #[test]
    fn service_workspace_read_path_falls_back_to_lenso_services_json() {
        let root = test_dir("workspace-fallback");
        let legacy = root.join(".lenso/services.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}").unwrap();

        assert_eq!(service_workspace_read_path_from(&root, None), legacy);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn service_workspace_base_url_infers_from_local_manifest_path() {
        let root = test_dir("workspace-base-url");
        let service_dir = root.join("services/support-suite-provider");
        fs::create_dir_all(&service_dir).unwrap();
        let manifest = service_dir.join("lenso.service.json");
        fs::write(&manifest, "{}").unwrap();
        write_service_workspace(
            &root.join("lenso.workspace.json"),
            &ServiceWorkspace {
                protocol: SERVICE_WORKSPACE_PROTOCOL.to_owned(),
                services: vec![ServiceWorkspaceService {
                    auto_start: true,
                    command: "pnpm start".to_owned(),
                    cwd: "services/support-suite-provider".to_owned(),
                    lang: "ts".to_owned(),
                    manifest: "lenso.service.json".to_owned(),
                    modules: vec!["support-suite".to_owned()],
                    name: "support-suite-provider".to_owned(),
                    ready_timeout_ms: 10_000,
                    ready_url: "http://127.0.0.1:4110/lenso/service/v1/status".to_owned(),
                }],
            },
        )
        .unwrap();

        let inferred =
            infer_workspace_base_url_for_manifest(&manifest.to_string_lossy(), Some(&root), None)
                .unwrap();

        assert_eq!(
            inferred,
            Some("http://127.0.0.1:4110/lenso/service/v1".to_owned())
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn service_workspace_install_reference_resolves_service_name() {
        let root = test_dir("workspace-install-reference");
        let service_dir = root.join("services/support-suite-provider");
        fs::create_dir_all(&service_dir).unwrap();
        let manifest = service_dir.join("lenso.service.json");
        fs::write(&manifest, "{}").unwrap();
        write_service_workspace(
            &root.join("lenso.workspace.json"),
            &ServiceWorkspace {
                protocol: SERVICE_WORKSPACE_PROTOCOL.to_owned(),
                services: vec![ServiceWorkspaceService {
                    auto_start: true,
                    command: "pnpm start".to_owned(),
                    cwd: "services/support-suite-provider".to_owned(),
                    lang: "ts".to_owned(),
                    manifest: "lenso.service.json".to_owned(),
                    modules: vec!["support-suite".to_owned()],
                    name: "support-suite-provider".to_owned(),
                    ready_timeout_ms: 10_000,
                    ready_url: "http://127.0.0.1:4110/lenso/service/v1/status".to_owned(),
                }],
            },
        )
        .unwrap();

        let resolved =
            resolve_workspace_install_reference("support-suite-provider", Some(&root), None)
                .unwrap()
                .unwrap();

        assert_eq!(resolved.manifest_reference, path_string(&manifest));
        assert_eq!(
            resolved.base_url.as_deref(),
            Some("http://127.0.0.1:4110/lenso/service/v1")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn export_service_workspace_writes_module_service_state() {
        let root = test_dir("workspace-export");
        let workspace_path = root.join("lenso.workspace.json");
        let output = root.join(".lenso/module-services.json");
        write_service_workspace(
            &workspace_path,
            &ServiceWorkspace {
                protocol: SERVICE_WORKSPACE_PROTOCOL.to_owned(),
                services: vec![ServiceWorkspaceService {
                    auto_start: true,
                    command: "pnpm start".to_owned(),
                    cwd: "services/support-suite-provider".to_owned(),
                    lang: "ts".to_owned(),
                    manifest: "lenso.service.json".to_owned(),
                    modules: vec!["support-suite".to_owned()],
                    name: "support-suite-provider".to_owned(),
                    ready_timeout_ms: 10_000,
                    ready_url: "http://127.0.0.1:4110/lenso/service/v1/status".to_owned(),
                }],
            },
        )
        .unwrap();

        export_service_workspace(ServiceWorkspaceExportOptions {
            output: Some(output.clone()),
            workspace_file: Some(workspace_path),
        })
        .unwrap();

        let value: Value = serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["modules"][0]["moduleName"], "support-suite-provider");
        assert_eq!(
            value["modules"][0]["services"][0]["readyUrl"],
            "http://127.0.0.1:4110/lenso/service/v1/status"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn service_workspace_upserts_services_and_exports_start_state() {
        let mut workspace = empty_service_workspace();
        upsert_service_workspace(
            &mut workspace,
            ServiceWorkspaceService {
                auto_start: true,
                command: "pnpm start".to_owned(),
                cwd: "services/support-suite-provider".to_owned(),
                lang: "ts".to_owned(),
                manifest: "lenso.service.json".to_owned(),
                modules: vec!["support-suite".to_owned()],
                name: "support-suite-provider".to_owned(),
                ready_timeout_ms: 10_000,
                ready_url: "http://127.0.0.1:4110/lenso/service/v1/status".to_owned(),
            },
        );
        upsert_service_workspace(
            &mut workspace,
            ServiceWorkspaceService {
                auto_start: true,
                command: "cargo run".to_owned(),
                cwd: "services/audit-provider".to_owned(),
                lang: "rust".to_owned(),
                manifest: "lenso.service.json".to_owned(),
                modules: vec!["audit".to_owned()],
                name: "audit-provider".to_owned(),
                ready_timeout_ms: 10_000,
                ready_url: "http://127.0.0.1:4130/lenso/service/v1/status".to_owned(),
            },
        );

        assert_eq!(workspace.services[0].name, "audit-provider");
        let state = service_workspace_module_services_json(&workspace);

        assert_eq!(state["modules"][0]["moduleName"], "audit-provider");
        assert_eq!(
            state["modules"][1]["services"][0]["readyUrl"],
            "http://127.0.0.1:4110/lenso/service/v1/status"
        );
    }

    #[test]
    fn ts_manifest_records_install_services() {
        let source = render_template(
            include_str!("../templates/service-ts/lenso.service.json.tmpl"),
            &scaffold(),
        );
        assert_no_template_tokens(&source);
        let manifest: Value = serde_json::from_str(&source).unwrap();

        assert_eq!(
            manifest["install"]["services"][0]["name"],
            "support-suite-provider"
        );
        assert_eq!(manifest["install"]["services"][0]["command"], "pnpm start");
        assert_eq!(
            manifest["install"]["services"][0]["readyUrl"],
            "http://127.0.0.1:4110/lenso/service/v1/status"
        );
        assert_eq!(manifest["install"]["services"][0]["autoStart"], json!(true));
        assert_eq!(
            manifest["install"]["services"][0]["readyTimeoutMs"],
            json!(10_000)
        );
    }

    #[test]
    fn rust_manifest_records_install_services() {
        let source = render_template(
            include_str!("../templates/service-rust/lenso.service.json.tmpl"),
            &scaffold(),
        );
        assert_no_template_tokens(&source);
        let manifest: Value = serde_json::from_str(&source).unwrap();

        assert_eq!(
            manifest["install"]["services"][0]["name"],
            "support-suite-provider"
        );
        assert_eq!(manifest["install"]["services"][0]["command"], "cargo run");
        assert_eq!(
            manifest["install"]["services"][0]["readyUrl"],
            "http://127.0.0.1:4110/lenso/service/v1/status"
        );
        assert_eq!(manifest["install"]["services"][0]["autoStart"], json!(true));
        assert_eq!(
            manifest["install"]["services"][0]["readyTimeoutMs"],
            json!(10_000)
        );
    }

    #[test]
    fn generated_service_manifests_use_service_protocol() {
        for template in [
            include_str!("../templates/service-ts/lenso.service.json.tmpl"),
            include_str!("../templates/service-rust/lenso.service.json.tmpl"),
        ] {
            let manifest: Value = serde_json::from_str(&render_template(template, &scaffold()))
                .expect("manifest should parse");

            assert_eq!(manifest["protocol"], "lenso.service.v1");
        }
    }

    #[test]
    fn service_code_templates_expose_module_release_checks() {
        let ts_service = render_template(
            include_str!("../templates/service-ts/src/service.ts"),
            &scaffold(),
        );
        let ts_server = render_template(
            include_str!("../templates/service-ts/src/server.ts"),
            &scaffold(),
        );
        let rust_server = render_template(
            include_str!("../templates/service-rust/src/main.rs"),
            &scaffold(),
        );

        assert!(ts_service.contains("defineModuleRelease"));
        assert!(ts_server.contains("--check-release"));
        assert!(rust_server.contains("fn module_release()"));
        assert!(rust_server.contains("--check-release"));
    }

    #[test]
    fn service_package_metadata_lists_declared_modules() {
        let metadata = service_package_metadata(&json!({
            "name": "support-suite-provider",
            "version": "0.2.0",
            "modules": [
                {"name": "support-ticket"},
                {"name": "support-inbox"}
            ]
        }))
        .unwrap();

        assert_eq!(
            metadata,
            ServicePackageMetadata {
                modules: vec![
                    ServicePackageModuleMetadata {
                        capabilities: Vec::new(),
                        dependencies: Vec::new(),
                        name: "support-ticket".to_owned(),
                        version: "0.2.0".to_owned(),
                    },
                    ServicePackageModuleMetadata {
                        capabilities: Vec::new(),
                        dependencies: Vec::new(),
                        name: "support-inbox".to_owned(),
                        version: "0.2.0".to_owned(),
                    },
                ],
                name: "support-suite-provider".to_owned(),
                version: "0.2.0".to_owned(),
            }
        );

        let artifact = service_package_manifest(&metadata);
        assert_eq!(artifact["protocol"], "lenso.service-package.v1");
        assert_eq!(artifact["serviceManifest"], "lenso.service.json");
    }

    #[tokio::test]
    async fn package_service_writes_manifest_and_package_metadata() {
        let root = std::env::temp_dir().join(format!(
            "lenso-cli-service-package-{}",
            uuid::Uuid::now_v7()
        ));
        let service_dir = root.join("support-suite-provider");
        fs::create_dir_all(&service_dir).unwrap();
        fs::write(
            service_dir.join("lenso.service.json"),
            serde_json::to_string_pretty(&json!({
                "protocol": "lenso.service.v1",
                "name": "support-suite-provider",
                "version": "0.2.0",
                "modules": [
                    {
                        "capabilities": ["support_ticket.tickets.read"],
                        "dependencies": ["auth"],
                        "name": "support-ticket",
                        "version": "0.2.1"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        package_service(ServicePackageOptions {
            check: false,
            json: false,
            manifest: "lenso.service.json".to_owned(),
            output_dir: PathBuf::from("dist/services"),
            service_dir: service_dir.clone(),
        })
        .await
        .unwrap();

        let package_manifest: Value = serde_json::from_str(
            &fs::read_to_string(
                service_dir.join("dist/services/support-suite-provider/lenso.service-package.json"),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(package_manifest["protocol"], "lenso.service-package.v1");
        assert_eq!(package_manifest["modules"], json!(["support-ticket"]));
        let module_release: Value = serde_json::from_str(
            &fs::read_to_string(
                service_dir.join(
                    "dist/services/support-suite-provider/modules/support-ticket/lenso.module-release.json",
                ),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(module_release["protocol"], "lenso.module-release.v1");
        assert_eq!(module_release["name"], "support-ticket");
        assert_eq!(module_release["version"], "0.2.1");
        assert_eq!(
            module_release["provider"]["servicePackage"],
            "../../lenso.service-package.json"
        );
        let module_contract: Value = serde_json::from_str(
            &fs::read_to_string(service_dir.join(
                "dist/services/support-suite-provider/modules/support-ticket/lenso.module.json",
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(module_contract["protocol"], "lenso.module.v1");
        assert_eq!(module_contract["source"], "service");
        assert_eq!(module_contract["name"], "support-ticket");
        assert_eq!(
            module_release["capabilities"],
            json!(["support_ticket.tickets.read"])
        );
        assert_eq!(module_release["dependencies"], json!(["auth"]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_templates_render_without_tokens() {
        let scaffold = scaffold();
        let ts_package_json = render_template(
            include_str!("../templates/service-ts/package.json.tmpl"),
            &scaffold,
        );
        assert!(ts_package_json.contains("\"service:package\""));
        assert!(ts_package_json.contains("\"service:release-plan\""));
        assert!(ts_package_json.contains("\"service:verify\""));
        for template in [
            include_str!("../templates/service-ts/package.json.tmpl"),
            include_str!("../templates/service-ts/pnpm-workspace.yaml.tmpl"),
            include_str!("../templates/service-rust/Cargo.toml.tmpl"),
            include_str!("../templates/service-ts/src/service.ts"),
            include_str!("../templates/service-ts/src/server.ts"),
            include_str!("../templates/service-rust/src/main.rs"),
        ] {
            assert_no_template_tokens(&render_template(template, &scaffold));
        }
    }

    fn assert_no_template_tokens(source: &str) {
        assert!(!source.contains("{{"), "{source}");
        assert!(!source.contains("}}"), "{source}");
    }
}
