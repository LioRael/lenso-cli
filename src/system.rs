use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const DEFAULT_SYSTEM_FILE: &str = "lenso.system.json";
const SERVICE_SYSTEM_PROTOCOL: &str = "lenso.system.v1";

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
        if let (Some(cwd), Some(lang), Some(command), Some(ready_url)) = (
            service.cwd.as_deref(),
            service.lang.as_deref(),
            service.command.as_deref(),
            service.ready_url.as_deref(),
        ) {
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
            commands.push(line);
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

fn system_path(system_file: Option<&Path>) -> Result<PathBuf> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    Ok(absolutize_from(
        &current_dir,
        system_file.unwrap_or_else(|| Path::new(DEFAULT_SYSTEM_FILE)),
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

fn shell_word(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'))
    {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\"'\"'"))
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
}
