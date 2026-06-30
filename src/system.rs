use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DEFAULT_SYSTEM_FILE: &str = "lenso.system.json";
const SERVICE_SYSTEM_PROTOCOL: &str = "lenso.system.v1";
const MODULE_INSTALLS_PATH: &str = ".lenso/module-installs.json";
const MODULE_SERVICES_PATH: &str = ".lenso/module-services.json";
const SERVICE_ENVIRONMENTS_PATH: &str = ".lenso/service-environments.json";
const SERVICE_DEPLOYMENTS_PATH: &str = ".lenso/service-deployments.json";
const SERVICE_RELEASES_PATH: &str = ".lenso/service-releases.json";

#[derive(Debug, Clone)]
pub(crate) struct SystemInitOptions {
    pub(crate) environments: Vec<String>,
    pub(crate) force: bool,
    pub(crate) name: String,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemAddServiceOptions {
    pub(crate) command: Option<String>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) lang: Option<String>,
    pub(crate) manifest: Option<String>,
    pub(crate) modules: Vec<String>,
    pub(crate) name: String,
    pub(crate) ready_url: Option<String>,
    pub(crate) system_file: Option<PathBuf>,
    pub(crate) target: String,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemAddModuleOptions {
    pub(crate) capabilities: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) install_to: Option<String>,
    pub(crate) name: String,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemPlanOptions {
    pub(crate) check: bool,
    pub(crate) json: bool,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemGraphOptions {
    pub(crate) json: bool,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemDiffOptions {
    pub(crate) check: bool,
    pub(crate) json: bool,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemApplyOptions {
    pub(crate) dry_run: bool,
    pub(crate) json: bool,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct SystemDoctorOptions {
    pub(crate) json: bool,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ServiceSystem {
    protocol: String,
    name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    environments: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    services: Vec<SystemService>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modules: Vec<SystemModule>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<SystemDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemService {
    name: String,
    target: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    manifest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ready_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemModule {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    install_to: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemDependency {
    from: String,
    capability: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemGraph {
    name: String,
    environments: Vec<String>,
    services: Vec<SystemGraphService>,
    modules: Vec<SystemGraphModule>,
    dependencies: Vec<SystemGraphDependency>,
    issues: Vec<SystemIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemGraphService {
    name: String,
    target: String,
    modules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemGraphModule {
    name: String,
    owner: String,
    capabilities: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemGraphDependency {
    from: String,
    capability: String,
    state: String,
    to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemIssue {
    code: String,
    message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemPlan {
    system_file: String,
    name: String,
    status: String,
    services: usize,
    modules: usize,
    dependencies: usize,
    issues: Vec<SystemIssue>,
    commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemDriftReport {
    version: u8,
    system_file: String,
    repo_root: String,
    name: String,
    status: String,
    graph_issues: Vec<SystemIssue>,
    drifts: Vec<SystemDrift>,
    commands: Vec<String>,
    applied: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SystemDrift {
    code: String,
    severity: String,
    resource: String,
    name: String,
    message: String,
    command: Option<String>,
}

#[derive(Debug, Default)]
struct HostSystemState {
    installed_modules: BTreeSet<String>,
    configured_services: BTreeSet<String>,
    environments: BTreeSet<String>,
    deployments: BTreeSet<String>,
    releases: BTreeSet<String>,
}

pub(crate) fn init_system(options: SystemInitOptions) -> Result<()> {
    let path = system_path(options.system_file.as_deref())?;
    if path.exists() && !options.force {
        bail!(
            "Service system already exists: {}. Use --force to replace it.",
            path.display()
        );
    }
    let system = ServiceSystem {
        protocol: SERVICE_SYSTEM_PROTOCOL.to_owned(),
        name: options.name,
        environments: options.environments,
        services: Vec::new(),
        modules: Vec::new(),
        dependencies: Vec::new(),
    };
    write_system(&path, &system)?;
    println!("Created service system {}.", path.display());
    Ok(())
}

pub(crate) fn add_system_service(options: SystemAddServiceOptions) -> Result<()> {
    let path = system_path(options.system_file.as_deref())?;
    let mut system = read_or_empty_system(&path)?;
    upsert_service(
        &mut system,
        SystemService {
            command: options.command,
            cwd: options.cwd.map(|path| path_string(&path)),
            lang: options.lang,
            manifest: options.manifest,
            modules: options.modules,
            name: options.name,
            ready_url: options.ready_url,
            target: options.target,
        },
    );
    write_system(&path, &system)?;
    println!("Updated service system {}.", path.display());
    Ok(())
}

pub(crate) fn add_system_module(options: SystemAddModuleOptions) -> Result<()> {
    let path = system_path(options.system_file.as_deref())?;
    let mut system = read_or_empty_system(&path)?;
    upsert_module(
        &mut system,
        SystemModule {
            capabilities: options.capabilities,
            dependencies: options.dependencies,
            install_to: options.install_to,
            name: options.name,
        },
    );
    write_system(&path, &system)?;
    println!("Updated service system {}.", path.display());
    Ok(())
}

pub(crate) fn plan_system(options: SystemPlanOptions) -> Result<()> {
    let path = system_read_path(options.system_file.as_deref())?;
    let system = read_system(&path)?;
    let graph = system_graph(&system);
    let commands = system_commands(&system);
    let plan = SystemPlan {
        commands,
        dependencies: graph.dependencies.len(),
        issues: graph.issues.clone(),
        modules: graph.modules.len(),
        name: system.name.clone(),
        services: graph.services.len(),
        status: if graph.issues.is_empty() {
            "ready".to_owned()
        } else {
            "needs_attention".to_owned()
        },
        system_file: path_string(&path),
    };
    if options.json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else {
        print_system_plan(&plan);
    }
    if options.check && !plan.issues.is_empty() {
        bail!("Service system plan has issues");
    }
    Ok(())
}

pub(crate) fn graph_system(options: SystemGraphOptions) -> Result<()> {
    let path = system_read_path(options.system_file.as_deref())?;
    let system = read_system(&path)?;
    let graph = system_graph(&system);
    if options.json {
        println!("{}", serde_json::to_string_pretty(&graph)?);
    } else {
        print_system_graph(&graph);
    }
    Ok(())
}

pub(crate) fn diff_system(options: SystemDiffOptions) -> Result<()> {
    let report = system_drift_report(
        options.system_file.as_deref(),
        options.repo_root.as_deref(),
        Vec::new(),
    )?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_system_drift_report(&report, "Service system drift");
    }
    if options.check && report.status != "ready" {
        bail!("Service system has drift");
    }
    Ok(())
}

pub(crate) fn apply_system(options: SystemApplyOptions) -> Result<()> {
    let path = system_read_path(options.system_file.as_deref())?;
    let repo_root = repo_root_path(options.repo_root.as_deref())?;
    let system = read_system(&path)?;
    let mut applied = Vec::new();
    applied.extend(apply_module_services(&repo_root, &system, options.dry_run)?);
    applied.extend(apply_service_environments(
        &repo_root,
        &system,
        options.dry_run,
    )?);
    let report = system_drift_report(Some(&path), Some(&repo_root), applied)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_system_drift_report(
            &report,
            if options.dry_run {
                "Service system apply preview"
            } else {
                "Service system apply"
            },
        );
    }
    Ok(())
}

pub(crate) fn doctor_system(options: SystemDoctorOptions) -> Result<()> {
    let report = system_drift_report(
        options.system_file.as_deref(),
        options.repo_root.as_deref(),
        Vec::new(),
    )?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_system_drift_report(&report, "Service system doctor");
        if report.status == "ready" {
            println!("next: none");
        } else if let Some(command) = report
            .drifts
            .iter()
            .find_map(|drift| drift.command.as_ref())
        {
            println!("next: {command}");
        }
    }
    Ok(())
}

fn upsert_service(system: &mut ServiceSystem, service: SystemService) {
    if let Some(existing) = system
        .services
        .iter_mut()
        .find(|existing| existing.name == service.name)
    {
        *existing = service;
    } else {
        system.services.push(service);
    }
    system.services.sort_by(|a, b| a.name.cmp(&b.name));
}

fn upsert_module(system: &mut ServiceSystem, module: SystemModule) {
    if let Some(existing) = system
        .modules
        .iter_mut()
        .find(|existing| existing.name == module.name)
    {
        *existing = module;
    } else {
        system.modules.push(module);
    }
    system.modules.sort_by(|a, b| a.name.cmp(&b.name));
}

fn system_graph(system: &ServiceSystem) -> SystemGraph {
    let services_by_name = system
        .services
        .iter()
        .map(|service| (service.name.as_str(), service))
        .collect::<BTreeMap<_, _>>();
    let modules_by_name = system
        .modules
        .iter()
        .map(|module| (module.name.as_str(), module))
        .collect::<BTreeMap<_, _>>();
    let mut module_owner = BTreeMap::new();
    let mut issues = Vec::new();
    for service in &system.services {
        for module_name in &service.modules {
            if !modules_by_name.contains_key(module_name.as_str()) {
                issues.push(SystemIssue {
                    code: "module_not_declared".to_owned(),
                    message: format!(
                        "Service `{}` references undeclared module `{module_name}`.",
                        service.name
                    ),
                });
            }
            if let Some(existing) = module_owner.insert(module_name.as_str(), service.name.as_str())
            {
                issues.push(SystemIssue {
                    code: "module_owned_twice".to_owned(),
                    message: format!(
                        "Module `{module_name}` is assigned to both `{existing}` and `{}`.",
                        service.name
                    ),
                });
            }
        }
    }
    for module in &system.modules {
        if let Some(service_name) = module
            .install_to
            .as_deref()
            .and_then(|install_to| install_to.strip_prefix("service:"))
            && !services_by_name.contains_key(service_name)
        {
            issues.push(SystemIssue {
                code: "install_target_missing".to_owned(),
                message: format!(
                    "Module `{}` installs to missing service `{service_name}`.",
                    module.name
                ),
            });
        }
    }

    let capability_owners = capability_owners(system, &module_owner);
    let mut dependencies = Vec::new();
    for module in &system.modules {
        let from = module_owner_name(module, &module_owner);
        for capability in &module.dependencies {
            dependencies.push(dependency_edge(
                from,
                capability,
                capability_owners
                    .get(capability.as_str())
                    .map(Vec::as_slice),
            ));
        }
    }
    for dependency in &system.dependencies {
        if let Some(to) = dependency.to.as_deref() {
            let target_exists =
                services_by_name.contains_key(to) || modules_by_name.contains_key(to);
            let target_has_capability = target_owns_capability(
                to,
                &dependency.capability,
                &capability_owners,
                &modules_by_name,
            );
            dependencies.push(SystemGraphDependency {
                capability: dependency.capability.clone(),
                from: dependency.from.clone(),
                state: if !target_exists {
                    "unresolved".to_owned()
                } else if target_has_capability {
                    "resolved".to_owned()
                } else {
                    "missing_capability".to_owned()
                },
                to: Some(to.to_owned()),
            });
        } else {
            dependencies.push(dependency_edge(
                &dependency.from,
                &dependency.capability,
                capability_owners
                    .get(dependency.capability.as_str())
                    .map(Vec::as_slice),
            ));
        }
    }
    for dependency in &dependencies {
        if dependency.state != "resolved" {
            issues.push(SystemIssue {
                code: format!("dependency_{}", dependency.state),
                message: format!(
                    "`{}` depends on `{}`, but it is {}.",
                    dependency.from, dependency.capability, dependency.state
                ),
            });
        }
    }

    SystemGraph {
        dependencies,
        environments: system.environments.clone(),
        issues,
        modules: system
            .modules
            .iter()
            .map(|module| SystemGraphModule {
                capabilities: module.capabilities.clone(),
                dependencies: module.dependencies.clone(),
                name: module.name.clone(),
                owner: module_owner_name(module, &module_owner).to_owned(),
            })
            .collect(),
        name: system.name.clone(),
        services: system
            .services
            .iter()
            .map(|service| SystemGraphService {
                modules: service.modules.clone(),
                name: service.name.clone(),
                target: service.target.clone(),
            })
            .collect(),
    }
}

fn capability_owners<'a>(
    system: &'a ServiceSystem,
    module_owner: &BTreeMap<&'a str, &'a str>,
) -> BTreeMap<&'a str, Vec<&'a str>> {
    let mut owners: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for module in &system.modules {
        let owner = module_owner_name(module, module_owner);
        for capability in &module.capabilities {
            owners.entry(capability.as_str()).or_default().push(owner);
        }
    }
    owners
}

fn target_owns_capability(
    target: &str,
    capability: &str,
    capability_owners: &BTreeMap<&str, Vec<&str>>,
    modules_by_name: &BTreeMap<&str, &SystemModule>,
) -> bool {
    capability_owners
        .get(capability)
        .is_some_and(|owners| owners.iter().any(|owner| *owner == target))
        || modules_by_name.get(target).is_some_and(|module| {
            module
                .capabilities
                .iter()
                .any(|provided| provided == capability)
        })
}

fn dependency_edge(from: &str, capability: &str, owners: Option<&[&str]>) -> SystemGraphDependency {
    let (state, to) = match owners {
        Some(owners) if owners.len() == 1 => ("resolved", Some(owners[0].to_owned())),
        Some(owners) if owners.len() > 1 => ("ambiguous", Some(owners.join(","))),
        _ => ("unresolved", None),
    };
    SystemGraphDependency {
        capability: capability.to_owned(),
        from: from.to_owned(),
        state: state.to_owned(),
        to,
    }
}

fn install_owner(module: &SystemModule) -> Option<&str> {
    let install_to = module.install_to.as_deref()?;
    install_to.strip_prefix("service:").or(Some(install_to))
}

fn module_owner_name<'a>(
    module: &'a SystemModule,
    module_owner: &BTreeMap<&'a str, &'a str>,
) -> &'a str {
    module_owner
        .get(module.name.as_str())
        .copied()
        .or_else(|| install_owner(module))
        .unwrap_or("host")
}

fn system_commands(system: &ServiceSystem) -> Vec<String> {
    let mut commands = Vec::new();
    for service in &system.services {
        if let Some(command) = service_workspace_command(service) {
            commands.push(command);
        }
        for environment in &system.environments {
            if matches!(service.target.as_str(), "kubernetes" | "operator") {
                commands.push(format!(
                    "lenso service env add {} --service {} --target {}",
                    shell_word(environment),
                    shell_word(&service.name),
                    shell_word(&service.target)
                ));
            }
        }
    }
    commands
}

fn system_drift_report(
    system_file: Option<&Path>,
    repo_root: Option<&Path>,
    applied: Vec<String>,
) -> Result<SystemDriftReport> {
    let path = system_read_path(system_file)?;
    let repo_root = repo_root_path(repo_root)?;
    let system = read_system(&path)?;
    let graph = system_graph(&system);
    let state = read_host_system_state(&repo_root)?;
    let drifts = system_drifts(&system, &graph, &state);
    let commands = drifts
        .iter()
        .filter_map(|drift| drift.command.clone())
        .collect::<Vec<_>>();
    let status = if !graph.issues.is_empty() {
        "needs_attention"
    } else if !drifts.is_empty() {
        "drifted"
    } else {
        "ready"
    };
    Ok(SystemDriftReport {
        version: 1,
        system_file: path_string(&path),
        repo_root: path_string(&repo_root),
        name: system.name,
        status: status.to_owned(),
        graph_issues: graph.issues,
        drifts,
        commands,
        applied,
    })
}

fn system_drifts(
    system: &ServiceSystem,
    graph: &SystemGraph,
    state: &HostSystemState,
) -> Vec<SystemDrift> {
    let mut drifts = Vec::new();
    for service in &system.services {
        if !state.configured_services.contains(&service.name) {
            drifts.push(SystemDrift {
                code: "service_not_configured".to_owned(),
                severity: "warning".to_owned(),
                resource: "service".to_owned(),
                name: service.name.clone(),
                message: format!("Service `{}` is declared but not configured.", service.name),
                command: service_workspace_command(service),
            });
        }
        if matches!(service.target.as_str(), "kubernetes" | "operator") {
            for environment in &system.environments {
                let key = service_environment_key(&service.name, environment);
                if !state.environments.contains(&key) {
                    drifts.push(SystemDrift {
                        code: "service_env_missing".to_owned(),
                        severity: "warning".to_owned(),
                        resource: "environment".to_owned(),
                        name: key.clone(),
                        message: format!(
                            "Service `{}` has no `{environment}` environment state.",
                            service.name
                        ),
                        command: Some(format!(
                            "lenso service env add {} --service {} --target {}",
                            shell_word(environment),
                            shell_word(&service.name),
                            shell_word(&service.target)
                        )),
                    });
                } else if !state.deployments.contains(&key) {
                    drifts.push(SystemDrift {
                        code: "deployment_state_missing".to_owned(),
                        severity: "info".to_owned(),
                        resource: "deployment".to_owned(),
                        name: key.clone(),
                        message: format!(
                            "Service `{}` has `{environment}` env state but no deployment observation.",
                            service.name
                        ),
                        command: Some(format!(
                            "lenso service deploy status {} --env {} --source {} --write-state",
                            shell_word(&service.name),
                            shell_word(environment),
                            shell_word(&service.target)
                        )),
                    });
                }
                if !state.releases.contains(&key) {
                    drifts.push(SystemDrift {
                        code: "release_state_missing".to_owned(),
                        severity: "info".to_owned(),
                        resource: "release".to_owned(),
                        name: key,
                        message: format!(
                            "Service `{}` has no `{environment}` release record.",
                            service.name
                        ),
                        command: Some(format!(
                            "lenso service release plan {} <manifest-or-package> --env {} --output release-plan.json",
                            shell_word(&service.name),
                            shell_word(environment)
                        )),
                    });
                }
            }
        }
    }
    for module in &graph.modules {
        if !state.installed_modules.contains(&module.name) {
            drifts.push(SystemDrift {
                code: "module_not_installed".to_owned(),
                severity: "warning".to_owned(),
                resource: "module".to_owned(),
                name: module.name.clone(),
                message: format!("Module `{}` is declared but not installed.", module.name),
                command: Some(format!("lenso module install {}", shell_word(&module.name))),
            });
        }
    }
    drifts
}

fn read_host_system_state(repo_root: &Path) -> Result<HostSystemState> {
    Ok(HostSystemState {
        installed_modules: read_installed_modules(&repo_root.join(MODULE_INSTALLS_PATH))?,
        configured_services: read_configured_services(&repo_root.join(MODULE_SERVICES_PATH))?,
        environments: read_service_environment_keys(&repo_root.join(SERVICE_ENVIRONMENTS_PATH))?,
        deployments: read_service_deployment_keys(&repo_root.join(SERVICE_DEPLOYMENTS_PATH))?,
        releases: read_service_release_keys(&repo_root.join(SERVICE_RELEASES_PATH))?,
    })
}

fn read_installed_modules(path: &Path) -> Result<BTreeSet<String>> {
    Ok(read_json_if_exists(path)?
        .and_then(|value| value.get("modules").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|module| string_field(&module, "moduleName"))
        .collect())
}

fn read_configured_services(path: &Path) -> Result<BTreeSet<String>> {
    let mut services = BTreeSet::new();
    let modules = read_json_if_exists(path)?
        .and_then(|value| value.get("modules").and_then(Value::as_array).cloned())
        .unwrap_or_default();
    for module in modules {
        if let Some(module_name) = string_field(&module, "moduleName") {
            services.insert(module_name);
        }
        for service in module
            .get("services")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            if let Some(name) = string_field(&service, "name") {
                services.insert(name);
            }
        }
    }
    Ok(services)
}

fn read_service_environment_keys(path: &Path) -> Result<BTreeSet<String>> {
    Ok(read_json_if_exists(path)?
        .and_then(|value| value.get("environments").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|environment| service_env_key_from_value(&environment))
        .collect())
}

fn read_service_deployment_keys(path: &Path) -> Result<BTreeSet<String>> {
    Ok(read_json_if_exists(path)?
        .and_then(|value| value.get("observations").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|observation| service_env_key_from_value(&observation))
        .collect())
}

fn read_service_release_keys(path: &Path) -> Result<BTreeSet<String>> {
    Ok(read_json_if_exists(path)?
        .and_then(|value| value.get("releases").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|release| {
            let service = string_field(&release, "serviceName")?;
            let environment = release
                .get("environment")
                .and_then(|environment| string_field(environment, "name"))
                .unwrap_or_else(|| "default".to_owned());
            Some(service_environment_key(&service, &environment))
        })
        .collect())
}

fn apply_module_services(
    repo_root: &Path,
    system: &ServiceSystem,
    dry_run: bool,
) -> Result<Vec<String>> {
    let path = repo_root.join(MODULE_SERVICES_PATH);
    let mut file =
        read_json_if_exists(&path)?.unwrap_or_else(|| json!({ "modules": [], "version": 1 }));
    if !file.get("modules").is_some_and(Value::is_array) {
        file["modules"] = json!([]);
    }
    let mut applied = Vec::new();
    for service in &system.services {
        let Some(plan) = module_service_plan(service) else {
            continue;
        };
        for module_name in &service.modules {
            upsert_module_service_plan(&mut file, module_name, plan.clone())?;
            applied.push(format!(
                "{} {}",
                if dry_run { "would update" } else { "updated" },
                display_relative(repo_root, &path)
            ));
        }
    }
    if !dry_run && !applied.is_empty() {
        write_json(&path, &file)?;
    }
    applied.sort();
    applied.dedup();
    Ok(applied)
}

fn apply_service_environments(
    repo_root: &Path,
    system: &ServiceSystem,
    dry_run: bool,
) -> Result<Vec<String>> {
    let path = repo_root.join(SERVICE_ENVIRONMENTS_PATH);
    let mut file =
        read_json_if_exists(&path)?.unwrap_or_else(|| json!({ "environments": [], "version": 1 }));
    if !file.get("environments").is_some_and(Value::is_array) {
        file["environments"] = json!([]);
    }
    let mut applied = Vec::new();
    for service in &system.services {
        if !matches!(service.target.as_str(), "kubernetes" | "operator") {
            continue;
        }
        for environment in &system.environments {
            upsert_service_environment(&mut file, service, environment)?;
            applied.push(format!(
                "{} {}",
                if dry_run { "would update" } else { "updated" },
                display_relative(repo_root, &path)
            ));
        }
    }
    if !dry_run && !applied.is_empty() {
        write_json(&path, &file)?;
    }
    applied.sort();
    applied.dedup();
    Ok(applied)
}

fn print_system_plan(plan: &SystemPlan) {
    println!("Service system: {} ({})", plan.name, plan.status);
    println!("file: {}", plan.system_file);
    println!(
        "services: {} / modules: {} / dependencies: {}",
        plan.services, plan.modules, plan.dependencies
    );
    if plan.issues.is_empty() {
        println!("issues: none");
    } else {
        println!("issues:");
        for issue in &plan.issues {
            println!("  - {}: {}", issue.code, issue.message);
        }
    }
    if !plan.commands.is_empty() {
        println!("commands:");
        for command in &plan.commands {
            println!("  {command}");
        }
    }
}

fn print_system_graph(graph: &SystemGraph) {
    println!("Service system graph: {}", graph.name);
    if !graph.environments.is_empty() {
        println!("environments: {}", graph.environments.join(", "));
    }
    println!("services:");
    for service in &graph.services {
        println!(
            "  {} [{}] modules={}",
            service.name,
            service.target,
            service.modules.join(", ")
        );
    }
    println!("modules:");
    for module in &graph.modules {
        println!(
            "  {} -> {} capabilities={}",
            module.name,
            module.owner,
            module.capabilities.join(", ")
        );
    }
    println!("dependencies:");
    for dependency in &graph.dependencies {
        println!(
            "  {} -> {} [{}] {}",
            dependency.from,
            dependency.to.as_deref().unwrap_or("?"),
            dependency.state,
            dependency.capability
        );
    }
    if !graph.issues.is_empty() {
        println!("issues:");
        for issue in &graph.issues {
            println!("  - {}: {}", issue.code, issue.message);
        }
    }
}

fn print_system_drift_report(report: &SystemDriftReport, title: &str) {
    println!("{title}: {} ({})", report.name, report.status);
    println!("system: {}", report.system_file);
    println!("repo: {}", report.repo_root);
    if report.graph_issues.is_empty() {
        println!("graph issues: none");
    } else {
        println!("graph issues:");
        for issue in &report.graph_issues {
            println!("  - {}: {}", issue.code, issue.message);
        }
    }
    if report.drifts.is_empty() {
        println!("drift: none");
    } else {
        println!("drift:");
        for drift in &report.drifts {
            println!("  - {} {}: {}", drift.resource, drift.name, drift.message);
        }
    }
    if !report.applied.is_empty() {
        println!("applied:");
        for item in &report.applied {
            println!("  - {item}");
        }
    }
    if !report.commands.is_empty() {
        println!("commands:");
        for command in &report.commands {
            println!("  {command}");
        }
    }
}

fn read_or_empty_system(path: &Path) -> Result<ServiceSystem> {
    if path.exists() {
        return read_system(path);
    }
    Ok(ServiceSystem {
        dependencies: Vec::new(),
        environments: Vec::new(),
        modules: Vec::new(),
        name: "lenso-system".to_owned(),
        protocol: SERVICE_SYSTEM_PROTOCOL.to_owned(),
        services: Vec::new(),
    })
}

fn read_system(path: &Path) -> Result<ServiceSystem> {
    let source = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let system: ServiceSystem =
        serde_json::from_str(&source).with_context(|| format!("parse {}", path.display()))?;
    if system.protocol != SERVICE_SYSTEM_PROTOCOL {
        bail!(
            "Service system {} uses unsupported protocol `{}`",
            path.display(),
            system.protocol
        );
    }
    Ok(system)
}

fn write_system(path: &Path, system: &ServiceSystem) -> Result<()> {
    let mut contents = serde_json::to_string_pretty(system).context("serialize service system")?;
    contents.push('\n');
    write_file(path, contents.as_bytes())
}

fn read_json_if_exists(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let source = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&source)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    let mut contents = serde_json::to_string_pretty(value).context("serialize JSON")?;
    contents.push('\n');
    write_file(path, contents.as_bytes())
}

fn system_path(system_file: Option<&Path>) -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    Ok(absolutize_from(
        &current_dir,
        system_file.unwrap_or_else(|| Path::new(DEFAULT_SYSTEM_FILE)),
    ))
}

fn repo_root_path(repo_root: Option<&Path>) -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    Ok(absolutize_from(
        &current_dir,
        repo_root.unwrap_or_else(|| Path::new(".")),
    ))
}

fn system_read_path(system_file: Option<&Path>) -> Result<PathBuf> {
    let path = system_path(system_file)?;
    if !path.exists() {
        bail!("Service system file does not exist: {}", path.display());
    }
    Ok(path)
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

fn absolutize_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base.join(path)
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn display_relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn shell_word(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn service_workspace_command(service: &SystemService) -> Option<String> {
    let (Some(cwd), Some(lang), Some(command), Some(ready_url)) = (
        service.cwd.as_deref(),
        service.lang.as_deref(),
        service.command.as_deref(),
        service.ready_url.as_deref(),
    ) else {
        return None;
    };
    let mut line = format!(
        "lenso service workspace add {} --cwd {} --lang {} --command {} --ready-url {}",
        shell_word(&service.name),
        shell_word(cwd),
        shell_word(lang),
        shell_word(command),
        shell_word(ready_url)
    );
    if let Some(manifest) = service.manifest.as_deref() {
        line.push_str(&format!(" --manifest {}", shell_word(manifest)));
    }
    for module in &service.modules {
        line.push_str(&format!(" --module {}", shell_word(module)));
    }
    Some(line)
}

fn module_service_plan(service: &SystemService) -> Option<Value> {
    let (Some(command), Some(ready_url)) =
        (service.command.as_deref(), service.ready_url.as_deref())
    else {
        return None;
    };
    Some(json!({
        "autoStart": true,
        "command": command,
        "cwd": service.cwd.as_deref().unwrap_or("."),
        "name": &service.name,
        "readyTimeoutMs": 10000,
        "readyUrl": ready_url,
    }))
}

fn upsert_module_service_plan(file: &mut Value, module_name: &str, service: Value) -> Result<()> {
    let modules = file
        .get_mut("modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("module services modules must be an array"))?;
    let module_entry = if let Some(index) = modules
        .iter()
        .position(|entry| string_field(entry, "moduleName").as_deref() == Some(module_name))
    {
        &mut modules[index]
    } else {
        modules.push(json!({ "moduleName": module_name, "services": [] }));
        modules.last_mut().expect("pushed module service entry")
    };
    if !module_entry.get("services").is_some_and(Value::is_array) {
        module_entry["services"] = json!([]);
    }
    let service_name = string_field(&service, "name")
        .ok_or_else(|| anyhow::anyhow!("module service name must be a string"))?;
    let services = module_entry
        .get_mut("services")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("module service services must be an array"))?;
    if let Some(existing) = services
        .iter_mut()
        .find(|entry| string_field(entry, "name").as_deref() == Some(service_name.as_str()))
    {
        *existing = service;
    } else {
        services.push(service);
    }
    Ok(())
}

fn upsert_service_environment(
    file: &mut Value,
    service: &SystemService,
    environment: &str,
) -> Result<()> {
    let environments = file
        .get_mut("environments")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| anyhow::anyhow!("service environments must be an array"))?;
    let value = json!({
        "name": environment,
        "releaseTrack": environment,
        "serviceName": &service.name,
        "target": &service.target,
    });
    if let Some(existing) = environments.iter_mut().find(|entry| {
        string_field(entry, "serviceName").as_deref() == Some(service.name.as_str())
            && string_field(entry, "name").as_deref() == Some(environment)
    }) {
        *existing = value;
    } else {
        environments.push(value);
    }
    environments.sort_by_key(|entry| {
        (
            string_field(entry, "serviceName").unwrap_or_default(),
            string_field(entry, "name").unwrap_or_default(),
        )
    });
    Ok(())
}

fn service_env_key_from_value(value: &Value) -> Option<String> {
    Some(service_environment_key(
        &string_field(value, "serviceName")?,
        &string_field(value, "environment").or_else(|| string_field(value, "name"))?,
    ))
}

fn service_environment_key(service: &str, environment: &str) -> String {
    format!("{service}/{environment}")
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_resolves_module_dependencies_by_capability() {
        let system = ServiceSystem {
            dependencies: Vec::new(),
            environments: vec!["local".to_owned()],
            modules: vec![
                SystemModule {
                    capabilities: Vec::new(),
                    dependencies: vec!["billing.invoice.read".to_owned()],
                    install_to: Some("service:support".to_owned()),
                    name: "support-ticket".to_owned(),
                },
                SystemModule {
                    capabilities: vec!["billing.invoice.read".to_owned()],
                    dependencies: Vec::new(),
                    install_to: Some("service:billing".to_owned()),
                    name: "invoice".to_owned(),
                },
            ],
            name: "support-platform".to_owned(),
            protocol: SERVICE_SYSTEM_PROTOCOL.to_owned(),
            services: vec![
                SystemService {
                    command: None,
                    cwd: None,
                    lang: None,
                    manifest: None,
                    modules: vec!["support-ticket".to_owned()],
                    name: "support".to_owned(),
                    ready_url: None,
                    target: "local".to_owned(),
                },
                SystemService {
                    command: None,
                    cwd: None,
                    lang: None,
                    manifest: None,
                    modules: vec!["invoice".to_owned()],
                    name: "billing".to_owned(),
                    ready_url: None,
                    target: "kubernetes".to_owned(),
                },
            ],
        };

        let graph = system_graph(&system);

        assert_eq!(graph.dependencies[0].state, "resolved");
        assert_eq!(graph.dependencies[0].to.as_deref(), Some("billing"));
        assert!(graph.issues.is_empty());
    }

    #[test]
    fn graph_reports_missing_install_targets() {
        let system = ServiceSystem {
            dependencies: Vec::new(),
            environments: Vec::new(),
            modules: vec![SystemModule {
                capabilities: Vec::new(),
                dependencies: Vec::new(),
                install_to: Some("service:missing".to_owned()),
                name: "support-ticket".to_owned(),
            }],
            name: "support-platform".to_owned(),
            protocol: SERVICE_SYSTEM_PROTOCOL.to_owned(),
            services: Vec::new(),
        };

        let graph = system_graph(&system);

        assert_eq!(graph.issues[0].code, "install_target_missing");
    }

    #[test]
    fn graph_checks_explicit_target_capabilities() {
        let system = ServiceSystem {
            dependencies: vec![SystemDependency {
                capability: "billing.invoice.write".to_owned(),
                from: "support".to_owned(),
                to: Some("billing".to_owned()),
            }],
            environments: Vec::new(),
            modules: vec![SystemModule {
                capabilities: vec!["billing.invoice.read".to_owned()],
                dependencies: Vec::new(),
                install_to: Some("service:billing".to_owned()),
                name: "invoice".to_owned(),
            }],
            name: "support-platform".to_owned(),
            protocol: SERVICE_SYSTEM_PROTOCOL.to_owned(),
            services: vec![SystemService {
                command: None,
                cwd: None,
                lang: None,
                manifest: None,
                modules: vec!["invoice".to_owned()],
                name: "billing".to_owned(),
                ready_url: None,
                target: "external".to_owned(),
            }],
        };

        let graph = system_graph(&system);

        assert_eq!(graph.dependencies[0].state, "missing_capability");
        assert_eq!(graph.issues[0].code, "dependency_missing_capability");
    }

    #[test]
    fn commands_use_service_workspace_when_enough_fields_exist() {
        let system = ServiceSystem {
            dependencies: Vec::new(),
            environments: Vec::new(),
            modules: Vec::new(),
            name: "support-platform".to_owned(),
            protocol: SERVICE_SYSTEM_PROTOCOL.to_owned(),
            services: vec![SystemService {
                command: Some("pnpm dev".to_owned()),
                cwd: Some("services/support".to_owned()),
                lang: Some("ts".to_owned()),
                manifest: Some("lenso.service.json".to_owned()),
                modules: vec!["support-ticket".to_owned()],
                name: "support".to_owned(),
                ready_url: Some("http://127.0.0.1:4110/lenso/service/v1/status".to_owned()),
                target: "local".to_owned(),
            }],
        };

        let commands = system_commands(&system);

        assert!(commands[0].contains("lenso service workspace add support"));
        assert!(commands[0].contains("--module support-ticket"));
    }

    #[test]
    fn drift_report_reads_host_state() {
        let root = test_root("drift-report");
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(root.join(".lenso")).unwrap();
        let system_path = root.join("lenso.system.json");
        write_system(&system_path, &support_system()).unwrap();
        write_json(
            &root.join(MODULE_INSTALLS_PATH),
            &json!({
                "modules": [{ "moduleName": "support-ticket", "source": "remote" }],
                "version": 1
            }),
        )
        .unwrap();
        write_json(
            &root.join(MODULE_SERVICES_PATH),
            &json!({
                "modules": [{
                    "moduleName": "support-ticket",
                    "services": [{ "name": "support", "command": "pnpm start", "readyUrl": "http://127.0.0.1:4110/status" }]
                }],
                "version": 1
            }),
        )
        .unwrap();
        write_json(
            &root.join(SERVICE_ENVIRONMENTS_PATH),
            &json!({
                "environments": [{ "name": "staging", "serviceName": "support", "target": "operator" }],
                "version": 1
            }),
        )
        .unwrap();
        write_json(
            &root.join(SERVICE_DEPLOYMENTS_PATH),
            &json!({
                "observations": [{ "environment": "staging", "serviceName": "support" }],
                "version": 2
            }),
        )
        .unwrap();
        write_json(
            &root.join(SERVICE_RELEASES_PATH),
            &json!({
                "releases": [{ "environment": { "name": "staging" }, "serviceName": "support" }],
                "version": 1
            }),
        )
        .unwrap();

        let report = system_drift_report(Some(&system_path), Some(&root), Vec::new()).unwrap();

        assert_eq!(report.status, "ready");
        assert!(report.drifts.is_empty());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn apply_writes_safe_host_state() {
        let root = test_root("apply");
        fs::remove_dir_all(&root).ok();
        fs::create_dir_all(&root).unwrap();
        let system = support_system();

        let applied = apply_module_services(&root, &system, false).unwrap();
        applied
            .into_iter()
            .chain(apply_service_environments(&root, &system, false).unwrap())
            .for_each(drop);
        let services = read_json_if_exists(&root.join(MODULE_SERVICES_PATH))
            .unwrap()
            .unwrap();
        let environments = read_json_if_exists(&root.join(SERVICE_ENVIRONMENTS_PATH))
            .unwrap()
            .unwrap();

        assert_eq!(services["modules"][0]["moduleName"], "support-ticket");
        assert_eq!(services["modules"][0]["services"][0]["name"], "support");
        assert_eq!(environments["environments"][0]["serviceName"], "support");
        assert_eq!(environments["environments"][0]["name"], "staging");
        fs::remove_dir_all(root).ok();
    }

    fn support_system() -> ServiceSystem {
        ServiceSystem {
            dependencies: Vec::new(),
            environments: vec!["staging".to_owned()],
            modules: vec![SystemModule {
                capabilities: vec!["support.ticket.read".to_owned()],
                dependencies: Vec::new(),
                install_to: Some("service:support".to_owned()),
                name: "support-ticket".to_owned(),
            }],
            name: "support-platform".to_owned(),
            protocol: SERVICE_SYSTEM_PROTOCOL.to_owned(),
            services: vec![SystemService {
                command: Some("pnpm start".to_owned()),
                cwd: Some("services/support".to_owned()),
                lang: Some("ts".to_owned()),
                manifest: Some("http://127.0.0.1:4110/lenso/service/v1/manifest".to_owned()),
                modules: vec!["support-ticket".to_owned()],
                name: "support".to_owned(),
                ready_url: Some("http://127.0.0.1:4110/status".to_owned()),
                target: "operator".to_owned(),
            }],
        }
    }

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lenso-system-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
