use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{ServiceLanguage, host, service};

const LAUNCHPAD_PROTOCOL: &str = "lenso.launchpad.v1";
const LAUNCHPAD_FILE: &str = ".lenso/launchpad.json";
const SYSTEM_FILE: &str = "lenso.system.json";
const WORKSPACE_FILE: &str = "lenso.workspace.json";
const SUPPORT_DESK_BLUEPRINT: &str = "support-desk";

#[derive(Debug, Clone)]
pub(crate) struct AppCreateOptions {
    pub(crate) blueprint: String,
    pub(crate) dir: PathBuf,
    pub(crate) force: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct DevStatusOptions {
    pub(crate) repo_root: Option<PathBuf>,
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

pub(crate) fn create_app(options: AppCreateOptions) -> Result<()> {
    if options.blueprint != SUPPORT_DESK_BLUEPRINT {
        bail!(
            "unknown Launchpad blueprint `{}`; supported blueprint: {}",
            options.blueprint,
            SUPPORT_DESK_BLUEPRINT
        );
    }

    let project_name = project_name_from_dir(&options.dir);
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let target = absolutize_from(&current_dir, &options.dir);
    let target_display = target.to_string_lossy().to_string();

    host::init(&target_display, Some(&project_name), options.force)?;
    with_current_dir(&target, || create_support_desk_files(&project_name))?;

    println!();
    println!("Created Launchpad app {project_name}.");
    println!("Next steps:");
    println!("- cd {}", display_relative(&current_dir, &target));
    println!("- lenso dev status");
    println!("- lenso dev up");
    println!("- lenso agent context");
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

pub(crate) fn agent_context(options: AgentContextOptions) -> Result<()> {
    let repo_root = options.repo_root.unwrap_or_else(|| PathBuf::from("."));
    let state = read_launchpad_state_optional(&repo_root)?;
    let system = read_json_value_optional(&repo_root.join(SYSTEM_FILE))?;
    let workspace = read_json_value_optional(&repo_root.join(WORKSPACE_FILE))?;
    let markdown = agent_context_markdown(
        state.as_ref(),
        system.as_ref(),
        workspace.as_ref(),
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

fn create_support_desk_files(project_name: &str) -> Result<()> {
    if ensure_env_file()? {
        println!("Prepared .env from .env.example.");
    }
    service::create_service(service::ServiceCreateOptions {
        dry_run: false,
        lang: ServiceLanguage::Ts,
        name: "support-api".to_owned(),
        no_workspace: false,
        output_dir: Some(PathBuf::from("services")),
        port: 4110,
        workspace_file: None,
    })?;
    service::create_service(service::ServiceCreateOptions {
        dry_run: false,
        lang: ServiceLanguage::Rust,
        name: "notification-worker".to_owned(),
        no_workspace: false,
        output_dir: Some(PathBuf::from("services")),
        port: 4120,
        workspace_file: None,
    })?;

    write_json(Path::new(SYSTEM_FILE), &support_desk_system(project_name))?;
    write_json(
        Path::new(LAUNCHPAD_FILE),
        &support_desk_launchpad_state(project_name),
    )
}

fn support_desk_launchpad_state(project_name: &str) -> LaunchpadState {
    LaunchpadState {
        protocol: LAUNCHPAD_PROTOCOL.to_owned(),
        project_name: project_name.to_owned(),
        blueprint: SUPPORT_DESK_BLUEPRINT.to_owned(),
        status: "configured".to_owned(),
        summary: "Support desk app with one TypeScript API service and one Rust worker service."
            .to_owned(),
        services: vec![
            LaunchpadService {
                name: "support-api".to_owned(),
                role: "ticket intake and admin HTTP actions".to_owned(),
                language: "ts".to_owned(),
                cwd: "services/support-api".to_owned(),
                command: "pnpm start".to_owned(),
                manifest: "http://127.0.0.1:4110/lenso/service/v1/manifest".to_owned(),
                ready_url: "http://127.0.0.1:4110/lenso/service/v1/status".to_owned(),
                modules: vec!["support-api".to_owned()],
            },
            LaunchpadService {
                name: "notification-worker".to_owned(),
                role: "notification and background service functions".to_owned(),
                language: "rust".to_owned(),
                cwd: "services/notification-worker".to_owned(),
                command: "cargo run".to_owned(),
                manifest: "http://127.0.0.1:4120/lenso/service/v1/manifest".to_owned(),
                ready_url: "http://127.0.0.1:4120/lenso/service/v1/status".to_owned(),
                modules: vec!["notification-worker".to_owned()],
            },
        ],
        modules: vec![
            LaunchpadModule {
                name: "support-api".to_owned(),
                owner_service: "support-api".to_owned(),
                capability: "support.tickets".to_owned(),
            },
            LaunchpadModule {
                name: "notification-worker".to_owned(),
                owner_service: "notification-worker".to_owned(),
                capability: "support.notifications".to_owned(),
            },
        ],
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
                label: "TypeScript and Rust services scaffolded".to_owned(),
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
    }
}

fn support_desk_system(project_name: &str) -> Value {
    json!({
        "protocol": "lenso.system.v1",
        "name": project_name,
        "environments": ["local"],
        "services": [
            {
                "name": "support-api",
                "target": "local",
                "modules": ["support-api"],
                "cwd": "services/support-api",
                "manifest": "http://127.0.0.1:4110/lenso/service/v1/manifest",
                "command": "pnpm start",
                "lang": "ts",
                "readyUrl": "http://127.0.0.1:4110/lenso/service/v1/status"
            },
            {
                "name": "notification-worker",
                "target": "local",
                "modules": ["notification-worker"],
                "cwd": "services/notification-worker",
                "manifest": "http://127.0.0.1:4120/lenso/service/v1/manifest",
                "command": "cargo run",
                "lang": "rust",
                "readyUrl": "http://127.0.0.1:4120/lenso/service/v1/status"
            }
        ],
        "modules": [
            {
                "name": "auth",
                "installTo": "host",
                "capabilities": ["auth"]
            },
            {
                "name": "support-api",
                "installTo": "service:support-api",
                "capabilities": [
                    "support.tickets.read",
                    "support.tickets.write"
                ],
                "dependencies": ["auth"]
            },
            {
                "name": "notification-worker",
                "installTo": "service:notification-worker",
                "capabilities": ["support.notifications.send"],
                "dependencies": ["support.tickets.read"]
            }
        ],
        "dependencies": [
            {
                "from": "notification-worker",
                "to": "support-api",
                "capability": "support.tickets.read"
            }
        ]
    })
}

fn agent_context_markdown(
    state: Option<&LaunchpadState>,
    system: Option<&Value>,
    workspace: Option<&Value>,
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

fn read_json_value_optional(path: &Path) -> Result<Option<Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("parse {}", path.display()))
        .map(Some)
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
        .unwrap_or(SUPPORT_DESK_BLUEPRINT)
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
        assert_eq!(state.blueprint, SUPPORT_DESK_BLUEPRINT);
        assert_eq!(state.services.len(), 2);
        assert_eq!(state.modules.len(), 2);
        assert_eq!(state.commands.dev_up, "lenso dev up");
    }

    #[test]
    fn agent_context_mentions_boundaries_and_task() {
        let state = support_desk_launchpad_state("support-desk");
        let markdown = agent_context_markdown(
            Some(&state),
            Some(&support_desk_system("support-desk")),
            Some(&json!({"protocol": "lenso.service-workspace.v1", "services": []})),
            Some("Add ticket SLA fields."),
        )
        .unwrap();

        assert!(markdown.contains("# Lenso Agent Context"));
        assert!(markdown.contains("Host owns auth"));
        assert!(markdown.contains("Services are remote processes"));
        assert!(markdown.contains("Add ticket SLA fields."));
    }
}
