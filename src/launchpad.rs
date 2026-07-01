use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{ServiceLanguage, host, service};

const LAUNCHPAD_PROTOCOL: &str = "lenso.launchpad.v1";
const DEV_DOCTOR_PROTOCOL: &str = "lenso.dev-doctor.v1";
const APP_PROOF_PROTOCOL: &str = "lenso.app-proof.v1";
const LAUNCHPAD_FILE: &str = ".lenso/launchpad.json";
const DEV_DOCTOR_FILE: &str = ".lenso/dev-doctor.json";
const APP_PROOF_FILE: &str = ".lenso/app-proof.json";
const SYSTEM_FILE: &str = "lenso.system.json";
const WORKSPACE_FILE: &str = "lenso.workspace.json";
const DEFAULT_BLUEPRINT: &str = "support-desk";

#[derive(Debug, Clone)]
pub(crate) struct AppCreateOptions {
    pub(crate) blueprint: String,
    pub(crate) dir: PathBuf,
    pub(crate) force: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AppAddOptions {
    pub(crate) addon: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AppVerifyOptions {
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) write_proof: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AppDiffOptions {
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct AppRepairOptions {
    pub(crate) dry_run: bool,
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct DevStatusOptions {
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct DevDoctorOptions {
    pub(crate) live: bool,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) write_state: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AgentContextOptions {
    pub(crate) output: Option<PathBuf>,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) task: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchpadState {
    protocol: String,
    project_name: String,
    blueprint: String,
    status: String,
    summary: String,
    services: Vec<LaunchpadService>,
    modules: Vec<LaunchpadModule>,
    commands: LaunchpadCommands,
    checklist: Vec<LaunchpadChecklistItem>,
    #[serde(default)]
    addons: Vec<LaunchpadAddon>,
    #[serde(default)]
    supported_addons: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchpadService {
    name: String,
    role: String,
    language: String,
    cwd: String,
    command: String,
    manifest: String,
    ready_url: String,
    modules: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchpadModule {
    name: String,
    owner_service: String,
    capability: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchpadCommands {
    dev_up: String,
    dev_status: String,
    agent_context: String,
    console: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchpadChecklistItem {
    id: String,
    label: String,
    status: String,
    next_command: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchpadAddon {
    name: String,
    label: String,
    status: String,
    services: Vec<String>,
    modules: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevDoctorState {
    protocol: String,
    status: String,
    checked_at_unix_ms: u64,
    live: bool,
    checks: Vec<DevDoctorCheck>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevDoctorCheck {
    id: String,
    label: String,
    status: String,
    message: String,
    command: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppProofState {
    protocol: String,
    status: String,
    checked_at_unix_ms: u64,
    project_name: Option<String>,
    blueprint: Option<String>,
    addons: Vec<String>,
    checks: Vec<AppProofCheck>,
    drifts: Vec<AppProofDrift>,
    next_command: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppProofCheck {
    id: String,
    label: String,
    status: String,
    message: String,
    command: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppProofDrift {
    resource: String,
    name: String,
    message: String,
    command: Option<String>,
}

#[derive(Debug, Clone)]
struct Blueprint {
    name: String,
    label: String,
    summary: String,
    services: Vec<BlueprintService>,
    modules: Vec<BlueprintModule>,
    dependencies: Vec<BlueprintDependency>,
    supported_addons: Vec<String>,
}

#[derive(Debug, Clone)]
struct BlueprintService {
    name: String,
    role: String,
    lang: ServiceLanguage,
    lang_label: String,
    port: u16,
    command: String,
}

#[derive(Debug, Clone)]
struct BlueprintModule {
    name: String,
    owner_service: String,
    capabilities: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
struct BlueprintDependency {
    from: String,
    to: String,
    capability: String,
}

#[derive(Debug, Clone)]
struct Addon {
    name: String,
    label: String,
    summary: String,
    supported_blueprints: Vec<String>,
    services: Vec<BlueprintService>,
    modules: Vec<BlueprintModule>,
    dependencies: Vec<BlueprintDependency>,
}

pub(crate) fn list_blueprints() {
    println!("Lenso product blueprints:");
    for blueprint in built_in_blueprints() {
        println!("- {}: {}", blueprint.name, blueprint.label);
    }
}

pub(crate) fn inspect_blueprint(name: &str) -> Result<()> {
    let blueprint = blueprint_by_name(name)?;
    println!("{}: {}", blueprint.name, blueprint.label);
    println!("{}", blueprint.summary);
    println!();
    println!("services:");
    for service in &blueprint.services {
        println!(
            "- {} [{}] {} -> {}",
            service.name,
            service.lang_label,
            service.command,
            service_ready_url(service)
        );
    }
    println!();
    println!("modules:");
    for module in &blueprint.modules {
        println!(
            "- {} owned by {} ({})",
            module.name,
            module.owner_service,
            module.capabilities.join(", ")
        );
    }
    println!();
    println!("addons:");
    for addon in &blueprint.supported_addons {
        println!("- {addon}");
    }
    Ok(())
}

pub(crate) fn create_app(options: AppCreateOptions) -> Result<()> {
    let blueprint = blueprint_by_name(&options.blueprint)?;

    let project_name = project_name_from_dir(&options.dir);
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let target = absolutize_from(&current_dir, &options.dir);
    let target_display = target.to_string_lossy().to_string();

    host::init(&target_display, Some(&project_name), options.force)?;
    with_current_dir(&target, || {
        create_blueprint_files(&project_name, &blueprint)
    })?;

    println!();
    println!("Created Launchpad app {project_name}.");
    println!("Next steps:");
    println!("- cd {}", display_relative(&current_dir, &target));
    println!("- lenso dev status");
    println!("- lenso dev up");
    println!("- lenso agent context");
    Ok(())
}

pub(crate) fn add_app_addon(options: AppAddOptions) -> Result<()> {
    let repo_root = Path::new(".");
    let mut state = read_launchpad_state_required(repo_root)?;
    let addon = addon_by_name(&options.addon)?;
    if !addon.supported_blueprints.contains(&state.blueprint) {
        bail!(
            "addon `{}` does not support blueprint `{}`",
            addon.name,
            state.blueprint
        );
    }
    if addon_already_applied(&state, &addon.name) {
        bail!("addon `{}` is already applied", addon.name);
    }

    for service in &addon.services {
        if Path::new(&service_cwd(service)).exists() {
            upsert_workspace_service(service)?;
        } else {
            create_service_scaffold(service)?;
        }
        if !state
            .services
            .iter()
            .any(|existing| existing.name == service.name)
        {
            state
                .services
                .push(launchpad_service_from_blueprint(service));
        }
    }
    for module in &addon.modules {
        if !state
            .modules
            .iter()
            .any(|existing| existing.name == module.name)
        {
            state.modules.push(launchpad_module_from_blueprint(module));
        }
    }

    upsert_system_addon(Path::new(SYSTEM_FILE), &addon)?;
    state.addons.push(launchpad_addon_from_addon(&addon));
    state.checklist.push(LaunchpadChecklistItem {
        id: format!("addon-{}", addon.name),
        label: format!("Addon {} configured", addon.label),
        status: "done".to_owned(),
        next_command: None,
    });
    write_json(Path::new(LAUNCHPAD_FILE), &state)?;

    println!("Added addon {}.", addon.name);
    println!("{}", addon.summary);
    println!("Next: lenso dev doctor");
    Ok(())
}

pub(crate) fn app_verify(options: AppVerifyOptions) -> Result<()> {
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let proof = app_proof_state(&repo_root)?;
    print_app_proof(&proof);
    if options.write_proof {
        write_json(&repo_root.join(APP_PROOF_FILE), &proof)?;
        println!("Wrote {}.", repo_root.join(APP_PROOF_FILE).display());
    }
    if matches!(proof.status.as_str(), "failed" | "needs_attention") {
        bail!("app proof status is {}", proof.status);
    }
    Ok(())
}

pub(crate) fn app_diff(options: AppDiffOptions) -> Result<()> {
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let proof = app_proof_state(&repo_root)?;
    if proof.status == "ready" {
        println!("No app drift found.");
        return Ok(());
    }
    for drift in &proof.drifts {
        println!("- {} {}: {}", drift.resource, drift.name, drift.message);
        if let Some(command) = &drift.command {
            println!("  command: {command}");
        }
    }
    for check in proof
        .checks
        .iter()
        .filter(|check| matches!(check.status.as_str(), "failed" | "needs_attention"))
    {
        println!("- {}: {} ({})", check.id, check.status, check.message);
        if let Some(command) = &check.command {
            println!("  command: {command}");
        }
    }
    bail!("app proof status is {}", proof.status)
}

pub(crate) fn app_repair(options: AppRepairOptions) -> Result<()> {
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let proof = app_proof_state(&repo_root)?;
    let repairs = app_repair_plan(&proof.drifts);
    if repairs.is_empty() {
        println!("No safe app repairs needed.");
        return Ok(());
    }
    for repair in &repairs {
        println!("- {repair}");
    }
    if options.dry_run {
        println!("dry run: no files changed");
        return Ok(());
    }
    repair_generated_state(&repo_root)?;
    println!("Repaired generated app state.");
    println!("Next: lenso app verify --write-proof");
    Ok(())
}

pub(crate) fn dev_status(options: DevStatusOptions) -> Result<()> {
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let Some(state) = read_launchpad_state_optional(&repo_root)? else {
        println!(
            "No Launchpad state found at {}.",
            repo_root.join(LAUNCHPAD_FILE).display()
        );
        println!("Next: lenso app create support-desk --blueprint support-desk");
        return Ok(());
    };

    println!(
        "Lenso Launchpad: {} ({})",
        state.project_name, state.blueprint
    );
    println!("status: {}", state.status);
    println!("summary: {}", state.summary);
    println!();
    println!("services:");
    for service in &state.services {
        println!(
            "- {} [{}] {} -> {}",
            service.name, service.language, service.command, service.ready_url
        );
    }
    println!();
    println!("modules:");
    for module in &state.modules {
        println!(
            "- {} owned by {} ({})",
            module.name, module.owner_service, module.capability
        );
    }
    println!();
    println!("next: {}", state.commands.dev_up);
    Ok(())
}

pub(crate) fn dev_stop() {
    println!("lenso dev up runs in the foreground.");
    println!("Stop it with Ctrl-C in the terminal running dev up.");
}

pub(crate) async fn dev_doctor(options: DevDoctorOptions) -> Result<()> {
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let state = dev_doctor_state(&repo_root, options.live).await?;

    println!("Lenso dev doctor: {}", state.status);
    for check in &state.checks {
        println!("- {}: {} ({})", check.id, check.status, check.message);
        if let Some(command) = &check.command {
            println!("  next: {command}");
        }
    }
    if options.write_state {
        write_json(&repo_root.join(DEV_DOCTOR_FILE), &state)?;
        println!("Wrote {}.", repo_root.join(DEV_DOCTOR_FILE).display());
    }
    Ok(())
}

pub(crate) fn agent_context(options: AgentContextOptions) -> Result<()> {
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let state = read_launchpad_state_optional(&repo_root)?;
    let system = read_json_value_optional(&repo_root.join(SYSTEM_FILE))?;
    let workspace = read_json_value_optional(&repo_root.join(WORKSPACE_FILE))?;
    let doctor = read_json_value_optional(&repo_root.join(DEV_DOCTOR_FILE))?;
    let proof = read_app_proof_state_optional(&repo_root)?;
    let markdown = agent_context_markdown(
        state.as_ref(),
        system.as_ref(),
        workspace.as_ref(),
        doctor.as_ref(),
        proof.as_ref(),
        options.task.as_deref(),
    )?;

    if let Some(output) = options.output {
        write_file(&output, markdown.as_bytes())?;
        println!("Wrote agent context to {}.", output.display());
    } else {
        print!("{markdown}");
    }
    Ok(())
}

fn create_blueprint_files(project_name: &str, blueprint: &Blueprint) -> Result<()> {
    if ensure_env_file()? {
        println!("Prepared .env from .env.example.");
    }
    for service in &blueprint.services {
        create_service_scaffold(service)?;
    }

    write_json(
        Path::new(SYSTEM_FILE),
        &system_from_blueprint(project_name, blueprint),
    )?;
    write_json(
        Path::new(LAUNCHPAD_FILE),
        &launchpad_state_from_blueprint(project_name, blueprint),
    )
}

#[cfg(test)]
fn support_desk_launchpad_state(project_name: &str) -> LaunchpadState {
    launchpad_state_from_blueprint(project_name, &support_desk_blueprint())
}

fn launchpad_state_from_blueprint(project_name: &str, blueprint: &Blueprint) -> LaunchpadState {
    let service_label = if blueprint.services.len() == 1 {
        format!("one {} service", blueprint.services[0].lang_label)
    } else {
        "TypeScript and Rust services".to_owned()
    };
    LaunchpadState {
        protocol: LAUNCHPAD_PROTOCOL.to_owned(),
        project_name: project_name.to_owned(),
        blueprint: blueprint.name.clone(),
        status: "configured".to_owned(),
        summary: blueprint.summary.clone(),
        services: blueprint
            .services
            .iter()
            .map(launchpad_service_from_blueprint)
            .collect(),
        modules: blueprint
            .modules
            .iter()
            .map(launchpad_module_from_blueprint)
            .collect(),
        commands: LaunchpadCommands {
            dev_up: "lenso dev up".to_owned(),
            dev_status: "lenso dev status".to_owned(),
            agent_context: "lenso agent context".to_owned(),
            console: "http://127.0.0.1:3000/launchpad".to_owned(),
        },
        checklist: vec![
            LaunchpadChecklistItem {
                id: "app-created".to_owned(),
                label: "Host application scaffolded".to_owned(),
                status: "done".to_owned(),
                next_command: None,
            },
            LaunchpadChecklistItem {
                id: "services-created".to_owned(),
                label: format!("{service_label} scaffolded"),
                status: "done".to_owned(),
                next_command: None,
            },
            LaunchpadChecklistItem {
                id: "env-prepared".to_owned(),
                label: "Local environment file prepared".to_owned(),
                status: "done".to_owned(),
                next_command: None,
            },
            LaunchpadChecklistItem {
                id: "dev-up".to_owned(),
                label: "Run services and host locally".to_owned(),
                status: "next".to_owned(),
                next_command: Some("lenso dev up".to_owned()),
            },
            LaunchpadChecklistItem {
                id: "console-open".to_owned(),
                label: "Open Runtime Console Launchpad".to_owned(),
                status: "pending".to_owned(),
                next_command: Some("open http://127.0.0.1:3000/launchpad".to_owned()),
            },
        ],
        addons: Vec::new(),
        supported_addons: blueprint.supported_addons.clone(),
    }
}

#[cfg(test)]
fn support_desk_system(project_name: &str) -> Value {
    system_from_blueprint(project_name, &support_desk_blueprint())
}

fn system_from_blueprint(project_name: &str, blueprint: &Blueprint) -> Value {
    json!({
        "protocol": "lenso.system.v1",
        "name": project_name,
        "environments": ["local"],
        "services": blueprint.services.iter().map(system_service_from_blueprint).collect::<Vec<_>>(),
        "modules": system_modules_from_blueprint(blueprint),
        "dependencies": blueprint.dependencies.iter().map(system_dependency_from_blueprint).collect::<Vec<_>>()
    })
}

fn built_in_blueprints() -> Vec<Blueprint> {
    vec![
        support_desk_blueprint(),
        backoffice_crm_blueprint(),
        ops_console_blueprint(),
    ]
}

fn blueprint_by_name(name: &str) -> Result<Blueprint> {
    built_in_blueprints()
        .into_iter()
        .find(|blueprint| blueprint.name == name)
        .with_context(|| format!("unknown Launchpad blueprint `{name}`"))
}

fn support_desk_blueprint() -> Blueprint {
    Blueprint {
        name: "support-desk".to_owned(),
        label: "Support Desk".to_owned(),
        summary: "Support desk app with one TypeScript API service and one Rust worker service."
            .to_owned(),
        services: vec![
            blueprint_service(
                "support-api",
                "ticket intake and admin HTTP actions",
                ServiceLanguage::Ts,
                4110,
            ),
            blueprint_service(
                "notification-worker",
                "notification and background service functions",
                ServiceLanguage::Rust,
                4120,
            ),
        ],
        modules: vec![
            BlueprintModule {
                name: "support-api".to_owned(),
                owner_service: "support-api".to_owned(),
                capabilities: vec![
                    "support.tickets.read".to_owned(),
                    "support.tickets.write".to_owned(),
                ],
                dependencies: vec!["auth".to_owned()],
            },
            BlueprintModule {
                name: "notification-worker".to_owned(),
                owner_service: "notification-worker".to_owned(),
                capabilities: vec!["support.notifications.send".to_owned()],
                dependencies: vec!["support.tickets.read".to_owned()],
            },
        ],
        dependencies: vec![BlueprintDependency {
            from: "notification-worker".to_owned(),
            to: "support-api".to_owned(),
            capability: "support.tickets.read".to_owned(),
        }],
        supported_addons: vec![
            "support-sla".to_owned(),
            "customer-profile".to_owned(),
            "notifications".to_owned(),
        ],
    }
}

fn backoffice_crm_blueprint() -> Blueprint {
    Blueprint {
        name: "backoffice-crm".to_owned(),
        label: "Backoffice CRM".to_owned(),
        summary: "Backoffice CRM app with a TypeScript contacts service.".to_owned(),
        services: vec![blueprint_service(
            "crm-api",
            "contact and account operations",
            ServiceLanguage::Ts,
            4130,
        )],
        modules: vec![BlueprintModule {
            name: "crm-api".to_owned(),
            owner_service: "crm-api".to_owned(),
            capabilities: vec![
                "crm.contacts.read".to_owned(),
                "crm.contacts.write".to_owned(),
            ],
            dependencies: vec!["auth".to_owned()],
        }],
        dependencies: Vec::new(),
        supported_addons: vec!["customer-profile".to_owned(), "notifications".to_owned()],
    }
}

fn ops_console_blueprint() -> Blueprint {
    Blueprint {
        name: "ops-console".to_owned(),
        label: "Ops Console".to_owned(),
        summary: "Operations console app with a Rust audit service.".to_owned(),
        services: vec![blueprint_service(
            "ops-audit",
            "audit trail and operator evidence",
            ServiceLanguage::Rust,
            4140,
        )],
        modules: vec![BlueprintModule {
            name: "ops-audit".to_owned(),
            owner_service: "ops-audit".to_owned(),
            capabilities: vec!["ops.audit.read".to_owned()],
            dependencies: vec!["auth".to_owned()],
        }],
        dependencies: Vec::new(),
        supported_addons: vec!["notifications".to_owned()],
    }
}

fn blueprint_service(name: &str, role: &str, lang: ServiceLanguage, port: u16) -> BlueprintService {
    let (lang_label, command) = match lang {
        ServiceLanguage::Rust => ("rust", "cargo run"),
        ServiceLanguage::Ts => ("ts", "pnpm start"),
    };
    BlueprintService {
        name: name.to_owned(),
        role: role.to_owned(),
        lang,
        lang_label: lang_label.to_owned(),
        port,
        command: command.to_owned(),
    }
}

fn create_service_scaffold(service: &BlueprintService) -> Result<()> {
    service::create_service(service::ServiceCreateOptions {
        dry_run: false,
        lang: service.lang,
        name: service.name.clone(),
        no_workspace: false,
        output_dir: Some(PathBuf::from("services")),
        port: service.port,
        workspace_file: None,
    })
}

fn launchpad_service_from_blueprint(service: &BlueprintService) -> LaunchpadService {
    LaunchpadService {
        name: service.name.clone(),
        role: service.role.clone(),
        language: service.lang_label.clone(),
        cwd: service_cwd(service),
        command: service.command.clone(),
        manifest: service_manifest_url(service),
        ready_url: service_ready_url(service),
        modules: vec![service.name.clone()],
    }
}

fn launchpad_module_from_blueprint(module: &BlueprintModule) -> LaunchpadModule {
    LaunchpadModule {
        name: module.name.clone(),
        owner_service: module.owner_service.clone(),
        capability: module
            .capabilities
            .first()
            .cloned()
            .unwrap_or_else(|| module.name.clone()),
    }
}

fn system_service_from_blueprint(service: &BlueprintService) -> Value {
    json!({
        "name": service.name,
        "target": "local",
        "modules": [service.name],
        "cwd": service_cwd(service),
        "manifest": service_manifest_url(service),
        "command": service.command,
        "lang": service.lang_label,
        "readyUrl": service_ready_url(service)
    })
}

fn system_modules_from_blueprint(blueprint: &Blueprint) -> Vec<Value> {
    let mut modules = vec![json!({
        "name": "auth",
        "installTo": "host",
        "capabilities": ["auth"]
    })];
    modules.extend(blueprint.modules.iter().map(system_module_from_blueprint));
    modules
}

fn system_module_from_blueprint(module: &BlueprintModule) -> Value {
    json!({
        "name": module.name,
        "installTo": format!("service:{}", module.owner_service),
        "capabilities": module.capabilities,
        "dependencies": module.dependencies
    })
}

fn system_dependency_from_blueprint(dependency: &BlueprintDependency) -> Value {
    json!({
        "from": dependency.from,
        "to": dependency.to,
        "capability": dependency.capability
    })
}

fn service_cwd(service: &BlueprintService) -> String {
    format!("services/{}", service.name)
}

fn service_manifest_url(service: &BlueprintService) -> String {
    format!(
        "http://127.0.0.1:{}/lenso/service/v1/manifest",
        service.port
    )
}

fn service_ready_url(service: &BlueprintService) -> String {
    format!("http://127.0.0.1:{}/lenso/service/v1/status", service.port)
}

fn built_in_addons() -> Vec<Addon> {
    vec![
        support_sla_addon(),
        customer_profile_addon(),
        notifications_addon(),
    ]
}

fn addon_by_name(name: &str) -> Result<Addon> {
    built_in_addons()
        .into_iter()
        .find(|addon| addon.name == name)
        .with_context(|| format!("unknown Launchpad addon `{name}`"))
}

fn support_sla_addon() -> Addon {
    Addon {
        name: "support-sla".to_owned(),
        label: "Support SLA".to_owned(),
        summary: "Adds SLA tracking to the support desk blueprint.".to_owned(),
        supported_blueprints: vec!["support-desk".to_owned()],
        services: vec![blueprint_service(
            "support-sla",
            "ticket SLA and escalation policies",
            ServiceLanguage::Ts,
            4150,
        )],
        modules: vec![BlueprintModule {
            name: "support-sla".to_owned(),
            owner_service: "support-sla".to_owned(),
            capabilities: vec![
                "support.sla.read".to_owned(),
                "support.sla.write".to_owned(),
            ],
            dependencies: vec!["support.tickets.read".to_owned()],
        }],
        dependencies: vec![BlueprintDependency {
            from: "support-sla".to_owned(),
            to: "support-api".to_owned(),
            capability: "support.tickets.read".to_owned(),
        }],
    }
}

fn customer_profile_addon() -> Addon {
    Addon {
        name: "customer-profile".to_owned(),
        label: "Customer Profile".to_owned(),
        summary: "Adds customer profile data to a product blueprint.".to_owned(),
        supported_blueprints: vec!["support-desk".to_owned(), "backoffice-crm".to_owned()],
        services: vec![blueprint_service(
            "customer-profile",
            "customer account and profile data",
            ServiceLanguage::Ts,
            4160,
        )],
        modules: vec![BlueprintModule {
            name: "customer-profile".to_owned(),
            owner_service: "customer-profile".to_owned(),
            capabilities: vec![
                "customer.profile.read".to_owned(),
                "customer.profile.write".to_owned(),
            ],
            dependencies: vec!["auth".to_owned()],
        }],
        dependencies: Vec::new(),
    }
}

fn notifications_addon() -> Addon {
    Addon {
        name: "notifications".to_owned(),
        label: "Notifications".to_owned(),
        summary: "Adds notification sending as a Rust service.".to_owned(),
        supported_blueprints: vec![
            "support-desk".to_owned(),
            "backoffice-crm".to_owned(),
            "ops-console".to_owned(),
        ],
        services: vec![blueprint_service(
            "notifications",
            "notification delivery",
            ServiceLanguage::Rust,
            4170,
        )],
        modules: vec![BlueprintModule {
            name: "notifications".to_owned(),
            owner_service: "notifications".to_owned(),
            capabilities: vec!["notifications.send".to_owned()],
            dependencies: Vec::new(),
        }],
        dependencies: Vec::new(),
    }
}

fn addon_already_applied(state: &LaunchpadState, addon_name: &str) -> bool {
    state.addons.iter().any(|addon| addon.name == addon_name)
}

fn launchpad_addon_from_addon(addon: &Addon) -> LaunchpadAddon {
    LaunchpadAddon {
        name: addon.name.clone(),
        label: addon.label.clone(),
        status: "configured".to_owned(),
        services: addon
            .services
            .iter()
            .map(|service| service.name.clone())
            .collect(),
        modules: addon
            .modules
            .iter()
            .map(|module| module.name.clone())
            .collect(),
    }
}

fn upsert_workspace_service(service: &BlueprintService) -> Result<()> {
    service::add_service_workspace_entry(service::ServiceWorkspaceAddOptions {
        command: service.command.clone(),
        cwd: PathBuf::from(service_cwd(service)),
        lang: service.lang,
        manifest: "lenso.service.json".to_owned(),
        modules: vec![service.name.clone()],
        name: service.name.clone(),
        ready_url: service_ready_url(service),
        workspace_file: None,
    })
}

fn upsert_system_addon(path: &Path, addon: &Addon) -> Result<()> {
    let mut system = read_json_value_required(path)?;
    for service in &addon.services {
        upsert_json_object_by_name(
            &mut system,
            "services",
            system_service_from_blueprint(service),
        )?;
    }
    for module in &addon.modules {
        upsert_json_object_by_name(&mut system, "modules", system_module_from_blueprint(module))?;
    }
    for dependency in &addon.dependencies {
        upsert_json_dependency(&mut system, system_dependency_from_blueprint(dependency))?;
    }
    write_json(path, &system)
}

fn upsert_json_object_by_name(root: &mut Value, key: &str, item: Value) -> Result<()> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .context("generated item missing name")?;
    let array = json_array_mut(root, key)?;
    if let Some(existing) = array
        .iter_mut()
        .find(|existing| existing.get("name").and_then(Value::as_str) == Some(name))
    {
        *existing = item;
    } else {
        array.push(item);
    }
    Ok(())
}

fn upsert_json_dependency(root: &mut Value, item: Value) -> Result<()> {
    let from = item
        .get("from")
        .and_then(Value::as_str)
        .context("generated dependency missing from")?;
    let to = item
        .get("to")
        .and_then(Value::as_str)
        .context("generated dependency missing to")?;
    let capability = item
        .get("capability")
        .and_then(Value::as_str)
        .context("generated dependency missing capability")?;
    let array = json_array_mut(root, "dependencies")?;
    if let Some(existing) = array.iter_mut().find(|existing| {
        existing.get("from").and_then(Value::as_str) == Some(from)
            && existing.get("to").and_then(Value::as_str) == Some(to)
            && existing.get("capability").and_then(Value::as_str) == Some(capability)
    }) {
        *existing = item;
    } else {
        array.push(item);
    }
    Ok(())
}

fn json_array_mut<'a>(root: &'a mut Value, key: &str) -> Result<&'a mut Vec<Value>> {
    let object = root
        .as_object_mut()
        .context("system manifest must be an object")?;
    let value = object.entry(key).or_insert_with(|| json!([]));
    value
        .as_array_mut()
        .with_context(|| format!("system manifest `{key}` must be an array"))
}

async fn dev_doctor_state(repo_root: &Path, live: bool) -> Result<DevDoctorState> {
    let mut checks = Vec::new();
    checks.push(file_check(
        repo_root,
        ".env",
        ".env file",
        "cp .env.example .env",
    ));
    checks.push(json_file_check(
        repo_root,
        LAUNCHPAD_FILE,
        "Launchpad state",
        "lenso app create support-desk --blueprint support-desk",
    ));
    checks.push(json_file_check(
        repo_root,
        SYSTEM_FILE,
        "Service system manifest",
        "lenso app inspect support-desk",
    ));
    let workspace_check = json_file_check(
        repo_root,
        WORKSPACE_FILE,
        "Service workspace",
        "lenso service workspace list",
    );
    checks.push(workspace_check);

    if let Some(workspace) = read_json_value_optional(&repo_root.join(WORKSPACE_FILE))? {
        checks.extend(workspace_service_checks(repo_root, &workspace));
        checks.extend(command_checks(&workspace));
        if live {
            checks.extend(live_ready_checks(&workspace).await);
        }
    }

    Ok(DevDoctorState {
        protocol: DEV_DOCTOR_PROTOCOL.to_owned(),
        status: doctor_status(&checks),
        checked_at_unix_ms: current_unix_ms(),
        live,
        checks,
    })
}

fn file_check(repo_root: &Path, relative: &str, label: &str, command: &str) -> DevDoctorCheck {
    let path = repo_root.join(relative);
    if path.exists() {
        DevDoctorCheck {
            id: check_id(relative),
            label: label.to_owned(),
            status: "passed".to_owned(),
            message: format!("{relative} exists"),
            command: None,
        }
    } else {
        DevDoctorCheck {
            id: check_id(relative),
            label: label.to_owned(),
            status: "failed".to_owned(),
            message: format!("{relative} is missing"),
            command: Some(command.to_owned()),
        }
    }
}

fn json_file_check(repo_root: &Path, relative: &str, label: &str, command: &str) -> DevDoctorCheck {
    let path = repo_root.join(relative);
    match fs::read_to_string(&path) {
        Ok(source) => match serde_json::from_str::<Value>(&source) {
            Ok(_) => DevDoctorCheck {
                id: check_id(relative),
                label: label.to_owned(),
                status: "passed".to_owned(),
                message: format!("{relative} parses"),
                command: None,
            },
            Err(error) => DevDoctorCheck {
                id: check_id(relative),
                label: label.to_owned(),
                status: "failed".to_owned(),
                message: format!("{relative} does not parse: {error}"),
                command: Some(command.to_owned()),
            },
        },
        Err(_) => DevDoctorCheck {
            id: check_id(relative),
            label: label.to_owned(),
            status: "failed".to_owned(),
            message: format!("{relative} is missing"),
            command: Some(command.to_owned()),
        },
    }
}

fn workspace_service_checks(repo_root: &Path, workspace: &Value) -> Vec<DevDoctorCheck> {
    workspace_services(workspace)
        .into_iter()
        .flat_map(|service| {
            let cwd = service
                .get("cwd")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let manifest = service
                .get("manifest")
                .and_then(Value::as_str)
                .unwrap_or("lenso.service.json");
            let service_name = service
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("service");
            let cwd_path = repo_root.join(cwd);
            let manifest_path = cwd_path.join(manifest);
            [
                path_exists_check(
                    &cwd_path,
                    format!("service-cwd-{service_name}"),
                    format!("{service_name} cwd"),
                    format!("{cwd} exists"),
                    format!("{cwd} is missing"),
                    "lenso app add <addon>",
                ),
                path_exists_check(
                    &manifest_path,
                    format!("service-manifest-{service_name}"),
                    format!("{service_name} manifest"),
                    format!("{cwd}/{manifest} exists"),
                    format!("{cwd}/{manifest} is missing"),
                    "lenso service create <name> --lang ts --output-dir services",
                ),
            ]
        })
        .collect()
}

fn path_exists_check(
    path: &Path,
    id: String,
    label: String,
    ok_message: String,
    missing_message: String,
    command: &str,
) -> DevDoctorCheck {
    if path.exists() {
        DevDoctorCheck {
            id,
            label,
            status: "passed".to_owned(),
            message: ok_message,
            command: None,
        }
    } else {
        DevDoctorCheck {
            id,
            label,
            status: "failed".to_owned(),
            message: missing_message,
            command: Some(command.to_owned()),
        }
    }
}

fn command_checks(workspace: &Value) -> Vec<DevDoctorCheck> {
    let mut binaries = workspace_services(workspace)
        .into_iter()
        .filter_map(|service| service.get("command").and_then(Value::as_str))
        .filter_map(|command| command.split_whitespace().next())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    binaries.sort();
    binaries.dedup();
    binaries
        .into_iter()
        .map(|binary| {
            if command_available(&binary) {
                DevDoctorCheck {
                    id: format!("command-{binary}"),
                    label: format!("{binary} command"),
                    status: "passed".to_owned(),
                    message: format!("{binary} is available"),
                    command: None,
                }
            } else {
                DevDoctorCheck {
                    id: format!("command-{binary}"),
                    label: format!("{binary} command"),
                    status: "needs_attention".to_owned(),
                    message: format!("{binary} was not found on PATH"),
                    command: Some(format!("install {binary}")),
                }
            }
        })
        .collect()
}

async fn live_ready_checks(workspace: &Value) -> Vec<DevDoctorCheck> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
    else {
        return vec![DevDoctorCheck {
            id: "live-client".to_owned(),
            label: "Live HTTP checks".to_owned(),
            status: "failed".to_owned(),
            message: "could not create HTTP client".to_owned(),
            command: None,
        }];
    };
    let mut checks = Vec::new();
    for service in workspace_services(workspace) {
        let service_name = service
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("service");
        let Some(ready_url) = service.get("readyUrl").and_then(Value::as_str) else {
            checks.push(DevDoctorCheck {
                id: format!("ready-url-{service_name}"),
                label: format!("{service_name} ready URL"),
                status: "skipped".to_owned(),
                message: "readyUrl is not declared".to_owned(),
                command: None,
            });
            continue;
        };
        match client.get(ready_url).send().await {
            Ok(response) if response.status().is_success() => checks.push(DevDoctorCheck {
                id: format!("ready-{service_name}"),
                label: format!("{service_name} ready"),
                status: "passed".to_owned(),
                message: format!("{ready_url} returned {}", response.status()),
                command: None,
            }),
            Ok(response) => checks.push(DevDoctorCheck {
                id: format!("ready-{service_name}"),
                label: format!("{service_name} ready"),
                status: "needs_attention".to_owned(),
                message: format!("{ready_url} returned {}", response.status()),
                command: Some("lenso dev up".to_owned()),
            }),
            Err(error) => checks.push(DevDoctorCheck {
                id: format!("ready-{service_name}"),
                label: format!("{service_name} ready"),
                status: "needs_attention".to_owned(),
                message: format!("{ready_url} is unreachable: {error}"),
                command: Some("lenso dev up".to_owned()),
            }),
        }
    }
    checks
}

fn workspace_services(workspace: &Value) -> Vec<&Value> {
    workspace
        .get("services")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect()
}

fn command_available(binary: &str) -> bool {
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .any(|dir| dir.join(binary).exists())
}

fn doctor_status(checks: &[DevDoctorCheck]) -> String {
    if checks.iter().any(|check| check.status == "failed") {
        "failed".to_owned()
    } else if checks.iter().any(|check| check.status == "needs_attention") {
        "needs_attention".to_owned()
    } else {
        "ready".to_owned()
    }
}

fn app_proof_state(repo_root: &Path) -> Result<AppProofState> {
    let launchpad = read_launchpad_state_required(repo_root)?;
    let doctor = read_dev_doctor_state_optional(repo_root)?;
    let workspace = read_json_value_optional(&repo_root.join(WORKSPACE_FILE))?;
    let system = read_json_value_optional(&repo_root.join(SYSTEM_FILE))?;
    let (checks, mut drifts) =
        app_diff_from_values(&launchpad, workspace.as_ref(), system.as_ref())?;
    if doctor.is_none() {
        drifts.push(AppProofDrift {
            command: Some("lenso dev doctor --write-state".to_owned()),
            message: format!("{DEV_DOCTOR_FILE} is missing"),
            name: DEV_DOCTOR_FILE.to_owned(),
            resource: "dev-doctor".to_owned(),
        });
    }
    Ok(app_proof_state_from_parts(
        &launchpad,
        doctor.as_ref(),
        checks,
        drifts,
    ))
}

fn app_diff_from_values(
    launchpad: &LaunchpadState,
    workspace: Option<&Value>,
    system: Option<&Value>,
) -> Result<(Vec<AppProofCheck>, Vec<AppProofDrift>)> {
    let mut checks = Vec::new();
    let mut drifts = Vec::new();
    let workspace_services = workspace_service_names(workspace);
    let system_services = system_service_names(system);
    let launchpad_services = launchpad
        .services
        .iter()
        .map(|service| service.name.clone())
        .collect::<Vec<_>>();
    let expected_services = expected_services_from_launchpad(launchpad)?;

    for service in &expected_services {
        push_service_check(
            &mut checks,
            &mut drifts,
            "launchpad-service",
            &format!("launchpad-service-{}", service.name),
            &service.name,
            launchpad_services.contains(&service.name),
            "lenso app repair",
            LAUNCHPAD_FILE,
        );
        push_service_check(
            &mut checks,
            &mut drifts,
            "workspace-service",
            &format!("workspace-service-{}", service.name),
            &service.name,
            workspace_services.contains(&service.name),
            "lenso app repair",
            WORKSPACE_FILE,
        );
        push_service_check(
            &mut checks,
            &mut drifts,
            "system-service",
            &format!("system-service-{}", service.name),
            &service.name,
            system_services.contains(&service.name),
            "lenso app repair",
            SYSTEM_FILE,
        );
    }

    Ok((checks, drifts))
}

fn expected_services_from_launchpad(launchpad: &LaunchpadState) -> Result<Vec<BlueprintService>> {
    let blueprint = blueprint_by_name(&launchpad.blueprint)?;
    let mut services = blueprint.services;
    for addon in &launchpad.addons {
        services.extend(addon_by_name(&addon.name)?.services);
    }
    Ok(services)
}

fn workspace_service_names(workspace: Option<&Value>) -> Vec<String> {
    workspace
        .and_then(|value| value.get("services"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|service| service.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn system_service_names(system: Option<&Value>) -> Vec<String> {
    system
        .and_then(|value| value.get("services"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|service| service.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .collect()
}

fn push_service_check(
    checks: &mut Vec<AppProofCheck>,
    drifts: &mut Vec<AppProofDrift>,
    resource: &str,
    id: &str,
    service: &str,
    present: bool,
    command: &str,
    file: &str,
) {
    if present {
        checks.push(AppProofCheck {
            command: None,
            id: id.to_owned(),
            label: format!("{service} in {file}"),
            message: format!("{service} is present in {file}"),
            status: "passed".to_owned(),
        });
    } else {
        checks.push(AppProofCheck {
            command: Some(command.to_owned()),
            id: id.to_owned(),
            label: format!("{service} in {file}"),
            message: format!("{service} is missing from {file}"),
            status: "drifted".to_owned(),
        });
        drifts.push(AppProofDrift {
            command: Some(command.to_owned()),
            message: format!("{service} is missing from {file}"),
            name: service.to_owned(),
            resource: resource.to_owned(),
        });
    }
}

fn app_proof_state_from_parts(
    launchpad: &LaunchpadState,
    doctor: Option<&DevDoctorState>,
    mut checks: Vec<AppProofCheck>,
    drifts: Vec<AppProofDrift>,
) -> AppProofState {
    checks.push(AppProofCheck {
        command: match doctor {
            Some(state) if state.status == "ready" => None,
            _ => Some("lenso dev doctor --write-state".to_owned()),
        },
        id: "launchpad-doctor-state".to_owned(),
        label: "Launchpad doctor state".to_owned(),
        message: match doctor {
            Some(state) => format!("{DEV_DOCTOR_FILE} status is {}", state.status),
            None => format!("{DEV_DOCTOR_FILE} is missing"),
        },
        status: match doctor {
            Some(state) if state.status == "ready" => "passed".to_owned(),
            Some(state) => state.status.clone(),
            None => "needs_attention".to_owned(),
        },
    });

    let status = app_proof_status(&checks, &drifts).to_owned();
    let next_command = app_proof_next_command(&checks, &drifts);

    AppProofState {
        addons: launchpad
            .addons
            .iter()
            .map(|addon| addon.name.clone())
            .collect(),
        blueprint: Some(launchpad.blueprint.clone()),
        checked_at_unix_ms: current_unix_ms(),
        checks,
        drifts,
        next_command,
        project_name: Some(launchpad.project_name.clone()),
        protocol: APP_PROOF_PROTOCOL.to_owned(),
        status,
    }
}

fn app_proof_status(checks: &[AppProofCheck], drifts: &[AppProofDrift]) -> &'static str {
    if checks.iter().any(|check| check.status == "failed") {
        "failed"
    } else if checks.iter().any(|check| check.status == "needs_attention") {
        "needs_attention"
    } else if !drifts.is_empty() || checks.iter().any(|check| check.status == "drifted") {
        "drifted"
    } else if checks.is_empty() {
        "empty"
    } else {
        "ready"
    }
}

fn app_proof_next_command(checks: &[AppProofCheck], drifts: &[AppProofDrift]) -> Option<String> {
    drifts
        .iter()
        .find_map(|drift| drift.command.clone())
        .or_else(|| checks.iter().find_map(|check| check.command.clone()))
}

fn print_app_proof(proof: &AppProofState) {
    println!("App proof: {}", proof.status);
    if let Some(project_name) = &proof.project_name {
        println!("project: {project_name}");
    }
    if let Some(blueprint) = &proof.blueprint {
        println!("blueprint: {blueprint}");
    }
    let addons = if proof.addons.is_empty() {
        "none".to_owned()
    } else {
        proof.addons.join(", ")
    };
    println!("addons: {addons}");
    println!("checks: {}", proof.checks.len());
    println!("drifts: {}", proof.drifts.len());
    if let Some(command) = &proof.next_command {
        println!("next: {command}");
    }
}

fn app_repair_plan(drifts: &[AppProofDrift]) -> Vec<String> {
    drifts
        .iter()
        .filter_map(|drift| match drift.resource.as_str() {
            "launchpad-service" => Some(format!("restore Launchpad service {}", drift.name)),
            "workspace-service" => Some(format!("restore workspace service {}", drift.name)),
            "system-service" => Some(format!("restore system service {}", drift.name)),
            _ => None,
        })
        .collect()
}

fn repair_generated_state(repo_root: &Path) -> Result<()> {
    let launchpad = read_launchpad_state_required(repo_root)?;
    let blueprint = blueprint_by_name(&launchpad.blueprint)?;
    let addon_recipes = launchpad
        .addons
        .iter()
        .map(|addon| addon_by_name(&addon.name))
        .collect::<Result<Vec<_>>>()?;
    with_current_dir(repo_root, || {
        repair_launchpad_state(&launchpad, &blueprint, &addon_recipes)?;
        repair_workspace_recipes(&blueprint, &addon_recipes)?;
        repair_system_recipes(&launchpad.project_name, &blueprint, &addon_recipes)?;
        repair_missing_service_scaffolds(&blueprint.services)?;
        for addon in &addon_recipes {
            repair_missing_service_scaffolds(&addon.services)?;
        }
        Ok(())
    })
}

fn repair_launchpad_state(
    launchpad: &LaunchpadState,
    blueprint: &Blueprint,
    addons: &[Addon],
) -> Result<()> {
    let mut repaired = launchpad_state_from_blueprint(&launchpad.project_name, blueprint);
    for addon in &launchpad.addons {
        let addon_recipe = addons
            .iter()
            .find(|recipe| recipe.name == addon.name)
            .with_context(|| format!("unknown addon {}", addon.name))?;
        for service in &addon_recipe.services {
            if !repaired
                .services
                .iter()
                .any(|item| item.name == service.name)
            {
                repaired
                    .services
                    .push(launchpad_service_from_blueprint(service));
            }
        }
        for module in &addon_recipe.modules {
            if !repaired.modules.iter().any(|item| item.name == module.name) {
                repaired
                    .modules
                    .push(launchpad_module_from_blueprint(module));
            }
        }
        if !repaired.addons.iter().any(|item| item.name == addon.name) {
            repaired.addons.push(addon.clone());
        }
    }
    write_json(Path::new(LAUNCHPAD_FILE), &repaired)
}

fn repair_workspace_recipes(blueprint: &Blueprint, addons: &[Addon]) -> Result<()> {
    for service in &blueprint.services {
        upsert_workspace_service(service)?;
    }
    for addon in addons {
        for service in &addon.services {
            upsert_workspace_service(service)?;
        }
    }
    Ok(())
}

fn repair_system_recipes(
    project_name: &str,
    blueprint: &Blueprint,
    addons: &[Addon],
) -> Result<()> {
    let path = Path::new(SYSTEM_FILE);
    let mut system = if path.exists() {
        read_json_value_required(path)?
    } else {
        system_from_blueprint(project_name, blueprint)
    };
    for service in &blueprint.services {
        upsert_json_object_by_name(
            &mut system,
            "services",
            system_service_from_blueprint(service),
        )?;
    }
    for module in system_modules_from_blueprint(blueprint) {
        upsert_json_object_by_name(&mut system, "modules", module)?;
    }
    for dependency in &blueprint.dependencies {
        upsert_json_dependency(&mut system, system_dependency_from_blueprint(dependency))?;
    }
    for addon in addons {
        for service in &addon.services {
            upsert_json_object_by_name(
                &mut system,
                "services",
                system_service_from_blueprint(service),
            )?;
        }
        for module in &addon.modules {
            upsert_json_object_by_name(
                &mut system,
                "modules",
                system_module_from_blueprint(module),
            )?;
        }
        for dependency in &addon.dependencies {
            upsert_json_dependency(&mut system, system_dependency_from_blueprint(dependency))?;
        }
    }
    write_json(path, &system)
}

fn repair_missing_service_scaffolds(services: &[BlueprintService]) -> Result<()> {
    for service in services {
        if !Path::new(&service_cwd(service)).exists() {
            create_service_scaffold(service)?;
        }
    }
    Ok(())
}

fn check_id(relative: &str) -> String {
    relative
        .trim_start_matches(".lenso/")
        .replace(['/', '.'], "-")
        .trim_matches('-')
        .to_owned()
}

fn current_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn agent_context_markdown(
    state: Option<&LaunchpadState>,
    system: Option<&Value>,
    workspace: Option<&Value>,
    doctor: Option<&Value>,
    proof: Option<&AppProofState>,
    task: Option<&str>,
) -> Result<String> {
    let mut output = String::new();
    output.push_str("# Lenso Agent Context\n\n");
    if let Some(state) = state {
        output.push_str("## Launchpad\n\n");
        output.push_str(&format!("- Project: {}\n", state.project_name));
        output.push_str(&format!("- Blueprint: {}\n", state.blueprint));
        output.push_str(&format!("- Status: {}\n", state.status));
        output.push_str(&format!("- Summary: {}\n", state.summary));
        output.push_str(&format!("- Next command: {}\n\n", state.commands.dev_up));
        if !state.addons.is_empty() {
            output.push_str("## Addons\n\n");
            for addon in &state.addons {
                output.push_str(&format!(
                    "- {} ({}) services: {}\n",
                    addon.name,
                    addon.status,
                    addon.services.join(", ")
                ));
            }
            output.push('\n');
        }

        output.push_str("## Services\n\n");
        for service in &state.services {
            output.push_str(&format!(
                "- {} ({}) in `{}` with `{}`\n",
                service.name, service.language, service.cwd, service.command
            ));
        }
        output.push('\n');

        output.push_str("## Modules\n\n");
        for module in &state.modules {
            output.push_str(&format!(
                "- {} owned by {} for {}\n",
                module.name, module.owner_service, module.capability
            ));
        }
        output.push('\n');
    } else {
        output.push_str("## Launchpad\n\n");
        output.push_str("- State: not found\n");
        output
            .push_str("- Next command: lenso app create support-desk --blueprint support-desk\n\n");
    }

    output.push_str("## Boundaries\n\n");
    output.push_str("- Host owns auth, runtime queues, retries, outbox, Runtime Story, and Technical Operations.\n");
    output.push_str("- Services are remote processes that expose service manifests, routes, runtime functions, event handlers, and admin actions.\n");
    output.push_str("- Modules live inside services or the host; generated Launchpad JSON is control-plane state, not a hand-authored module contract.\n\n");

    output.push_str("## Service System\n\n");
    push_json_block(&mut output, system)?;
    output.push('\n');
    output.push_str("## Service Workspace\n\n");
    push_json_block(&mut output, workspace)?;

    if let Some(doctor) = doctor {
        output.push('\n');
        output.push_str("## Dev Doctor\n\n");
        let checks = doctor
            .get("checks")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let needs_attention = doctor
            .get("checks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|check| {
                check
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| matches!(status, "needs_attention" | "failed"))
            })
            .count();
        output.push_str(&format!(
            "- Status: {}\n",
            doctor
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
        output.push_str(&format!("- Checks: {checks}\n"));
        output.push_str(&format!("- Needs attention: {needs_attention}\n"));
    }

    if let Some(proof) = proof {
        output.push('\n');
        output.push_str("## App Proof\n\n");
        output.push_str(&format!("- Status: {}\n", proof.status));
        output.push_str(&format!("- Drifts: {}\n", proof.drifts.len()));
        if let Some(command) = &proof.next_command {
            output.push_str(&format!("- Next command: {command}\n"));
        }
        output.push_str("- Generated control-plane files may be repaired.\n");
        output.push_str("- Existing service source files are user code.\n");
        output.push_str("- Unknown services should not be deleted.\n");
    }

    if let Some(task) = task {
        output.push('\n');
        output.push_str("## Task\n\n");
        output.push_str(task);
        output.push('\n');
    }

    Ok(output)
}

fn push_json_block(output: &mut String, value: Option<&Value>) -> Result<()> {
    output.push_str("```json\n");
    if let Some(value) = value {
        output.push_str(&serde_json::to_string_pretty(value).context("serialize JSON block")?);
        output.push('\n');
    } else {
        output.push_str("{}\n");
    }
    output.push_str("```\n");
    Ok(())
}

fn read_launchpad_state_optional(repo_root: &Path) -> Result<Option<LaunchpadState>> {
    let path = repo_root.join(LAUNCHPAD_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn read_launchpad_state_required(repo_root: &Path) -> Result<LaunchpadState> {
    read_launchpad_state_optional(repo_root)?.with_context(|| {
        format!(
            "{} not found. Run `lenso app create <dir> --blueprint support-desk` first.",
            repo_root.join(LAUNCHPAD_FILE).display()
        )
    })
}

fn read_dev_doctor_state_optional(repo_root: &Path) -> Result<Option<DevDoctorState>> {
    let path = repo_root.join(DEV_DOCTOR_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn read_app_proof_state_optional(repo_root: &Path) -> Result<Option<AppProofState>> {
    let path = repo_root.join(APP_PROOF_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn read_json_value_optional(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
}

fn read_json_value_required(path: &Path) -> Result<Value> {
    read_json_value_optional(path)?.with_context(|| format!("{} not found", path.display()))
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let source = serde_json::to_string_pretty(value).context("serialize JSON")?;
    write_file(path, format!("{source}\n").as_bytes())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

fn ensure_env_file() -> Result<bool> {
    let env = Path::new(".env");
    if env.exists() {
        return Ok(false);
    }
    let example = Path::new(".env.example");
    fs::copy(example, env).with_context(|| {
        format!(
            "copy {} to {} for Launchpad local development",
            example.display(),
            env.display()
        )
    })?;
    Ok(true)
}

fn with_current_dir<T>(dir: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let original = std::env::current_dir().context("resolve current directory")?;
    std::env::set_current_dir(dir).with_context(|| format!("enter {}", dir.display()))?;
    let result = f();
    let restore = std::env::set_current_dir(&original)
        .with_context(|| format!("restore {}", original.display()));
    match (result, restore) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(restore_error)) => Err(error.context(format!("{restore_error}"))),
    }
}

fn project_name_from_dir(dir: &Path) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_BLUEPRINT)
        .to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchpad_state_models_support_desk() {
        let state = support_desk_launchpad_state("support-desk");

        assert_eq!(state.protocol, LAUNCHPAD_PROTOCOL);
        assert_eq!(state.project_name, "support-desk");
        assert_eq!(state.blueprint, DEFAULT_BLUEPRINT);
        assert_eq!(state.services.len(), 2);
        assert_eq!(state.modules.len(), 2);
        assert_eq!(state.commands.dev_up, "lenso dev up");
    }

    #[test]
    fn built_in_blueprints_include_three_product_paths() {
        let names = built_in_blueprints()
            .into_iter()
            .map(|blueprint| blueprint.name)
            .collect::<Vec<_>>();

        assert_eq!(names, ["support-desk", "backoffice-crm", "ops-console"]);
    }

    #[test]
    fn support_desk_declares_supported_addons() {
        let blueprint = blueprint_by_name("support-desk").unwrap();

        assert_eq!(
            blueprint.supported_addons,
            ["support-sla", "customer-profile", "notifications"]
        );
    }

    #[test]
    fn support_sla_addon_targets_support_desk() {
        let addon = addon_by_name("support-sla").unwrap();

        assert_eq!(addon.supported_blueprints, ["support-desk"]);
        assert_eq!(addon.services[0].name, "support-sla");
    }

    #[test]
    fn duplicate_addon_is_rejected() {
        let mut state = support_desk_launchpad_state("support-desk");
        state.addons.push(LaunchpadAddon {
            label: "Support SLA".to_owned(),
            modules: vec!["support-sla".to_owned()],
            name: "support-sla".to_owned(),
            services: vec!["support-sla".to_owned()],
            status: "configured".to_owned(),
        });

        assert!(addon_already_applied(&state, "support-sla"));
    }

    #[test]
    fn app_proof_status_ready_when_checks_pass() {
        let checks = vec![AppProofCheck {
            command: None,
            id: "workspace".to_owned(),
            label: "Workspace".to_owned(),
            message: "ok".to_owned(),
            status: "passed".to_owned(),
        }];

        assert_eq!(app_proof_status(&checks, &[]), "ready");
    }

    #[test]
    fn app_proof_status_drifted_when_drift_exists() {
        let checks = vec![AppProofCheck {
            command: Some("lenso app repair".to_owned()),
            id: "workspace-service-support-sla".to_owned(),
            label: "support-sla workspace entry".to_owned(),
            message: "missing".to_owned(),
            status: "drifted".to_owned(),
        }];
        let drifts = vec![AppProofDrift {
            command: Some("lenso app repair".to_owned()),
            message: "support-sla is missing from lenso.workspace.json".to_owned(),
            name: "support-sla".to_owned(),
            resource: "workspace-service".to_owned(),
        }];

        assert_eq!(app_proof_status(&checks, &drifts), "drifted");
    }

    #[test]
    fn app_proof_state_includes_blueprint_addon_and_doctor() {
        let mut launchpad = support_desk_launchpad_state("acme-support");
        launchpad.addons.push(LaunchpadAddon {
            label: "Support SLA".to_owned(),
            modules: vec!["support-sla".to_owned()],
            name: "support-sla".to_owned(),
            services: vec!["support-sla".to_owned()],
            status: "configured".to_owned(),
        });
        let doctor = DevDoctorState {
            checked_at_unix_ms: 1782900000000,
            checks: vec![DevDoctorCheck {
                command: None,
                id: "env".to_owned(),
                label: ".env file".to_owned(),
                message: ".env exists".to_owned(),
                status: "passed".to_owned(),
            }],
            live: false,
            protocol: DEV_DOCTOR_PROTOCOL.to_owned(),
            status: "ready".to_owned(),
        };

        let proof = app_proof_state_from_parts(&launchpad, Some(&doctor), Vec::new(), Vec::new());

        assert_eq!(proof.protocol, APP_PROOF_PROTOCOL);
        assert_eq!(proof.project_name.as_deref(), Some("acme-support"));
        assert_eq!(proof.blueprint.as_deref(), Some("support-desk"));
        assert_eq!(proof.addons, vec!["support-sla"]);
        assert_eq!(proof.status, "ready");
    }

    #[test]
    fn app_diff_detects_missing_workspace_service() {
        let launchpad = support_desk_launchpad_state("acme-support");
        let workspace = json!({
            "protocol": "lenso.service-workspace.v1",
            "services": []
        });

        let (checks, drifts) = app_diff_from_values(&launchpad, Some(&workspace), None).unwrap();

        assert!(
            drifts.iter().any(|drift| {
                drift.resource == "workspace-service" && drift.name == "support-api"
            })
        );
        assert!(checks.iter().any(|check| {
            check.id == "workspace-service-support-api" && check.status == "drifted"
        }));
    }

    #[test]
    fn app_repair_plan_mentions_missing_workspace_service() {
        let drifts = vec![AppProofDrift {
            command: Some("lenso app repair".to_owned()),
            message: "support-sla is missing from lenso.workspace.json".to_owned(),
            name: "support-sla".to_owned(),
            resource: "workspace-service".to_owned(),
        }];

        assert_eq!(
            app_repair_plan(&drifts),
            vec!["restore workspace service support-sla"]
        );
    }

    #[test]
    fn app_repair_plan_does_not_include_source_overwrite() {
        let drifts = vec![AppProofDrift {
            command: Some("manual review".to_owned()),
            message: "service directory exists with user code".to_owned(),
            name: "support-api".to_owned(),
            resource: "service-source".to_owned(),
        }];

        assert!(app_repair_plan(&drifts).is_empty());
    }

    #[test]
    fn agent_context_mentions_app_proof_when_present() {
        let proof = AppProofState {
            addons: vec!["support-sla".to_owned()],
            blueprint: Some("support-desk".to_owned()),
            checked_at_unix_ms: 1782900000000,
            checks: Vec::new(),
            drifts: Vec::new(),
            next_command: Some("lenso app verify --write-proof".to_owned()),
            project_name: Some("acme-support".to_owned()),
            protocol: APP_PROOF_PROTOCOL.to_owned(),
            status: "ready".to_owned(),
        };

        let markdown = agent_context_markdown(None, None, None, None, Some(&proof), None).unwrap();

        assert!(markdown.contains("## App Proof"));
        assert!(markdown.contains("Status: ready"));
        assert!(markdown.contains("Existing service source files are user code."));
    }

    #[test]
    fn agent_context_mentions_boundaries_and_task() {
        let state = support_desk_launchpad_state("support-desk");
        let markdown = agent_context_markdown(
            Some(&state),
            Some(&support_desk_system("support-desk")),
            Some(&json!({"protocol": "lenso.service-workspace.v1", "services": []})),
            None,
            None,
            Some("Add ticket SLA fields."),
        )
        .unwrap();

        assert!(markdown.contains("# Lenso Agent Context"));
        assert!(markdown.contains("Host owns auth"));
        assert!(markdown.contains("Services are remote processes"));
        assert!(markdown.contains("Add ticket SLA fields."));
    }

    #[test]
    fn agent_context_mentions_dev_doctor_when_present() {
        let state = support_desk_launchpad_state("support-desk");
        let doctor = json!({
            "protocol": DEV_DOCTOR_PROTOCOL,
            "status": "ready",
            "checks": [
                { "id": "env-file", "status": "passed" },
                { "id": "pnpm", "status": "needs_attention" }
            ]
        });
        let markdown = agent_context_markdown(
            Some(&state),
            None,
            None,
            Some(&doctor),
            None,
            Some("Add overdue ticket escalation."),
        )
        .unwrap();

        assert!(markdown.contains("## Dev Doctor"));
        assert!(markdown.contains("- Status: ready"));
        assert!(markdown.contains("- Checks: 2"));
        assert!(markdown.contains("- Needs attention: 1"));
    }

    #[test]
    fn doctor_status_prioritizes_failed_then_attention() {
        assert_eq!(
            doctor_status(&[DevDoctorCheck {
                command: None,
                id: "env".to_owned(),
                label: "Env".to_owned(),
                message: "missing".to_owned(),
                status: "failed".to_owned(),
            }]),
            "failed"
        );
        assert_eq!(
            doctor_status(&[DevDoctorCheck {
                command: None,
                id: "pnpm".to_owned(),
                label: "pnpm".to_owned(),
                message: "not found".to_owned(),
                status: "needs_attention".to_owned(),
            }]),
            "needs_attention"
        );
    }
}
