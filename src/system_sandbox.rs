#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result};
use lenso_service::{
    ContractSemanticKind, SystemV2Graph, WorkloadRole, check_contract_artifact_value,
    system_v2_graph,
};
#[cfg(unix)]
use nix::{
    errno::Errno,
    sys::signal::{Signal, kill, killpg},
    unistd::Pid,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt as _, BufReader},
    process::{Child, Command},
    time::Instant,
};
use uuid::Uuid;

mod scenario;

const DEFAULT_SYSTEM_FILE: &str = "lenso.system.json";
const DEFAULT_SANDBOX_FILE: &str = "lenso.system-sandbox.json";
const SANDBOX_PROTOCOL: &str = "lenso.system-sandbox.v1";
const PLAN_PROTOCOL: &str = "lenso.system-sandbox-plan.v1";
const STATE_PROTOCOL: &str = "lenso.system-sandbox-state.v1";
const OWNER_PROTOCOL: &str = "lenso.system-sandbox-owner.v1";

#[derive(Debug, Clone)]
pub(crate) struct SystemDevOptions {
    pub(crate) cleanup: bool,
    pub(crate) dry_run: bool,
    pub(crate) json: bool,
    pub(crate) sandbox_file: Option<PathBuf>,
    pub(crate) scenario: Option<String>,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SandboxDefinition {
    protocol: String,
    services: Vec<SandboxService>,
    #[serde(default)]
    scenarios: Vec<scenario::FailureScenario>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SandboxService {
    service_id: String,
    workloads: Vec<SandboxWorkload>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SandboxWorkload {
    workload_id: String,
    command: Vec<String>,
    #[serde(default)]
    scenario_command: Vec<String>,
    #[serde(default)]
    cwd: Option<PathBuf>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    health_url: Option<String>,
    #[serde(default = "default_health_timeout_ms")]
    health_timeout_ms: u64,
}

const fn default_health_timeout_ms() -> u64 {
    30_000
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxPlan {
    protocol: String,
    system_id: String,
    system_file: PathBuf,
    sandbox_file: PathBuf,
    owned_root: PathBuf,
    services: Vec<PlannedService>,
    workloads: Vec<PlannedWorkload>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlannedService {
    service_id: String,
    store_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlannedWorkload {
    service_id: String,
    workload_id: String,
    role: WorkloadRole,
    identity: String,
    command: Vec<String>,
    scenario_command: Vec<String>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
    endpoint: Option<String>,
    health_url: Option<String>,
    health_timeout_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SandboxPhase {
    Starting,
    Ready,
    Completed,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SandboxState {
    protocol: String,
    ownership_token: String,
    system_id: String,
    phase: SandboxPhase,
    workloads: Vec<WorkloadState>,
    endpoints: Vec<EndpointState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkloadState {
    service_id: String,
    workload_id: String,
    role: WorkloadRole,
    identity: String,
    phase: SandboxPhase,
    process_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EndpointState {
    service_id: String,
    workload_id: String,
    endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnerMarker {
    protocol: String,
    ownership_token: String,
    system_id: String,
}

#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
struct SandboxError {
    artifact_version: &'static str,
    code: &'static str,
    message: String,
    next_action: String,
}

impl SandboxError {
    fn new(code: &'static str, message: impl Into<String>, next_action: impl Into<String>) -> Self {
        Self {
            artifact_version: "lenso.command-error.v1",
            code,
            message: message.into(),
            next_action: next_action.into(),
        }
    }
}

#[derive(Debug)]
struct OwnedProcess {
    state_index: usize,
    process_group_id: u32,
    child: Child,
}

#[derive(Debug)]
struct RunningSandbox {
    plan: SandboxPlan,
    state: SandboxState,
    processes: Vec<OwnedProcess>,
}

pub(crate) async fn dev_system(options: SystemDevOptions) -> Result<()> {
    #[cfg(not(unix))]
    return Err(command_error(
        SandboxError::new(
            "unsupported_platform",
            "System Sandbox process ownership currently requires a Unix platform.",
            "Run System Sandbox on macOS or Linux.",
        ),
        options.json,
    ));

    let current = std::env::current_dir().context("resolve current directory")?;
    let system_file = absolute(
        &current,
        options
            .system_file
            .as_deref()
            .unwrap_or(Path::new(DEFAULT_SYSTEM_FILE)),
    );
    let system_dir = system_file.parent().unwrap_or(Path::new("."));
    let sandbox_file = absolute(
        system_dir,
        options
            .sandbox_file
            .as_deref()
            .unwrap_or(Path::new(DEFAULT_SANDBOX_FILE)),
    );
    let system: Value = read_typed(&system_file, "System").map_err(|error| {
        command_error(
            input_error("system_artifact_invalid", &system_file, &error),
            options.json,
        )
    })?;
    validate_options(&options).map_err(|error| command_error(error, options.json))?;
    let system_id = validate_system(&system).map_err(|error| command_error(error, options.json))?;
    let owned_root = system_dir.join(".lenso/system-sandbox").join(system_id);

    if options.cleanup {
        cleanup_recorded(&owned_root)
            .await
            .map_err(|error| command_error(error, options.json))?;
        print_status(system_id, SandboxPhase::Stopped, options.json)?;
        return Ok(());
    }

    let definition: SandboxDefinition =
        read_typed(&sandbox_file, "System Sandbox").map_err(|error| {
            command_error(
                input_error("sandbox_artifact_invalid", &sandbox_file, &error),
                options.json,
            )
        })?;
    let plan = build_plan(&system, &definition, &system_file, &sandbox_file)
        .map_err(|error| command_error(error, options.json))?;
    if let Some(scenario_id) = options.scenario.as_deref() {
        scenario::ensure_declared(&definition.scenarios, scenario_id)
            .map_err(|error| command_error(error, options.json))?;
    }
    if options.dry_run {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }

    let mut running = launch(plan, options.json)
        .await
        .map_err(|error| command_error(error, options.json))?;
    if let Some(scenario_id) = options.scenario.as_deref() {
        let result = scenario::run(&mut running, &definition.scenarios, scenario_id)
            .await
            .map_err(|error| command_error(error, options.json))?;
        if options.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!(
                "Failure Scenario {}: {}",
                result.scenario_id, result.outcome
            );
        }
        return Ok(());
    }
    if options.json {
        println!("{}", serde_json::to_string_pretty(&running.state)?);
    } else {
        println!("System Sandbox {}: ready", running.state.system_id);
    }
    if let Err(error) = running.wait_for_stop().await {
        return Err(command_error(
            rollback_launch(&mut running, error).await,
            options.json,
        ));
    }
    running
        .shutdown()
        .await
        .map_err(|error| command_error(error, options.json))
}

fn validate_options(options: &SystemDevOptions) -> std::result::Result<(), SandboxError> {
    if options.cleanup && options.dry_run {
        return Err(SandboxError::new(
            "conflicting_options",
            "cleanup and dry-run cannot be used together.",
            "Choose either validation or cleanup.",
        ));
    }
    if options.scenario.is_some() && (options.cleanup || options.dry_run) {
        return Err(SandboxError::new(
            "conflicting_options",
            "scenario cannot be combined with cleanup or dry-run.",
            "Run the scenario by itself; it performs deterministic cleanup automatically.",
        ));
    }
    Ok(())
}

fn validate_system(system: &Value) -> std::result::Result<&str, SandboxError> {
    let system_id = system
        .get("systemId")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_definition("System v2 must declare systemId."))?;
    if !is_safe_identity(system_id) {
        return Err(SandboxError::new(
            "unsafe_system_identity",
            format!("System identity is not a safe path component: {system_id}"),
            "Use only ASCII letters, numbers, dot, underscore, and dash in systemId.",
        ));
    }
    if let Some(services) = system.get("autonomousServices").and_then(Value::as_array) {
        for service in services {
            if let Some(service_id) = service.get("serviceId").and_then(Value::as_str)
                && !is_safe_identity(service_id)
            {
                return Err(SandboxError::new(
                    "unsafe_service_identity",
                    format!("Service identity is not a safe path component: {service_id}"),
                    "Use only ASCII letters, numbers, dot, underscore, and dash in serviceId.",
                ));
            }
        }
    }
    let check = check_contract_artifact_value(system).map_err(|error| {
        SandboxError::new(
            "system_validation_failed",
            error.to_string(),
            "Fix the System v2 definition and rerun the command.",
        )
    })?;
    if check.semantic_kind != ContractSemanticKind::MixedSystem {
        return Err(SandboxError::new(
            "unsupported_system",
            "System Sandbox requires a lenso.system.v2 mixed System definition.",
            "Use a System v2 definition with Autonomous Services.",
        ));
    }
    Ok(system_id)
}

fn is_safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn build_plan(
    system: &Value,
    definition: &SandboxDefinition,
    system_file: &Path,
    sandbox_file: &Path,
) -> std::result::Result<SandboxPlan, SandboxError> {
    validate_system(system)?;
    let declared = declared_workloads(system)?;
    let graph = system_v2_graph(system).map_err(|issues| {
        SandboxError::new(
            "system_validation_failed",
            format!("{issues:?}"),
            "Fix the reported System graph issues and rerun the dry-run.",
        )
    })?;
    if definition.protocol != SANDBOX_PROTOCOL {
        return Err(invalid_definition(format!(
            "Unsupported sandbox protocol {}.",
            definition.protocol
        )));
    }

    let configured = configured_workloads(definition)?;
    scenario::validate(&definition.scenarios, &configured, &declared)?;
    let declared_keys = declared.keys().collect::<BTreeSet<_>>();
    let configured_keys = configured.keys().collect::<BTreeSet<_>>();
    if declared_keys != configured_keys {
        return Err(SandboxError::new(
            "sandbox_workload_mismatch",
            format!(
                "Sandbox Workloads do not match System Workloads. System: {declared_keys:?}; sandbox: {configured_keys:?}."
            ),
            "Give every System Workload exactly one local launch declaration.",
        ));
    }

    let system_id = graph.system_id.clone();
    let system_dir = system_file.parent().unwrap_or(Path::new("."));
    let sandbox_dir = sandbox_file.parent().unwrap_or(system_dir);
    let owned_root = system_dir.join(".lenso/system-sandbox").join(&system_id);
    validate_owned_root(&owned_root, system_dir)?;
    if owned_root.exists() {
        return Err(SandboxError::new(
            "sandbox_already_exists",
            format!("Sandbox state already exists: {}", owned_root.display()),
            "Run the cleanup command before starting another sandbox for this System.",
        ));
    }
    let services_in_system = declared
        .keys()
        .map(|(service, _)| service.as_str())
        .collect::<BTreeSet<_>>();
    let service_order = service_dependency_order(&graph, &services_in_system)?;
    let mut services = Vec::new();
    let mut workloads = Vec::new();
    for service_id in service_order {
        let store_path = owned_root.join("services").join(&service_id).join("store");
        services.push(PlannedService {
            service_id: service_id.clone(),
            store_path: store_path.clone(),
        });
        let mut items = declared
            .iter()
            .filter(|((service, _), _)| service == &service_id)
            .collect::<Vec<_>>();
        items.sort_by_key(|((_, workload), role)| (role_order(role), workload.as_str()));
        for ((_, workload_id), role) in items {
            let config = configured[&(service_id.clone(), workload_id.clone())];
            validate_workload(config, sandbox_dir)?;
            let cwd = config.cwd.as_deref().map_or_else(
                || sandbox_dir.to_path_buf(),
                |path| absolute(sandbox_dir, path),
            );
            let identity = format!("local-dev://{system_id}/{service_id}/{workload_id}");
            let mut env = config.env.clone();
            env.insert("LENSO_SYSTEM_ID".to_owned(), system_id.clone());
            env.insert("LENSO_SERVICE_ID".to_owned(), service_id.clone());
            env.insert("LENSO_WORKLOAD_ID".to_owned(), workload_id.clone());
            env.insert("LENSO_WORKLOAD_IDENTITY".to_owned(), identity.clone());
            env.insert(
                "LENSO_SERVICE_STORE_PATH".to_owned(),
                store_path.display().to_string(),
            );
            workloads.push(PlannedWorkload {
                service_id: service_id.clone(),
                workload_id: workload_id.clone(),
                role: role.clone(),
                identity,
                command: config.command.clone(),
                scenario_command: config.scenario_command.clone(),
                cwd,
                env,
                endpoint: config.endpoint.clone(),
                health_url: config.health_url.clone(),
                health_timeout_ms: config.health_timeout_ms,
            });
        }
    }
    Ok(SandboxPlan {
        protocol: PLAN_PROTOCOL.to_owned(),
        system_id,
        system_file: system_file.to_path_buf(),
        sandbox_file: sandbox_file.to_path_buf(),
        owned_root,
        services,
        workloads,
    })
}

fn declared_workloads(
    system: &Value,
) -> std::result::Result<BTreeMap<(String, String), WorkloadRole>, SandboxError> {
    let services = system
        .get("autonomousServices")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_definition("autonomousServices must be an array."))?;
    if services.is_empty() {
        return Err(invalid_definition(
            "System Sandbox requires at least one Autonomous Service.",
        ));
    }
    let mut result = BTreeMap::new();
    for service in services {
        let service_id = required_string(service, "serviceId")?;
        if !is_safe_identity(&service_id) {
            return Err(SandboxError::new(
                "unsafe_service_identity",
                format!("Service identity is not a safe path component: {service_id}"),
                "Use only ASCII letters, numbers, dot, underscore, and dash in serviceId.",
            ));
        }
        let workloads = service
            .get("workloads")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid_definition("workloads must be an array."))?;
        let mut roles = BTreeSet::new();
        for workload in workloads {
            let role = WorkloadRole::new(required_string(workload, "role")?);
            roles.insert(role.as_str().to_owned());
            result.insert(
                (service_id.clone(), required_string(workload, "workloadId")?),
                role,
            );
        }
        for required in ["api", "worker", "migration"] {
            if !roles.contains(required) {
                return Err(invalid_definition(format!(
                    "Autonomous Service {service_id} must declare a {required} Workload."
                )));
            }
        }
    }
    Ok(result)
}

fn configured_workloads(
    definition: &SandboxDefinition,
) -> std::result::Result<BTreeMap<(String, String), &SandboxWorkload>, SandboxError> {
    if definition.services.is_empty() {
        return Err(invalid_definition(
            "System Sandbox requires at least one Service launch declaration.",
        ));
    }
    let mut result = BTreeMap::new();
    for service in &definition.services {
        if !is_safe_identity(&service.service_id) {
            return Err(SandboxError::new(
                "unsafe_service_identity",
                format!(
                    "Service identity is not a safe path component: {}",
                    service.service_id
                ),
                "Use only ASCII letters, numbers, dot, underscore, and dash in serviceId.",
            ));
        }
        if service.workloads.is_empty() {
            return Err(invalid_definition(format!(
                "Service {} has no Workload launch declarations.",
                service.service_id
            )));
        }
        for workload in &service.workloads {
            if result
                .insert(
                    (service.service_id.clone(), workload.workload_id.clone()),
                    workload,
                )
                .is_some()
            {
                return Err(invalid_definition(format!(
                    "Workload {}/{} is duplicated.",
                    service.service_id, workload.workload_id
                )));
            }
        }
    }
    Ok(result)
}

fn required_string(value: &Value, field: &str) -> std::result::Result<String, SandboxError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| invalid_definition(format!("{field} must be a non-empty string.")))
}

fn invalid_definition(message: impl Into<String>) -> SandboxError {
    SandboxError::new(
        "invalid_sandbox_definition",
        message,
        "Fix the System Sandbox definition and rerun the dry-run.",
    )
}

fn validate_workload(
    workload: &SandboxWorkload,
    sandbox_dir: &Path,
) -> std::result::Result<(), SandboxError> {
    if workload.command.is_empty() || workload.command[0].is_empty() {
        return Err(invalid_definition(format!(
            "Workload {} must declare a command array.",
            workload.workload_id
        )));
    }
    let cwd = workload.cwd.as_deref().map_or_else(
        || sandbox_dir.to_path_buf(),
        |path| absolute(sandbox_dir, path),
    );
    if !cwd.is_dir() {
        return Err(SandboxError::new(
            "workload_cwd_missing",
            format!(
                "Workload {} cwd does not exist: {}",
                workload.workload_id,
                cwd.display()
            ),
            "Create the directory or correct cwd, then rerun the dry-run.",
        ));
    }
    if !command_exists(&workload.command[0], &cwd, &workload.env) {
        return Err(SandboxError::new(
            "workload_command_missing",
            format!(
                "Workload {} executable was not found: {}",
                workload.workload_id, workload.command[0]
            ),
            "Install the executable or correct command, then rerun the dry-run.",
        ));
    }
    if let Some(command) = workload.scenario_command.first()
        && !command_exists(command, &cwd, &workload.env)
    {
        return Err(SandboxError::new(
            "scenario_command_missing",
            format!(
                "Workload {} scenario executable was not found: {command}",
                workload.workload_id
            ),
            "Install the executable or correct scenarioCommand, then rerun the dry-run.",
        ));
    }
    for (field, value) in [
        ("endpoint", workload.endpoint.as_deref()),
        ("healthUrl", workload.health_url.as_deref()),
    ] {
        if let Some(url) = value {
            let parsed = reqwest::Url::parse(url).map_err(|error| {
                SandboxError::new(
                    "invalid_workload_url",
                    format!(
                        "Workload {} {field} is invalid: {error}",
                        workload.workload_id
                    ),
                    format!("Set {field} to an absolute HTTP URL."),
                )
            })?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(SandboxError::new(
                    "invalid_workload_url",
                    format!("Workload {} {field} must use HTTP.", workload.workload_id),
                    format!("Set {field} to an absolute HTTP URL."),
                ));
            }
        }
    }
    Ok(())
}

fn command_exists(command: &str, cwd: &Path, env: &BTreeMap<String, String>) -> bool {
    if command.contains(std::path::MAIN_SEPARATOR) {
        return absolute(cwd, Path::new(command)).is_file();
    }
    let path = env
        .get("PATH")
        .cloned()
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default();
    std::env::split_paths(&path).any(|directory| directory.join(command).is_file())
}

fn validate_owned_root(root: &Path, system_dir: &Path) -> std::result::Result<(), SandboxError> {
    let expected_parent = system_dir.join(".lenso/system-sandbox");
    if root.parent() != Some(expected_parent.as_path()) {
        return Err(SandboxError::new(
            "unsafe_sandbox_root",
            format!("Refusing unsafe sandbox root: {}", root.display()),
            "Keep sandbox state under the System .lenso directory.",
        ));
    }
    Ok(())
}

fn service_dependency_order(
    graph: &SystemV2Graph,
    services: &BTreeSet<&str>,
) -> std::result::Result<Vec<String>, SandboxError> {
    let owners = graph
        .nodes
        .iter()
        .filter_map(|node| {
            node.owner
                .as_ref()
                .map(|owner| (node.id.as_str(), owner.as_str()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut remaining = services
        .iter()
        .map(|service| ((*service).to_owned(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for relationship in &graph.relationships {
        if relationship.kind != "consumes" {
            continue;
        }
        let (Some(consumer), Some(producer)) = (
            owners.get(relationship.from.as_str()),
            owners.get(relationship.to.as_str()),
        ) else {
            continue;
        };
        if consumer != producer && services.contains(consumer) && services.contains(producer) {
            remaining
                .get_mut(*consumer)
                .expect("consumer Service exists")
                .insert((*producer).to_owned());
        }
    }
    let mut ordered = Vec::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .find(|(_, dependencies)| dependencies.iter().all(|item| ordered.contains(item)))
            .map(|(service, _)| service.clone());
        let Some(service) = ready else {
            return Err(SandboxError::new(
                "dependency_cycle",
                "Autonomous Service contract dependencies contain a cycle.",
                "Remove the contract cycle and rerun the dry-run.",
            ));
        };
        remaining.remove(&service);
        ordered.push(service);
    }
    Ok(ordered)
}

const fn role_order(role: &WorkloadRole) -> u8 {
    match role {
        WorkloadRole::Migration => 0,
        WorkloadRole::Api => 1,
        WorkloadRole::Worker => 2,
        WorkloadRole::Other(_) => 3,
    }
}

async fn launch(
    plan: SandboxPlan,
    json_logs: bool,
) -> std::result::Result<RunningSandbox, SandboxError> {
    if plan.owned_root.exists() {
        return Err(SandboxError::new(
            "sandbox_already_exists",
            format!(
                "Sandbox state already exists: {}",
                plan.owned_root.display()
            ),
            "Run the cleanup command before starting another sandbox for this System.",
        ));
    }
    let token = Uuid::now_v7().to_string();
    let mut running = RunningSandbox {
        state: SandboxState {
            protocol: STATE_PROTOCOL.to_owned(),
            ownership_token: token.clone(),
            system_id: plan.system_id.clone(),
            phase: SandboxPhase::Starting,
            workloads: plan
                .workloads
                .iter()
                .map(|workload| WorkloadState {
                    service_id: workload.service_id.clone(),
                    workload_id: workload.workload_id.clone(),
                    role: workload.role.clone(),
                    identity: workload.identity.clone(),
                    phase: SandboxPhase::Starting,
                    process_id: None,
                })
                .collect(),
            endpoints: Vec::new(),
        },
        plan,
        processes: Vec::new(),
    };
    allocate(&running.plan, &token).await?;
    if let Err(error) = persist_state(&running.plan.owned_root, &running.state).await {
        rollback_root(&running.plan.owned_root).await;
        return Err(error);
    }
    for index in 0..running.plan.workloads.len() {
        if let Err(error) = launch_workload(&mut running, index, json_logs).await {
            running.state.phase = SandboxPhase::Failed;
            return Err(rollback_launch(&mut running, error).await);
        }
        if let Err(error) = persist_state(&running.plan.owned_root, &running.state).await {
            return Err(rollback_launch(&mut running, error).await);
        }
    }
    running.state.phase = SandboxPhase::Ready;
    if let Err(error) = persist_state(&running.plan.owned_root, &running.state).await {
        return Err(rollback_launch(&mut running, error).await);
    }
    Ok(running)
}

async fn allocate(plan: &SandboxPlan, token: &str) -> std::result::Result<(), SandboxError> {
    let parent = plan.owned_root.parent().ok_or_else(|| {
        SandboxError::new(
            "unsafe_sandbox_root",
            "Sandbox root has no parent.",
            "Keep sandbox state under the System .lenso directory.",
        )
    })?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| io_error("store_allocation_failed", "create sandbox parent", &error))?;
    if let Err(error) = tokio::fs::create_dir(&plan.owned_root).await {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return Err(SandboxError::new(
                "sandbox_already_exists",
                format!(
                    "Sandbox state already exists: {}",
                    plan.owned_root.display()
                ),
                "Run the cleanup command before starting another sandbox for this System.",
            ));
        }
        return Err(io_error(
            "store_allocation_failed",
            "claim sandbox root",
            &error,
        ));
    }
    if let Err(error) = write_json_async(
        &plan.owned_root.join(".owner.json"),
        &OwnerMarker {
            protocol: OWNER_PROTOCOL.to_owned(),
            ownership_token: token.to_owned(),
            system_id: plan.system_id.clone(),
        },
    )
    .await
    {
        rollback_root(&plan.owned_root).await;
        return Err(error);
    }
    for service in &plan.services {
        if let Err(error) = tokio::fs::create_dir_all(&service.store_path).await {
            let error = io_error(
                "store_allocation_failed",
                "allocate isolated Service Store",
                &error,
            );
            rollback_root(&plan.owned_root).await;
            return Err(error);
        }
    }
    Ok(())
}

async fn launch_workload(
    running: &mut RunningSandbox,
    index: usize,
    json_logs: bool,
) -> std::result::Result<(), SandboxError> {
    let workload = &running.plan.workloads[index];
    let mut command = Command::new(&workload.command[0]);
    command
        .args(&workload.command[1..])
        .current_dir(&workload.cwd)
        .envs(&workload.env)
        .env(
            "LENSO_SANDBOX_OWNERSHIP_TOKEN",
            &running.state.ownership_token,
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.as_std_mut().process_group(0);
    let mut child = command.spawn().map_err(|error| {
        SandboxError::new(
            "process_start_failed",
            format!(
                "Could not start {}/{}: {error}",
                workload.service_id, workload.workload_id
            ),
            "Fix the Workload command and rerun the dry-run.",
        )
    })?;
    let process_group_id = child.id().ok_or_else(|| {
        SandboxError::new(
            "process_start_failed",
            format!(
                "Workload {}/{} started without a process id.",
                workload.service_id, workload.workload_id
            ),
            "Inspect the Workload command and retry.",
        )
    })?;
    running.state.workloads[index].process_id = Some(process_group_id);
    capture_logs(&mut child, workload, json_logs);
    if workload.role == WorkloadRole::Migration {
        let status = match child.wait().await {
            Ok(status) => status,
            Err(error) => {
                let original = SandboxError::new(
                    "process_status_failed",
                    format!("Could not observe Migration Workload: {error}"),
                    "Inspect the correlated logs and retry.",
                );
                return cleanup_started_workload(&mut child, process_group_id, original).await;
            }
        };
        if !status.success() {
            let original = SandboxError::new(
                "process_exited",
                format!(
                    "Migration Workload {}/{} exited with {status}.",
                    workload.service_id, workload.workload_id
                ),
                "Fix the migration error in the correlated logs and retry.",
            );
            return cleanup_started_workload(&mut child, process_group_id, original).await;
        }
        stop_owned_child(&mut child, process_group_id)
            .await
            .map_err(|cleanup| {
                SandboxError::new(
                    "cleanup_incomplete",
                    format!("Migration cleanup failed: {cleanup}"),
                    "Stop the reported process group, then rerun cleanup.",
                )
            })?;
        running.state.workloads[index].phase = SandboxPhase::Completed;
        running.state.workloads[index].process_id = None;
        return Ok(());
    }
    if let Err(error) = wait_for_health(&mut child, workload).await {
        return cleanup_started_workload(&mut child, process_group_id, error).await;
    }
    running.state.workloads[index].phase = SandboxPhase::Ready;
    if let Some(endpoint) = &workload.endpoint {
        running.state.endpoints.push(EndpointState {
            service_id: workload.service_id.clone(),
            workload_id: workload.workload_id.clone(),
            endpoint: endpoint.clone(),
        });
    }
    running.processes.push(OwnedProcess {
        state_index: index,
        process_group_id,
        child,
    });
    Ok(())
}

async fn cleanup_started_workload(
    child: &mut Child,
    process_group_id: u32,
    original: SandboxError,
) -> std::result::Result<(), SandboxError> {
    match stop_owned_child(child, process_group_id).await {
        Ok(()) => Err(original),
        Err(cleanup) => Err(SandboxError::new(
            "cleanup_incomplete",
            format!("{} Cleanup also failed: {cleanup}", original.message),
            "Stop the reported process group, then rerun cleanup.",
        )),
    }
}

fn capture_logs(child: &mut Child, workload: &PlannedWorkload, json: bool) {
    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(stdout, workload, "stdout", json);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(stderr, workload, "stderr", json);
    }
}

fn spawn_log_reader<R>(reader: R, workload: &PlannedWorkload, stream: &'static str, json: bool)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let service_id = workload.service_id.clone();
    let workload_id = workload.workload_id.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(reader).lines();
        while let Ok(Some(message)) = lines.next_line().await {
            if json {
                let value = serde_json::json!({
                    "artifactVersion": "lenso.system-sandbox-log.v1",
                    "serviceId": service_id,
                    "workloadId": workload_id,
                    "stream": stream,
                    "message": message,
                });
                println!("{value}");
            } else {
                println!("[{service_id}/{workload_id} {stream}] {message}");
            }
        }
    });
}

async fn wait_for_health(
    child: &mut Child,
    workload: &PlannedWorkload,
) -> std::result::Result<(), SandboxError> {
    let deadline = Instant::now() + Duration::from_millis(workload.health_timeout_ms);
    let client = reqwest::Client::new();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| {
            SandboxError::new(
                "process_status_failed",
                error.to_string(),
                "Inspect the correlated logs and retry.",
            )
        })? {
            return Err(SandboxError::new(
                "process_exited",
                format!(
                    "Workload {}/{} exited with {status} before readiness.",
                    workload.service_id, workload.workload_id
                ),
                "Fix the startup error in the correlated logs and retry.",
            ));
        }
        let ready = if let Some(url) = &workload.health_url {
            client
                .get(url)
                .send()
                .await
                .is_ok_and(|response| response.status().is_success())
        } else {
            true
        };
        if ready {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(SandboxError::new(
                "health_timeout",
                format!(
                    "Workload {}/{} did not become healthy within {}ms.",
                    workload.service_id, workload.workload_id, workload.health_timeout_ms
                ),
                "Inspect the correlated logs and healthUrl, then retry.",
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

impl RunningSandbox {
    async fn wait_for_stop(&mut self) -> std::result::Result<(), SandboxError> {
        loop {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => {
                    signal.map_err(|error| SandboxError::new(
                        "shutdown_signal_failed",
                        error.to_string(),
                        "Stop the sandbox with the cleanup command.",
                    ))?;
                    return Ok(());
                }
                () = tokio::time::sleep(Duration::from_millis(200)) => {
                    for process in &mut self.processes {
                        if let Some(status) = process.child.try_wait().map_err(|error| {
                            SandboxError::new(
                                "process_status_failed",
                                error.to_string(),
                                "Inspect the sandbox state and correlated logs, then run cleanup.",
                            )
                        })? {
                            let (service_id, workload_id) = {
                                let workload = &mut self.state.workloads[process.state_index];
                                workload.phase = SandboxPhase::Failed;
                                workload.process_id = None;
                                (workload.service_id.clone(), workload.workload_id.clone())
                            };
                            self.state.phase = SandboxPhase::Failed;
                            let _ = persist_state(&self.plan.owned_root, &self.state).await;
                            return Err(SandboxError::new(
                                "process_exited",
                                format!(
                                    "Workload {service_id}/{workload_id} exited unexpectedly with {status}."
                                ),
                                "Inspect the correlated logs, fix the Workload, and restart the sandbox.",
                            ));
                        }
                    }
                }
            }
        }
    }

    async fn shutdown(&mut self) -> std::result::Result<(), SandboxError> {
        self.state.phase = SandboxPhase::Stopping;
        let mut failures = Vec::new();
        for process in self.processes.iter_mut().rev() {
            self.state.workloads[process.state_index].phase = SandboxPhase::Stopping;
            if let Err(error) = stop_owned_child(&mut process.child, process.process_group_id).await
            {
                failures.push(error);
                continue;
            }
            self.state.workloads[process.state_index].phase = SandboxPhase::Stopped;
            self.state.workloads[process.state_index].process_id = None;
        }
        if !failures.is_empty() {
            let _ = persist_state(&self.plan.owned_root, &self.state).await;
            return Err(SandboxError::new(
                "cleanup_incomplete",
                format!("Sandbox cleanup was incomplete: {}", failures.join("; ")),
                "Stop the reported process groups, then rerun cleanup.",
            ));
        }
        verify_owner(
            &self.plan.owned_root,
            &self.state.ownership_token,
            &self.state.system_id,
        )?;
        tokio::fs::remove_dir_all(&self.plan.owned_root)
            .await
            .map_err(|error| {
                io_error(
                    "cleanup_incomplete",
                    "remove the owned sandbox root",
                    &error,
                )
            })?;
        self.state.phase = SandboxPhase::Stopped;
        Ok(())
    }
}

async fn stop_owned_child(child: &mut Child, pid: u32) -> std::result::Result<(), String> {
    terminate_group(pid)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        child.try_wait().map_err(|error| error.to_string())?;
        if !process_group_exists(pid)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    force_terminate_group(pid)?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        child.try_wait().map_err(|error| error.to_string())?;
        if !process_group_exists(pid)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "process group {pid} did not stop after SIGTERM and SIGKILL"
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn rollback_launch(running: &mut RunningSandbox, original: SandboxError) -> SandboxError {
    match running.shutdown().await {
        Ok(()) => original,
        Err(cleanup) => SandboxError::new(
            "cleanup_incomplete",
            format!(
                "{} Cleanup also failed: {}",
                original.message, cleanup.message
            ),
            cleanup.next_action,
        ),
    }
}

async fn cleanup_recorded(root: &Path) -> std::result::Result<(), SandboxError> {
    if !root.exists() {
        return Ok(());
    }
    let state: SandboxState = read_typed(&root.join("state.json"), "System Sandbox state")
        .map_err(|error| {
            SandboxError::new(
                "cleanup_state_invalid",
                error.to_string(),
                "Inspect state and remove only resources proven to belong to this sandbox.",
            )
        })?;
    if state.protocol != STATE_PROTOCOL {
        return Err(SandboxError::new(
            "cleanup_state_invalid",
            format!("Unsupported sandbox state protocol {}.", state.protocol),
            "Do not remove resources until the state protocol is recognized.",
        ));
    }
    verify_owner(root, &state.ownership_token, &state.system_id)?;
    let mut failures = Vec::new();
    for workload in state.workloads.iter().rev() {
        if let Some(pid) = workload.process_id
            && let Err(error) = recorded_process_owned(pid, &state.ownership_token)
        {
            failures.push(format!(
                "{}/{}: {error}",
                workload.service_id, workload.workload_id
            ));
        }
    }
    if !failures.is_empty() {
        return Err(SandboxError::new(
            "cleanup_incomplete",
            failures.join("; "),
            "Stop the reported process groups, then rerun cleanup.",
        ));
    }
    for pid in owned_process_ids(&state.ownership_token)? {
        if let Err(error) = terminate_recorded_process(pid).await {
            failures.push(format!("process {pid}: {error}"));
        }
    }
    if !failures.is_empty() {
        return Err(SandboxError::new(
            "cleanup_incomplete",
            failures.join("; "),
            "Stop the reported sandbox-owned processes, then rerun cleanup.",
        ));
    }
    tokio::fs::remove_dir_all(root).await.map_err(|error| {
        io_error(
            "cleanup_incomplete",
            "remove the owned sandbox root",
            &error,
        )
    })
}

fn owned_process_ids(token: &str) -> std::result::Result<Vec<u32>, SandboxError> {
    let output = std::process::Command::new("ps")
        .args(process_listing_args())
        .output()
        .map_err(|error| {
            SandboxError::new(
                "cleanup_incomplete",
                format!("Could not enumerate sandbox processes: {error}"),
                "Inspect the recorded process IDs and retry cleanup.",
            )
        })?;
    if !output.status.success() {
        return Err(SandboxError::new(
            "cleanup_incomplete",
            "Process enumeration failed.",
            "Inspect the recorded process IDs and retry cleanup.",
        ));
    }
    let marker = format!("LENSO_SANDBOX_OWNERSHIP_TOKEN={token}");
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.contains(&marker))
        .filter_map(|line| line.split_whitespace().next()?.parse().ok())
        .collect())
}

#[cfg(target_os = "linux")]
fn process_listing_args() -> &'static [&'static str] {
    &["axeww", "-o", "pid=,command="]
}

#[cfg(not(target_os = "linux"))]
fn process_listing_args() -> &'static [&'static str] {
    &["eww", "-ax", "-o", "pid=,command="]
}

fn recorded_process_owned(pid: u32, token: &str) -> std::result::Result<bool, String> {
    let output = std::process::Command::new("ps")
        .args(["eww", "-p", &pid.to_string(), "-o", "command="])
        .output()
        .map_err(|error| format!("could not inspect process {pid}: {error}"))?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(false);
    }
    let command = String::from_utf8_lossy(&output.stdout);
    if command.contains(&format!("LENSO_SANDBOX_OWNERSHIP_TOKEN={token}")) {
        Ok(true)
    } else {
        Err(format!(
            "refusing to stop process {pid} because its sandbox ownership token does not match"
        ))
    }
}

#[cfg(unix)]
fn terminate_group(pid: u32) -> std::result::Result<(), String> {
    let raw = i32::try_from(pid).map_err(|_| Errno::EINVAL.to_string())?;
    match killpg(Pid::from_raw(raw), Signal::SIGTERM) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(unix))]
fn terminate_group(_pid: u32) -> std::result::Result<(), String> {
    Err("process-group cleanup requires macOS or Linux".to_owned())
}

#[cfg(unix)]
fn force_terminate_group(pid: u32) -> std::result::Result<(), String> {
    let raw = i32::try_from(pid).map_err(|error| error.to_string())?;
    match killpg(Pid::from_raw(raw), Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(unix))]
fn force_terminate_group(_pid: u32) -> std::result::Result<(), String> {
    Err("process-group cleanup requires macOS or Linux".to_owned())
}

#[cfg(unix)]
fn process_group_exists(pid: u32) -> std::result::Result<bool, String> {
    let raw = i32::try_from(pid).map_err(|_| Errno::EINVAL.to_string())?;
    match killpg(Pid::from_raw(raw), None) {
        Ok(()) | Err(Errno::EPERM) => Ok(true),
        Err(Errno::ESRCH) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(not(unix))]
fn process_group_exists(_pid: u32) -> std::result::Result<bool, String> {
    Err("process-group cleanup requires macOS or Linux".to_owned())
}

#[cfg(unix)]
async fn terminate_recorded_process(pid: u32) -> std::result::Result<(), String> {
    let raw = i32::try_from(pid).map_err(|_| Errno::EINVAL.to_string())?;
    let process = Pid::from_raw(raw);
    match kill(process, Signal::SIGTERM) {
        Ok(()) | Err(Errno::ESRCH) => {}
        Err(error) => return Err(error.to_string()),
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match kill(process, None) {
            Err(Errno::ESRCH) => return Ok(()),
            Ok(()) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Ok(()) => {
                kill(process, Signal::SIGKILL).map_err(|error| error.to_string())?;
                return Ok(());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(not(unix))]
async fn terminate_recorded_process(_pid: u32) -> std::result::Result<(), String> {
    Err("recorded cleanup requires macOS or Linux".to_owned())
}

fn verify_owner(
    root: &Path,
    token: &str,
    system_id: &str,
) -> std::result::Result<(), SandboxError> {
    let marker: OwnerMarker = read_typed(&root.join(".owner.json"), "sandbox owner marker")
        .map_err(|error| {
            SandboxError::new(
                "ownership_unproven",
                error.to_string(),
                "Do not delete this directory until ownership is proven.",
            )
        })?;
    if marker.protocol != OWNER_PROTOCOL
        || marker.ownership_token != token
        || marker.system_id != system_id
    {
        return Err(SandboxError::new(
            "ownership_unproven",
            format!("Refusing to remove unowned path: {}", root.display()),
            "Do not delete this directory until marker and state ownership match.",
        ));
    }
    Ok(())
}

async fn persist_state(root: &Path, state: &SandboxState) -> std::result::Result<(), SandboxError> {
    write_json_async(&root.join("state.json"), state).await
}

async fn write_json_async<T: Serialize>(
    path: &Path,
    value: &T,
) -> std::result::Result<(), SandboxError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        SandboxError::new(
            "state_write_failed",
            error.to_string(),
            "Inspect sandbox state and retry cleanup.",
        )
    })?;
    tokio::fs::write(path, bytes)
        .await
        .map_err(|error| io_error("state_write_failed", "write sandbox state", &error))
}

fn io_error(code: &'static str, action: &str, error: &std::io::Error) -> SandboxError {
    SandboxError::new(
        code,
        format!("Could not {action}: {error}"),
        "Fix local filesystem permissions and retry.",
    )
}

async fn rollback_root(root: &Path) {
    let _ = tokio::fs::remove_dir_all(root).await;
}

fn read_typed<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<T> {
    let source =
        fs::read_to_string(path).with_context(|| format!("read {kind} {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("parse {kind} {}", path.display()))
}

fn input_error(code: &'static str, path: &Path, error: &anyhow::Error) -> SandboxError {
    SandboxError::new(
        code,
        format!("Could not read or parse {}: {error}", path.display()),
        "Fix the input path or JSON document and rerun the command.",
    )
}

fn absolute(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn command_error(error: SandboxError, json: bool) -> anyhow::Error {
    if json && let Ok(value) = serde_json::to_string_pretty(&error) {
        println!("{value}");
    }
    anyhow::Error::new(error)
}

fn print_status(system_id: &str, phase: SandboxPhase, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "artifactVersion": STATE_PROTOCOL,
                "systemId": system_id,
                "phase": phase,
            }))?
        );
    } else {
        println!(
            "System Sandbox {system_id}: {}",
            serde_json::to_value(phase)?.as_str().unwrap_or("stopped")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_is_deterministic_and_orders_services_then_workload_roles() {
        let root = test_root("plan");
        fs::create_dir_all(&root).unwrap();
        let system_file = root.join(DEFAULT_SYSTEM_FILE);
        let sandbox_file = root.join(DEFAULT_SANDBOX_FILE);
        let system = system_fixture();
        let definition = sandbox_fixture(false);

        let first = build_plan(&system, &definition, &system_file, &sandbox_file).unwrap();
        let second = build_plan(&system, &definition, &system_file, &sandbox_file).unwrap();

        assert_eq!(first, second);
        assert_eq!(first.protocol, PLAN_PROTOCOL);
        assert_eq!(
            first
                .workloads
                .iter()
                .map(|item| format!("{}/{}", item.service_id, item.workload_id))
                .collect::<Vec<_>>(),
            [
                "support/support-migrate",
                "support/support-api",
                "support/support-worker",
                "notifications/notifications-migrate",
                "notifications/notifications-api",
                "notifications/notifications-worker",
            ]
        );
        assert_eq!(
            first.workloads[0].identity,
            "local-dev://support-platform/support/support-migrate"
        );
        assert!(!first.owned_root.exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn dry_run_validation_rejects_missing_commands_before_mutation() {
        let root = test_root("invalid-command");
        fs::create_dir_all(&root).unwrap();
        let system_file = root.join(DEFAULT_SYSTEM_FILE);
        let sandbox_file = root.join(DEFAULT_SANDBOX_FILE);
        let mut definition = sandbox_fixture(false);
        definition.services[0].workloads[0].command = vec!["missing-lenso-command".to_owned()];

        let error =
            build_plan(&system_fixture(), &definition, &system_file, &sandbox_file).unwrap_err();

        assert_eq!(error.code, "workload_command_missing");
        assert!(!root.join(".lenso").exists());
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn plan_rejects_service_identities_that_escape_the_store_root() {
        let root = test_root("unsafe-service");
        fs::create_dir_all(&root).unwrap();
        let mut system = system_fixture();
        system["autonomousServices"][0]["serviceId"] = Value::String("../../escape".to_owned());

        let error = build_plan(
            &system,
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap_err();

        assert_eq!(error.code, "unsafe_service_identity");
        assert!(!root.join("escape").exists());
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn launch_publishes_state_and_cleanup_removes_only_owned_resources() {
        let root = test_root("launch");
        fs::create_dir_all(&root).unwrap();
        let unrelated = root.join("unrelated.txt");
        fs::write(&unrelated, "keep").unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();

        let mut running = launch(plan.clone(), false).await.unwrap();

        assert_eq!(running.state.phase, SandboxPhase::Ready);
        assert_eq!(running.state.workloads[0].phase, SandboxPhase::Completed);
        assert!(running.state.workloads[0].process_id.is_none());
        assert_eq!(running.state.endpoints.len(), 2);
        assert!(plan.services.iter().all(|item| item.store_path.is_dir()));
        assert!(plan.owned_root.join("state.json").is_file());

        running.shutdown().await.unwrap();

        assert!(!plan.owned_root.exists());
        assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn failed_start_rolls_back_all_owned_state() {
        let root = test_root("rollback");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(true),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();

        let error = launch(plan.clone(), false).await.unwrap_err();

        assert_eq!(error.code, "process_exited");
        assert!(!plan.owned_root.exists());
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn concurrent_root_claim_preserves_the_existing_owner() {
        let root = test_root("concurrent-claim");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        fs::create_dir_all(&plan.owned_root).unwrap();
        let sentinel = plan.owned_root.join("owned-by-another-launch");
        fs::write(&sentinel, "keep").unwrap();

        let error = allocate(&plan, "candidate-owner").await.unwrap_err();

        assert_eq!(error.code, "sandbox_already_exists");
        assert_eq!(fs::read_to_string(sentinel).unwrap(), "keep");
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn cleanup_refuses_a_mismatched_ownership_marker() {
        let root = test_root("ownership");
        fs::create_dir_all(&root).unwrap();
        write_json_async(
            &root.join(".owner.json"),
            &OwnerMarker {
                protocol: OWNER_PROTOCOL.to_owned(),
                ownership_token: "different".to_owned(),
                system_id: "support-platform".to_owned(),
            },
        )
        .await
        .unwrap();
        write_json_async(
            &root.join("state.json"),
            &SandboxState {
                protocol: STATE_PROTOCOL.to_owned(),
                ownership_token: "expected".to_owned(),
                system_id: "support-platform".to_owned(),
                phase: SandboxPhase::Ready,
                workloads: Vec::new(),
                endpoints: Vec::new(),
            },
        )
        .await
        .unwrap();

        let error = cleanup_recorded(&root).await.unwrap_err();

        assert_eq!(error.code, "ownership_unproven");
        assert!(root.exists());
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn cleanup_refuses_a_reused_or_unowned_process_id() {
        let root = test_root("unowned-pid");
        fs::create_dir_all(&root).unwrap();
        let mut unrelated = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = unrelated.id();
        write_json_async(
            &root.join(".owner.json"),
            &OwnerMarker {
                protocol: OWNER_PROTOCOL.to_owned(),
                ownership_token: "expected".to_owned(),
                system_id: "support-platform".to_owned(),
            },
        )
        .await
        .unwrap();
        write_json_async(
            &root.join("state.json"),
            &SandboxState {
                protocol: STATE_PROTOCOL.to_owned(),
                ownership_token: "expected".to_owned(),
                system_id: "support-platform".to_owned(),
                phase: SandboxPhase::Ready,
                workloads: vec![WorkloadState {
                    service_id: "support".to_owned(),
                    workload_id: "support-api".to_owned(),
                    role: WorkloadRole::Api,
                    identity: "local-dev://support-platform/support/support-api".to_owned(),
                    phase: SandboxPhase::Ready,
                    process_id: Some(pid),
                }],
                endpoints: Vec::new(),
            },
        )
        .await
        .unwrap();

        let error = cleanup_recorded(&root).await.unwrap_err();

        assert_eq!(error.code, "cleanup_incomplete");
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn recorded_cleanup_stops_owned_process_groups() {
        let root = test_root("recorded-cleanup");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let running = launch(plan.clone(), false).await.unwrap();

        cleanup_recorded(&plan.owned_root).await.unwrap();

        assert!(!plan.owned_root.exists());
        drop(running);
        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cleanup_kills_descendants_after_the_group_leader_exits() {
        let mut command = Command::new("sh");
        command
            .args([
                "-c",
                "sh -c 'trap \"\" TERM HUP; while :; do sleep 1; done' & exit 0",
            ])
            .kill_on_drop(true);
        command.as_std_mut().process_group(0);
        let mut child = command.spawn().unwrap();
        let process_group_id = child.id().unwrap();
        child.wait().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(process_group_exists(process_group_id).unwrap());

        stop_owned_child(&mut child, process_group_id)
            .await
            .unwrap();

        assert!(!process_group_exists(process_group_id).unwrap());
    }

    fn sandbox_fixture(fail_migration: bool) -> SandboxDefinition {
        let command = |service: &str, role: &str| {
            if fail_migration && service == "support" && role == "migration" {
                vec!["sh".to_owned(), "-c".to_owned(), "exit 9".to_owned()]
            } else if role == "migration" {
                vec!["sh".to_owned(), "-c".to_owned(), "exit 0".to_owned()]
            } else {
                vec!["sh".to_owned(), "-c".to_owned(), "exec sleep 30".to_owned()]
            }
        };
        SandboxDefinition {
            protocol: SANDBOX_PROTOCOL.to_owned(),
            scenarios: Vec::new(),
            services: ["notifications", "support"]
                .into_iter()
                .map(|service| SandboxService {
                    service_id: service.to_owned(),
                    workloads: [
                        ("api", "api"),
                        ("worker", "worker"),
                        ("migrate", "migration"),
                    ]
                    .into_iter()
                    .map(|(suffix, role)| SandboxWorkload {
                        workload_id: format!("{service}-{suffix}"),
                        command: command(service, role),
                        scenario_command: Vec::new(),
                        cwd: None,
                        env: BTreeMap::new(),
                        endpoint: (role == "api").then(|| format!("http://127.0.0.1/{service}")),
                        health_url: None,
                        health_timeout_ms: 1_000,
                    })
                    .collect(),
                })
                .collect(),
        }
    }

    fn system_fixture() -> Value {
        serde_json::json!({
            "protocol": "lenso.system.v2",
            "systemId": "support-platform",
            "host": { "hostId": "support-host", "modules": ["auth"] },
            "providers": [{ "providerId": "mail-provider", "modules": ["mail"] }],
            "autonomousServices": [
                {
                    "serviceId": "notifications",
                    "modules": ["notification"],
                    "workloads": [
                        { "workloadId": "notifications-api", "role": "api" },
                        { "workloadId": "notifications-worker", "role": "worker" },
                        { "workloadId": "notifications-migrate", "role": "migration" }
                    ]
                },
                {
                    "serviceId": "support",
                    "modules": ["support-ticket"],
                    "workloads": [
                        { "workloadId": "support-api", "role": "api" },
                        { "workloadId": "support-worker", "role": "worker" },
                        { "workloadId": "support-migrate", "role": "migration" }
                    ]
                }
            ],
            "contracts": [{
                "contractId": "support-http.v1",
                "version": "v1",
                "producerKind": "autonomous_service",
                "producerId": "support",
                "artifact": { "format": "openapi", "path": "support.v1.yaml" },
                "tenancyMode": "none"
            }],
            "consumers": [{
                "consumerId": "notifications-support",
                "ownerKind": "autonomous_service",
                "ownerId": "notifications",
                "contractId": "support-http.v1",
                "tenancyMode": "none"
            }]
        })
    }

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lenso-system-sandbox-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
