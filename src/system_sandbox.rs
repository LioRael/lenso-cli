#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write as _,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::app_composition::{self, ImplementationBinding};
use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path as AxumPath, State},
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use lenso_service::{
    ContractSemanticKind, SystemV2Graph, WorkloadRole, check_contract_artifact_value,
    system_v2_graph,
};
use lenso_workload_control::workload_control::{
    OperationRecord, WORKLOAD_CONTROL_OBSERVE_PATH, WORKLOAD_CONTROL_OPERATION_PATH,
    WORKLOAD_CONTROL_OPERATIONS_PATH, WORKLOAD_CONTROL_PROTOCOL, WorkloadControlAction,
    WorkloadControlAuthority, WorkloadControlAuthorityDecision, WorkloadControlCapability,
    WorkloadControlError, WorkloadControlErrorCode, WorkloadControlFailure,
    WorkloadMutationRequest, WorkloadObservation, WorkloadObservationRequest,
    WorkloadOperationPhase, WorkloadOperationResult, WorkloadOperationalState, WorkloadProtection,
    WorkloadReference, workload_control_schema_digest,
};
#[cfg(test)]
use lenso_workload_control::workload_control::{WorkloadControlActor, WorkloadControlActorKind};
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
    net::TcpListener,
    process::{Child, Command},
    sync::{Mutex, oneshot},
    task::JoinHandle,
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
const LOCAL_CONTROL_ADAPTER_PROTOCOL: &str = "lenso.local-control-adapter.v1";
const LOCAL_CONTROL_ADAPTER_STATE_SCHEMA: &str = "lenso.local-control-adapter-state.v2";
const LOCAL_CONTROL_ADAPTER_PLAN_PROTOCOL: &str = "lenso.local-control-adapter-plan.v1";
const LOCAL_CONTROL_ADAPTER_DIR: &str = ".lenso/local-control-adapter";
const WORKSPACE_FILE: &str = "lenso.workspace.json";
const WORKLOAD_CONTROL_TOKEN_ENV: &str = "LENSO_WORKLOAD_CONTROL_TOKEN";
const WORKLOAD_CONTROL_CREDENTIAL_FILE: &str = "credential";
const WORKLOAD_CONTROL_REQUEST_LIMIT: usize = 64 * 1024;
const WORKLOAD_CONTROL_SCALAR_MAX_LENGTH: usize = 255;
const WORKLOAD_CONTROL_SAFE_MESSAGE_MAX_LENGTH: usize = 1_024;

#[derive(Debug, Clone)]
pub(crate) struct SystemDevOptions {
    pub(crate) adapter_child: bool,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    composition_digest: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LocalControlAdapterState {
    protocol: String,
    #[serde(default)]
    schema: String,
    #[serde(default)]
    adapter_id: String,
    app_id: String,
    composition_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    endpoint: Option<String>,
    #[serde(default)]
    workload_control_protocol: String,
    #[serde(default)]
    workload_control_schema_digest: String,
    #[serde(default)]
    capabilities: BTreeSet<WorkloadControlCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential_file: Option<PathBuf>,
    adapter_pid: Option<u32>,
    phase: SandboxPhase,
    sandbox_root: PathBuf,
    workload_identities: Vec<String>,
}

#[derive(Debug)]
struct LocalControlRuntime {
    adapter_id: String,
    available: bool,
    workload_states: BTreeMap<WorkloadReference, WorkloadOperationalState>,
    revisions: BTreeMap<WorkloadReference, String>,
    operations: BTreeMap<String, OperationRecord>,
    active_operations: BTreeMap<WorkloadReference, String>,
    idempotency: BTreeMap<(WorkloadReference, String), IdempotencyEntry>,
    operation_delay: Duration,
}

#[derive(Debug, Clone)]
struct IdempotencyEntry {
    request: WorkloadMutationRequest,
    operation_id: String,
}

#[derive(Debug)]
struct PreparedOperation {
    record: OperationRecord,
    execute: bool,
}

impl LocalControlRuntime {
    fn new(running: &RunningSandbox, adapter_id: String, operation_delay: Duration) -> Self {
        let workload_states = running
            .plan
            .workloads
            .iter()
            .enumerate()
            .map(|(index, workload)| {
                let reference = WorkloadReference {
                    system_id: running.plan.system_id.clone(),
                    service_id: workload.service_id.clone(),
                    workload_id: workload.workload_id.clone(),
                };
                (
                    reference,
                    operational_state_for_phase(running.state.workloads[index].phase),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let revisions = workload_states
            .keys()
            .cloned()
            .map(|workload| (workload, new_workload_revision()))
            .collect();
        Self {
            adapter_id,
            available: true,
            workload_states,
            revisions,
            operations: BTreeMap::new(),
            active_operations: BTreeMap::new(),
            idempotency: BTreeMap::new(),
            operation_delay,
        }
    }

    fn observe(
        &self,
        workload: &WorkloadReference,
    ) -> std::result::Result<WorkloadObservation, WorkloadControlHttpError> {
        if is_control_plane_workload(workload, &self.adapter_id) {
            return Ok(WorkloadObservation {
                protocol: WORKLOAD_CONTROL_PROTOCOL.to_owned(),
                workload: workload.clone(),
                state: WorkloadOperationalState::Unknown,
                observed_revision: None,
                capabilities: BTreeSet::new(),
                protection: WorkloadProtection::ControlPlane,
                active_operation: None,
                observed_at_unix_ms: unix_time_ms(),
            });
        }
        let state = self
            .workload_states
            .get(workload)
            .copied()
            .ok_or_else(WorkloadControlHttpError::workload_not_found)?;
        if !self.available {
            return Ok(WorkloadObservation {
                protocol: WORKLOAD_CONTROL_PROTOCOL.to_owned(),
                workload: workload.clone(),
                state: WorkloadOperationalState::Unknown,
                observed_revision: None,
                capabilities: standard_local_capabilities(),
                protection: WorkloadProtection::Controllable,
                active_operation: self.active_operations.get(workload).cloned(),
                observed_at_unix_ms: unix_time_ms(),
            });
        }
        let revision = self
            .revisions
            .get(workload)
            .expect("every planned Workload has a revision")
            .clone();
        Ok(WorkloadObservation {
            protocol: WORKLOAD_CONTROL_PROTOCOL.to_owned(),
            workload: workload.clone(),
            state,
            observed_revision: Some(revision),
            capabilities: standard_local_capabilities(),
            protection: WorkloadProtection::Controllable,
            active_operation: self.active_operations.get(workload).cloned(),
            observed_at_unix_ms: unix_time_ms(),
        })
    }

    fn prepare_operation(
        &mut self,
        request: WorkloadMutationRequest,
    ) -> std::result::Result<PreparedOperation, WorkloadControlHttpError> {
        if request.protocol != WORKLOAD_CONTROL_PROTOCOL {
            return Err(WorkloadControlHttpError::new(
                StatusCode::CONFLICT,
                WorkloadControlErrorCode::IncompatibleProtocol,
                "The Workload Control protocol is not supported by this adapter.",
            ));
        }
        validate_mutation_request(&request)?;
        if is_control_plane_workload(&request.workload, &self.adapter_id) {
            return Err(WorkloadControlHttpError::new(
                StatusCode::FORBIDDEN,
                WorkloadControlErrorCode::ProtectedWorkload,
                "The active Console and Workload Control Adapter cannot be controlled.",
            ));
        }
        if !self.available {
            return Err(WorkloadControlHttpError::authority_unavailable());
        }
        if !self.revisions.contains_key(&request.workload) {
            return Err(WorkloadControlHttpError::workload_not_found());
        }
        if !matches!(
            request.action,
            WorkloadControlAction::Suspend | WorkloadControlAction::Resume
        ) {
            return Err(WorkloadControlHttpError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                WorkloadControlErrorCode::UnsupportedAction,
                "The Local Control Adapter supports only suspend and resume.",
            ));
        }
        let idempotency_scope = (request.workload.clone(), request.idempotency_key.clone());
        if let Some(existing) = self.idempotency.get(&idempotency_scope) {
            if existing.request == request {
                let record = self
                    .operations
                    .get(&existing.operation_id)
                    .expect("idempotency records reference an Operation Record")
                    .clone();
                return Ok(PreparedOperation {
                    record,
                    execute: false,
                });
            }
            return Err(WorkloadControlHttpError::new(
                StatusCode::CONFLICT,
                WorkloadControlErrorCode::IdempotencyConflict,
                "The idempotency key was already used for a different mutation.",
            )
            .with_operation(existing.operation_id.clone()));
        }
        if self.revisions.get(&request.workload) != Some(&request.observed_revision) {
            return Err(WorkloadControlHttpError::new(
                StatusCode::CONFLICT,
                WorkloadControlErrorCode::StaleRevision,
                "The observed revision is no longer current.",
            )
            .with_current_revision(
                self.revisions
                    .get(&request.workload)
                    .expect("known Workloads have a revision")
                    .clone(),
            ));
        }
        if let Some(active_operation) = self.active_operations.get(&request.workload) {
            return Err(WorkloadControlHttpError::new(
                StatusCode::CONFLICT,
                WorkloadControlErrorCode::ActiveMutation,
                "The Workload already has an active mutation.",
            )
            .with_active_operation(active_operation.clone()));
        }
        let operation_id = format!("wop_{}", Uuid::now_v7());
        let now = unix_time_ms();
        let record = OperationRecord {
            protocol: WORKLOAD_CONTROL_PROTOCOL.to_owned(),
            operation_id: operation_id.clone(),
            request: request.clone(),
            authority: WorkloadControlAuthority {
                adapter_id: self.adapter_id.clone(),
                decision: WorkloadControlAuthorityDecision::Accepted,
            },
            phase: WorkloadOperationPhase::Accepted,
            requested_at_unix_ms: now,
            decided_at_unix_ms: now,
            updated_at_unix_ms: now,
            finished_at_unix_ms: None,
            result: None,
            failure: None,
        };
        self.active_operations
            .insert(request.workload.clone(), operation_id.clone());
        self.idempotency.insert(
            idempotency_scope,
            IdempotencyEntry {
                request,
                operation_id: operation_id.clone(),
            },
        );
        self.operations.insert(operation_id, record.clone());
        Ok(PreparedOperation {
            record,
            execute: true,
        })
    }

    fn operation(
        &self,
        operation_id: &str,
    ) -> std::result::Result<OperationRecord, WorkloadControlHttpError> {
        self.operations.get(operation_id).cloned().ok_or_else(|| {
            WorkloadControlHttpError::new(
                StatusCode::NOT_FOUND,
                WorkloadControlErrorCode::OperationNotFound,
                "The Workload operation does not exist.",
            )
        })
    }
}

impl LocalControlRuntime {
    fn advance_revision(
        &mut self,
        workload: &WorkloadReference,
        state: WorkloadOperationalState,
    ) -> WorkloadOperationResult {
        let observed_revision = new_workload_revision();
        self.revisions
            .insert(workload.clone(), observed_revision.clone());
        WorkloadOperationResult {
            state,
            observed_revision,
        }
    }
}

async fn execute_workload_operation(
    runtime: &Arc<Mutex<LocalControlRuntime>>,
    sandbox: &Arc<Mutex<RunningSandbox>>,
    operation_id: &str,
) {
    let (request, prior_state) = {
        let mut metadata = runtime.lock().await;
        let Some(request) = metadata
            .operations
            .get(operation_id)
            .map(|record| record.request.clone())
        else {
            return;
        };
        let prior_state = metadata
            .workload_states
            .get(&request.workload)
            .copied()
            .unwrap_or(WorkloadOperationalState::Unknown);
        if !metadata.available {
            finish_unavailable_operation(&mut metadata, operation_id, &request, prior_state);
            return;
        }
        if let Some(record) = metadata.operations.get_mut(operation_id) {
            let executing_at = unix_time_ms().max(record.updated_at_unix_ms);
            record.phase = WorkloadOperationPhase::Executing;
            record.updated_at_unix_ms = executing_at;
        }
        metadata.workload_states.insert(
            request.workload.clone(),
            WorkloadOperationalState::Transitioning,
        );
        (request, prior_state)
    };
    let (outcome, observed_state) = {
        let mut running = sandbox.lock().await;
        let available = runtime.lock().await.available;
        if !available {
            let mut metadata = runtime.lock().await;
            finish_unavailable_operation(&mut metadata, operation_id, &request, prior_state);
            return;
        }
        let outcome = match request.action {
            WorkloadControlAction::Suspend => {
                suspend_sandbox_workload(&mut running, &request.workload).await
            }
            WorkloadControlAction::Resume => {
                resume_sandbox_workload(&mut running, &request.workload).await
            }
            WorkloadControlAction::Restart | WorkloadControlAction::Scale { .. } => {
                Err(local_control_failure(
                    WorkloadControlErrorCode::UnsupportedAction,
                    "The Local Control Adapter does not support this action.",
                ))
            }
        };
        let observed_state = running
            .workload_index(&request.workload)
            .ok()
            .map(|index| operational_state_for_phase(running.state.workloads[index].phase))
            .unwrap_or(WorkloadOperationalState::Failed);
        (outcome, observed_state)
    };
    let mut metadata = runtime.lock().await;
    let finished_at = unix_time_ms().max(
        metadata
            .operations
            .get(operation_id)
            .map_or(0, |record| record.updated_at_unix_ms),
    );
    let (result, failure) = match outcome {
        Ok(state) => {
            metadata
                .workload_states
                .insert(request.workload.clone(), state);
            (
                Some(metadata.advance_revision(&request.workload, state)),
                None,
            )
        }
        Err(failure) => {
            metadata
                .workload_states
                .insert(request.workload.clone(), observed_state);
            if observed_state == WorkloadOperationalState::Unknown {
                metadata.revisions.remove(&request.workload);
            } else if observed_state != prior_state {
                let _ = metadata.advance_revision(&request.workload, observed_state);
            }
            (None, Some(failure))
        }
    };
    if let Some(record) = metadata.operations.get_mut(operation_id) {
        record.updated_at_unix_ms = finished_at;
        record.finished_at_unix_ms = Some(finished_at);
        record.phase = if result.is_some() {
            WorkloadOperationPhase::Succeeded
        } else {
            WorkloadOperationPhase::Failed
        };
        record.result = result;
        record.failure = failure;
    }
    metadata.active_operations.remove(&request.workload);
}

fn finish_unavailable_operation(
    metadata: &mut LocalControlRuntime,
    operation_id: &str,
    request: &WorkloadMutationRequest,
    prior_state: WorkloadOperationalState,
) {
    metadata
        .workload_states
        .insert(request.workload.clone(), prior_state);
    if let Some(record) = metadata.operations.get_mut(operation_id) {
        let finished_at = unix_time_ms().max(record.updated_at_unix_ms);
        record.phase = WorkloadOperationPhase::Failed;
        record.updated_at_unix_ms = finished_at;
        record.finished_at_unix_ms = Some(finished_at);
        record.result = None;
        record.failure = Some(local_control_failure(
            WorkloadControlErrorCode::AuthorityUnavailable,
            "The Workload Control Adapter stopped before executing the mutation.",
        ));
    }
    metadata.active_operations.remove(&request.workload);
}

async fn suspend_sandbox_workload(
    running: &mut RunningSandbox,
    workload: &WorkloadReference,
) -> std::result::Result<WorkloadOperationalState, WorkloadControlFailure> {
    let state_index = running.workload_index(workload)?;
    if running.state.workloads[state_index].phase == SandboxPhase::Stopped {
        return Ok(WorkloadOperationalState::Suspended);
    }
    let Some(process_index) = running
        .processes
        .iter()
        .position(|process| process.state_index == state_index)
    else {
        running.state.workloads[state_index].phase = SandboxPhase::Failed;
        running.state.workloads[state_index].process_id = None;
        let _ = persist_state(&running.plan.owned_root, &running.state).await;
        return Err(local_control_failure(
            WorkloadControlErrorCode::AuthorityUnavailable,
            "The owned Workload process is unavailable.",
        ));
    };
    let prior_phase = running.state.workloads[state_index].phase;
    running.state.workloads[state_index].phase = SandboxPhase::Stopping;
    if persist_state(&running.plan.owned_root, &running.state)
        .await
        .is_err()
    {
        running.state.workloads[state_index].phase = prior_phase;
        return Err(local_control_failure(
            WorkloadControlErrorCode::AuthorityUnavailable,
            "The Local Control Adapter could not persist the transition.",
        ));
    }
    let mut process = running.processes.remove(process_index);
    if stop_owned_child(&mut process.child, process.process_group_id)
        .await
        .is_err()
    {
        running.state.workloads[state_index].phase = SandboxPhase::Failed;
        running.processes.insert(process_index, process);
        let _ = persist_state(&running.plan.owned_root, &running.state).await;
        return Err(local_control_failure(
            WorkloadControlErrorCode::AuthorityUnavailable,
            "The Local Control Adapter could not stop the owned Workload.",
        ));
    }
    running.state.workloads[state_index].phase = SandboxPhase::Stopped;
    running.state.workloads[state_index].process_id = None;
    running.endpoints_forget(state_index);
    persist_state(&running.plan.owned_root, &running.state)
        .await
        .map_err(|_| {
            local_control_failure(
                WorkloadControlErrorCode::AuthorityUnavailable,
                "The Local Control Adapter could not persist the suspended state.",
            )
        })?;
    Ok(WorkloadOperationalState::Suspended)
}

async fn resume_sandbox_workload(
    running: &mut RunningSandbox,
    workload: &WorkloadReference,
) -> std::result::Result<WorkloadOperationalState, WorkloadControlFailure> {
    let state_index = running.workload_index(workload)?;
    if running.state.workloads[state_index].phase == SandboxPhase::Ready
        && running
            .processes
            .iter()
            .any(|process| process.state_index == state_index)
    {
        return Ok(WorkloadOperationalState::Running);
    }
    running.state.workloads[state_index].phase = SandboxPhase::Starting;
    running.state.workloads[state_index].process_id = None;
    if launch_workload(running, state_index, false).await.is_err() {
        running.state.workloads[state_index].phase = SandboxPhase::Failed;
        running.state.workloads[state_index].process_id = None;
        let _ = persist_state(&running.plan.owned_root, &running.state).await;
        return Err(local_control_failure(
            WorkloadControlErrorCode::AuthorityUnavailable,
            "The Local Control Adapter could not relaunch and verify the Workload.",
        ));
    }
    persist_state(&running.plan.owned_root, &running.state)
        .await
        .map_err(|_| {
            local_control_failure(
                WorkloadControlErrorCode::AuthorityUnavailable,
                "The Local Control Adapter could not persist the resumed state.",
            )
        })?;
    Ok(WorkloadOperationalState::Running)
}

impl RunningSandbox {
    fn workload_index(
        &self,
        workload: &WorkloadReference,
    ) -> std::result::Result<usize, WorkloadControlFailure> {
        self.state
            .workloads
            .iter()
            .position(|candidate| {
                workload.system_id == self.plan.system_id
                    && workload.service_id == candidate.service_id
                    && workload.workload_id == candidate.workload_id
            })
            .ok_or_else(|| {
                local_control_failure(
                    WorkloadControlErrorCode::WorkloadNotFound,
                    "The Workload Reference is not managed by this adapter.",
                )
            })
    }

    fn endpoints_forget(&mut self, state_index: usize) {
        let state = &self.state.workloads[state_index];
        self.state.endpoints.retain(|endpoint| {
            endpoint.service_id != state.service_id || endpoint.workload_id != state.workload_id
        });
    }
}

const fn operational_state_for_phase(phase: SandboxPhase) -> WorkloadOperationalState {
    match phase {
        SandboxPhase::Ready => WorkloadOperationalState::Running,
        SandboxPhase::Stopped => WorkloadOperationalState::Suspended,
        SandboxPhase::Starting | SandboxPhase::Stopping => WorkloadOperationalState::Transitioning,
        SandboxPhase::Failed => WorkloadOperationalState::Failed,
        SandboxPhase::Completed => WorkloadOperationalState::Unknown,
    }
}

fn local_control_failure(
    code: WorkloadControlErrorCode,
    message: impl Into<String>,
) -> WorkloadControlFailure {
    WorkloadControlFailure {
        code,
        message: safe_workload_control_message(message),
    }
}

fn validate_mutation_request(
    request: &WorkloadMutationRequest,
) -> std::result::Result<(), WorkloadControlHttpError> {
    validate_workload_reference(&request.workload)?;
    if [
        request.observed_revision.as_str(),
        request.idempotency_key.as_str(),
        request.actor.subject.as_str(),
    ]
    .into_iter()
    .any(|value| !valid_workload_control_scalar(value))
    {
        return Err(invalid_workload_control_document(
            "The Workload mutation does not match the required protocol.",
        ));
    }
    Ok(())
}

fn validate_workload_reference(
    workload: &WorkloadReference,
) -> std::result::Result<(), WorkloadControlHttpError> {
    if [
        workload.system_id.as_str(),
        workload.service_id.as_str(),
        workload.workload_id.as_str(),
    ]
    .into_iter()
    .any(|value| !valid_workload_control_scalar(value))
    {
        return Err(invalid_workload_control_document(
            "The Workload Reference does not match the required protocol.",
        ));
    }
    Ok(())
}

fn valid_workload_control_scalar(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= WORKLOAD_CONTROL_SCALAR_MAX_LENGTH
}

fn safe_workload_control_message(message: impl Into<String>) -> String {
    let message = message.into();
    if !message.trim().is_empty()
        && message.chars().count() <= WORKLOAD_CONTROL_SAFE_MESSAGE_MAX_LENGTH
    {
        message
    } else {
        "The Workload Control Adapter could not provide a safe error explanation.".to_owned()
    }
}

fn invalid_workload_control_document(message: &'static str) -> WorkloadControlHttpError {
    WorkloadControlHttpError::new(
        StatusCode::BAD_REQUEST,
        WorkloadControlErrorCode::IncompatibleProtocol,
        message,
    )
}

fn is_control_plane_workload(workload: &WorkloadReference, adapter_id: &str) -> bool {
    workload.service_id == "lenso-console"
        || workload.workload_id == "lenso-console"
        || workload.service_id == "lenso-local-control-adapter"
        || workload.workload_id == "lenso-local-control-adapter"
        || workload.service_id == adapter_id
        || workload.workload_id == adapter_id
}

#[derive(Debug)]
struct WorkloadControlHttpError {
    status: StatusCode,
    code: WorkloadControlErrorCode,
    message: String,
    operation_id: Option<String>,
    current_revision: Option<String>,
    active_operation: Option<String>,
}

impl WorkloadControlHttpError {
    fn new(status: StatusCode, code: WorkloadControlErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: safe_workload_control_message(message),
            operation_id: None,
            current_revision: None,
            active_operation: None,
        }
    }

    fn with_operation(mut self, operation_id: String) -> Self {
        self.operation_id = valid_workload_control_scalar(&operation_id).then_some(operation_id);
        self
    }

    fn with_current_revision(mut self, current_revision: String) -> Self {
        self.current_revision =
            valid_workload_control_scalar(&current_revision).then_some(current_revision);
        self
    }

    fn with_active_operation(mut self, active_operation: String) -> Self {
        self.active_operation =
            valid_workload_control_scalar(&active_operation).then_some(active_operation);
        self
    }

    fn authority_unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: WorkloadControlErrorCode::AuthorityUnavailable,
            message: "The bound Workload Control Adapter is unavailable.".to_owned(),
            operation_id: None,
            current_revision: None,
            active_operation: None,
        }
    }

    fn workload_not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            WorkloadControlErrorCode::WorkloadNotFound,
            "The Workload Reference is not managed by this adapter.",
        )
    }

    fn into_response(self) -> Response {
        (
            self.status,
            Json(WorkloadControlError {
                protocol: WORKLOAD_CONTROL_PROTOCOL.to_owned(),
                code: self.code,
                message: self.message,
                operation_id: self.operation_id,
                current_revision: self.current_revision,
                active_operation: self.active_operation,
            }),
        )
            .into_response()
    }
}

#[derive(Clone)]
struct WorkloadControlHttpState {
    runtime: Arc<Mutex<LocalControlRuntime>>,
    sandbox: Arc<Mutex<RunningSandbox>>,
    bearer_token: Arc<str>,
}

#[derive(Debug)]
struct WorkloadControlServer {
    local_addr: SocketAddr,
    runtime: Arc<Mutex<LocalControlRuntime>>,
    sandbox: Arc<Mutex<RunningSandbox>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    server_task: Option<JoinHandle<std::result::Result<(), String>>>,
}

impl WorkloadControlServer {
    fn endpoint(&self) -> String {
        format!("http://{}", self.local_addr)
    }

    #[cfg(test)]
    const fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    async fn wait_for_stop(&self) -> std::result::Result<(), SandboxError> {
        loop {
            tokio::select! {
                signal = tokio::signal::ctrl_c() => {
                    signal.map_err(|error| SandboxError::new(
                        "shutdown_signal_failed",
                        error.to_string(),
                        "Stop the adapter with the cleanup command.",
                    ))?;
                    return Ok(());
                }
                () = tokio::time::sleep(Duration::from_millis(200)) => {
                    if self.server_task.as_ref().is_some_and(JoinHandle::is_finished) {
                        self.runtime.lock().await.available = false;
                        return Err(SandboxError::new(
                            "adapter_http_failed",
                            "The Workload Control HTTP task stopped unexpectedly.",
                            "Restart the Local Control Adapter.",
                        ));
                    }
                    if let Ok(mut sandbox) = self.sandbox.try_lock() {
                        let observation = sandbox.check_for_unexpected_exit().await;
                        drop(sandbox);
                        if let Err(error) = observation {
                            self.runtime.lock().await.available = false;
                            return Err(error);
                        }
                    }
                }
            }
        }
    }

    async fn shutdown(&mut self) -> std::result::Result<(), SandboxError> {
        self.runtime.lock().await.available = false;
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        let http_result = if let Some(server_task) = self.server_task.take() {
            match server_task.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(SandboxError::new(
                    "adapter_http_shutdown_failed",
                    format!("The Workload Control HTTP task failed: {error}"),
                    "Stop the Local Control Adapter and retry cleanup.",
                )),
                Err(error) => Err(SandboxError::new(
                    "adapter_http_shutdown_failed",
                    format!("Could not join the Workload Control HTTP task: {error}"),
                    "Stop the Local Control Adapter and retry cleanup.",
                )),
            }
        } else {
            Ok(())
        };
        let sandbox_result = self.sandbox.lock().await.shutdown().await;
        match (http_result, sandbox_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(http), Err(sandbox)) => Err(SandboxError::new(
                "cleanup_incomplete",
                format!("{} Cleanup also failed: {}", http.message, sandbox.message),
                sandbox.next_action,
            )),
        }
    }
}

async fn start_workload_control_server(
    running: RunningSandbox,
    adapter_id: String,
    bearer_token: String,
    operation_delay: Duration,
) -> std::result::Result<WorkloadControlServer, SandboxError> {
    validate_workload_control_plan(&running.plan, &adapter_id)?;
    if bearer_token.is_empty() {
        return Err(SandboxError::new(
            "adapter_auth_missing",
            "The Local Control Adapter requires a bearer token.",
            "Set LENSO_WORKLOAD_CONTROL_TOKEN in the server-side environment.",
        ));
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|error| {
            SandboxError::new(
                "adapter_http_bind_failed",
                format!("Could not bind the loopback Workload Control endpoint: {error}"),
                "Confirm that a loopback socket is available and retry.",
            )
        })?;
    let local_addr = listener.local_addr().map_err(|error| {
        SandboxError::new(
            "adapter_http_bind_failed",
            format!("Could not inspect the Workload Control endpoint: {error}"),
            "Confirm that a loopback socket is available and retry.",
        )
    })?;
    if !local_addr.ip().is_loopback() {
        return Err(SandboxError::new(
            "adapter_http_bind_failed",
            "The Workload Control endpoint did not bind to loopback.",
            "Bind the Local Control Adapter to loopback only.",
        ));
    }
    let runtime = Arc::new(Mutex::new(LocalControlRuntime::new(
        &running,
        adapter_id,
        operation_delay,
    )));
    let sandbox = Arc::new(Mutex::new(running));
    let router = workload_control_router(WorkloadControlHttpState {
        runtime: Arc::clone(&runtime),
        sandbox: Arc::clone(&sandbox),
        bearer_token: Arc::from(bearer_token),
    });
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server_task = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
            .map_err(|error| error.to_string())
    });
    Ok(WorkloadControlServer {
        local_addr,
        runtime,
        sandbox,
        shutdown_tx: Some(shutdown_tx),
        server_task: Some(server_task),
    })
}

fn workload_control_router(state: WorkloadControlHttpState) -> Router {
    Router::new()
        .route(
            WORKLOAD_CONTROL_OBSERVE_PATH,
            post(observe_workload_handler),
        )
        .route(
            WORKLOAD_CONTROL_OPERATIONS_PATH,
            post(submit_workload_operation_handler),
        )
        .route(
            WORKLOAD_CONTROL_OPERATION_PATH,
            get(get_workload_operation_handler),
        )
        .with_state(state)
}

async fn observe_workload_handler(
    State(state): State<WorkloadControlHttpState>,
    request: Request<Body>,
) -> Response {
    if let Err(error) = require_workload_control_bearer(&request, &state.bearer_token) {
        return error.into_response();
    }
    let request = match parse_workload_control_json::<WorkloadObservationRequest>(request).await {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    if request.protocol != WORKLOAD_CONTROL_PROTOCOL {
        return WorkloadControlHttpError::new(
            StatusCode::CONFLICT,
            WorkloadControlErrorCode::IncompatibleProtocol,
            "The Workload Control protocol is not supported by this adapter.",
        )
        .into_response();
    }
    if let Err(error) = validate_workload_reference(&request.workload) {
        return error.into_response();
    }
    match state.runtime.lock().await.observe(&request.workload) {
        Ok(observation) => Json(observation).into_response(),
        Err(error) => error.into_response(),
    }
}

async fn submit_workload_operation_handler(
    State(state): State<WorkloadControlHttpState>,
    request: Request<Body>,
) -> Response {
    if let Err(error) = require_workload_control_bearer(&request, &state.bearer_token) {
        return error.into_response();
    }
    let request = match parse_workload_mutation_request(request).await {
        Ok(request) => request,
        Err(error) => return error.into_response(),
    };
    let (prepared, delay) = {
        let mut runtime = state.runtime.lock().await;
        let delay = runtime.operation_delay;
        match runtime.prepare_operation(request) {
            Ok(prepared) => (prepared, delay),
            Err(error) => return error.into_response(),
        }
    };
    if prepared.execute {
        let runtime = Arc::clone(&state.runtime);
        let sandbox = Arc::clone(&state.sandbox);
        let operation_id = prepared.record.operation_id.clone();
        tokio::spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            } else {
                tokio::task::yield_now().await;
            }
            execute_workload_operation(&runtime, &sandbox, &operation_id).await;
        });
    }
    (StatusCode::ACCEPTED, Json(prepared.record)).into_response()
}

async fn get_workload_operation_handler(
    State(state): State<WorkloadControlHttpState>,
    AxumPath(operation_id): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = require_workload_control_bearer_headers(&headers, &state.bearer_token) {
        return error.into_response();
    }
    match state.runtime.lock().await.operation(&operation_id) {
        Ok(record) => Json(record).into_response(),
        Err(error) => error.into_response(),
    }
}

fn require_workload_control_bearer(
    request: &Request<Body>,
    expected: &str,
) -> std::result::Result<(), WorkloadControlHttpError> {
    require_workload_control_bearer_headers(request.headers(), expected)
}

fn require_workload_control_bearer_headers(
    headers: &HeaderMap,
    expected: &str,
) -> std::result::Result<(), WorkloadControlHttpError> {
    let authorized = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == expected);
    if authorized {
        Ok(())
    } else {
        Err(WorkloadControlHttpError::new(
            StatusCode::UNAUTHORIZED,
            WorkloadControlErrorCode::Unauthenticated,
            "A valid server-side bearer token is required.",
        ))
    }
}

async fn parse_workload_control_json<T: serde::de::DeserializeOwned>(
    request: Request<Body>,
) -> std::result::Result<T, WorkloadControlHttpError> {
    let bytes = read_workload_control_body(request).await?;
    serde_json::from_slice(&bytes).map_err(|_| {
        WorkloadControlHttpError::new(
            StatusCode::BAD_REQUEST,
            WorkloadControlErrorCode::IncompatibleProtocol,
            "The Workload Control request body does not match the required schema.",
        )
    })
}

async fn parse_workload_mutation_request(
    request: Request<Body>,
) -> std::result::Result<WorkloadMutationRequest, WorkloadControlHttpError> {
    let bytes = read_workload_control_body(request).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| {
        WorkloadControlHttpError::new(
            StatusCode::BAD_REQUEST,
            WorkloadControlErrorCode::IncompatibleProtocol,
            "The Workload Control request body does not match the required schema.",
        )
    })?;
    validate_workload_control_action_shape(&value)?;
    if value.pointer("/action/kind").and_then(Value::as_str) == Some("scale")
        && value
            .pointer("/action/targetCapacity")
            .and_then(Value::as_u64)
            == Some(0)
    {
        return Err(WorkloadControlHttpError::new(
            StatusCode::BAD_REQUEST,
            WorkloadControlErrorCode::InvalidCapacity,
            "Scale target capacity must be greater than zero.",
        ));
    }
    serde_json::from_value(value).map_err(|_| {
        WorkloadControlHttpError::new(
            StatusCode::BAD_REQUEST,
            WorkloadControlErrorCode::IncompatibleProtocol,
            "The Workload Control request body does not match the required schema.",
        )
    })
}

fn validate_workload_control_action_shape(
    document: &Value,
) -> std::result::Result<(), WorkloadControlHttpError> {
    let Some(action) = document.get("action").and_then(Value::as_object) else {
        return Ok(());
    };
    let Some(kind) = action.get("kind").and_then(Value::as_str) else {
        return Ok(());
    };
    let exact_fields = match kind {
        "suspend" | "resume" | "restart" => action.len() == 1,
        "scale" => action.len() == 2 && action.contains_key("targetCapacity"),
        _ => true,
    };
    if exact_fields {
        Ok(())
    } else {
        Err(invalid_workload_control_document(
            "The Workload Control action contains fields outside its required schema.",
        ))
    }
}

async fn read_workload_control_body(
    request: Request<Body>,
) -> std::result::Result<axum::body::Bytes, WorkloadControlHttpError> {
    to_bytes(request.into_body(), WORKLOAD_CONTROL_REQUEST_LIMIT)
        .await
        .map_err(|_| {
            WorkloadControlHttpError::new(
                StatusCode::BAD_REQUEST,
                WorkloadControlErrorCode::IncompatibleProtocol,
                "The Workload Control request body is invalid.",
            )
        })
}

fn standard_local_capabilities() -> BTreeSet<WorkloadControlCapability> {
    BTreeSet::from([
        WorkloadControlCapability::Suspend,
        WorkloadControlCapability::Resume,
    ])
}

fn new_workload_revision() -> String {
    format!("wrev_{}", Uuid::now_v7())
}

fn unix_time_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
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
    validate_options(&options).map_err(|error| command_error(error, options.json))?;
    let explicit_system_file = options.system_file.is_some();
    let system_file = absolute(
        &current,
        options
            .system_file
            .as_deref()
            .unwrap_or(Path::new(DEFAULT_SYSTEM_FILE)),
    );
    if let Some(composition_file) = exact_composition_path(
        &current,
        options.system_file.as_deref(),
        explicit_system_file,
    ) {
        return dev_composition(options, composition_file).await;
    }
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

fn exact_composition_path(
    current: &Path,
    explicit_path: Option<&Path>,
    was_explicit: bool,
) -> Option<PathBuf> {
    let candidate = explicit_path.map(|path| absolute(current, path));
    if was_explicit {
        return candidate.filter(|path| {
            path.file_name().and_then(|name| name.to_str())
                == Some(app_composition::APP_COMPOSITION_FILE)
        });
    }
    let default = current.join(app_composition::APP_COMPOSITION_FILE);
    default.exists().then_some(default)
}

async fn dev_composition(options: SystemDevOptions, composition_file: PathBuf) -> Result<()> {
    if options.sandbox_file.is_some() {
        return Err(command_error(
            SandboxError::new(
                "conflicting_options",
                "An exact App Composition owns its local runtime plan; --sandbox-file is not used.",
                "Remove --sandbox-file and run lenso system dev from the App directory.",
            ),
            options.json,
        ));
    }
    if options.scenario.is_some() {
        return Err(command_error(
            SandboxError::new(
                "unsupported_option",
                "Failure Scenarios are only available for an explicit System Sandbox definition.",
                "Run lenso system dev with lenso.system.json and lenso.system-sandbox.json for scenarios.",
            ),
            options.json,
        ));
    }
    let plan = build_composition_plan(&composition_file, options.cleanup)
        .map_err(|error| command_error(error, options.json))?;
    if options.adapter_child {
        return run_composition_adapter_child(plan).await;
    }
    let adapter_state_path = adapter_state_path(&plan);

    if options.cleanup {
        if adapter_state_path.exists() {
            let adapter_state: LocalControlAdapterState =
                read_typed(&adapter_state_path, "Local Control Adapter state").map_err(
                    |error| {
                        command_error(
                            input_error("adapter_state_invalid", &adapter_state_path, &error),
                            options.json,
                        )
                    },
                )?;
            stop_local_adapter(&adapter_state, &plan)
                .await
                .map_err(|error| command_error(error, options.json))?;
        }
        cleanup_recorded(&plan.owned_root)
            .await
            .map_err(|error| command_error(error, options.json))?;
        let state = adapter_state_for(&plan, None, SandboxPhase::Stopped);
        write_adapter_state(&adapter_state_path, &state)
            .map_err(|error| command_error(error, options.json))?;
        print_adapter_state(&state, options.json)?;
        return Ok(());
    }
    if options.dry_run {
        println!("{}", serde_json::to_string_pretty(&plan)?);
        return Ok(());
    }
    validate_workload_control_plan(&plan, &adapter_id_for(&plan))
        .map_err(|error| command_error(error, options.json))?;

    let starting = adapter_state_for(&plan, None, SandboxPhase::Starting);
    write_adapter_state(&adapter_state_path, &starting)
        .map_err(|error| command_error(error, options.json))?;
    let mut child = start_adapter_child(&composition_file)?;
    let adapter_state =
        read_typed::<LocalControlAdapterState>(&adapter_state_path, "Local Control Adapter state")
            .ok();
    if !adapter_state.is_some_and(|state| {
        matches!(
            state.phase,
            SandboxPhase::Ready | SandboxPhase::Failed | SandboxPhase::Stopped
        )
    }) {
        let starting = adapter_state_for(&plan, Some(child.id()), SandboxPhase::Starting);
        write_adapter_state(&adapter_state_path, &starting)
            .map_err(|error| command_error(error, options.json))?;
    }
    let state = wait_for_adapter_ready(&mut child, &adapter_state_path).await?;
    print_adapter_state(&state, options.json)?;
    Ok(())
}

fn build_composition_plan(
    composition_file: &Path,
    allow_existing: bool,
) -> std::result::Result<SandboxPlan, SandboxError> {
    let composition = app_composition::read(composition_file)
        .map_err(|error| input_error("app_composition_invalid", composition_file, &error))?;
    if !is_safe_identity(&composition.app_id) {
        return Err(SandboxError::new(
            "unsafe_app_identity",
            format!(
                "App identity is not a safe path component: {}",
                composition.app_id
            ),
            "Use only ASCII letters, numbers, dot, underscore, and dash in the App id.",
        ));
    }
    let app_dir = composition_file.parent().unwrap_or(Path::new("."));
    let workspace_file = app_dir.join(WORKSPACE_FILE);
    let workspace: Value = read_typed(&workspace_file, "Service workspace")
        .map_err(|error| input_error("workspace_invalid", &workspace_file, &error))?;
    if workspace.get("protocol").and_then(Value::as_str) != Some("lenso.service-workspace.v1") {
        return Err(SandboxError::new(
            "workspace_invalid",
            format!(
                "Unsupported Service workspace protocol in {}.",
                workspace_file.display()
            ),
            "Run lenso service workspace init or regenerate the App services.",
        ));
    }
    let workspace_services = workspace
        .get("services")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid_definition("Service workspace services must be an array."))?;
    let mut requested_services = BTreeMap::new();
    for module in &composition.modules {
        if let ImplementationBinding::Service { service_reference } = &module.implementation {
            let service_name = service_name_from_reference(service_reference).ok_or_else(|| {
                SandboxError::new(
                    "invalid_service_reference",
                    format!(
                        "Could not map Service Reference {service_reference} to a local service."
                    ),
                    "Use a stable service:provider/name reference backed by lenso.workspace.json.",
                )
            })?;
            requested_services.insert(service_name, service_reference.clone());
        }
    }
    let system_dir = composition_file.parent().unwrap_or(Path::new("."));
    let owned_root = system_dir
        .join(".lenso/system-sandbox")
        .join(&composition.app_id);
    validate_owned_root(&owned_root, system_dir)?;
    if requested_services.is_empty() {
        if allow_existing {
            return Ok(SandboxPlan {
                protocol: LOCAL_CONTROL_ADAPTER_PLAN_PROTOCOL.to_owned(),
                composition_digest: Some(composition.content_digest),
                system_id: composition.app_id,
                system_file: composition_file.to_path_buf(),
                sandbox_file: composition_file.to_path_buf(),
                owned_root,
                services: Vec::new(),
                workloads: Vec::new(),
            });
        }
        return Err(SandboxError::new(
            "no_local_workloads",
            "The App Composition has no service-backed implementation to run locally.",
            "Select at least one service-backed Module implementation before running lenso system dev.",
        ));
    }

    if owned_root.exists() && !allow_existing {
        return Err(SandboxError::new(
            "sandbox_already_exists",
            format!(
                "Local Control Adapter state already exists: {}",
                owned_root.display()
            ),
            "Run lenso system dev --cleanup before starting another local adapter.",
        ));
    }

    let mut services = Vec::new();
    let mut workloads = Vec::new();
    for (service_name, service_reference) in requested_services {
        let service = workspace_services
            .iter()
            .find(|service| {
                service.get("name").and_then(Value::as_str) == Some(service_name.as_str())
            })
            .ok_or_else(|| {
                SandboxError::new(
                    "workspace_service_missing",
                    format!(
                        "Service Reference {service_reference} has no {service_name} entry in {}.",
                        workspace_file.display()
                    ),
                    "Materialize the referenced service into lenso.workspace.json before running the App.",
                )
            })?;
        let command = service
            .get("command")
            .and_then(Value::as_str)
            .filter(|command| !command.trim().is_empty())
            .map(|command| {
                command
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .ok_or_else(|| {
                invalid_definition(format!(
                    "Workspace service {service_name} must declare a command."
                ))
            })?;
        let cwd = service
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|cwd| !cwd.is_empty())
            .map(|cwd| absolute(system_dir, Path::new(cwd)))
            .unwrap_or_else(|| system_dir.to_path_buf());
        if !cwd.is_dir() {
            return Err(SandboxError::new(
                "workload_cwd_missing",
                format!(
                    "Workspace service {service_name} cwd does not exist: {}",
                    cwd.display()
                ),
                "Create the service directory or correct its workspace cwd.",
            ));
        }
        if command.is_empty() || !command_exists(&command[0], &cwd, &BTreeMap::new()) {
            return Err(SandboxError::new(
                "workload_command_missing",
                format!(
                    "Workspace service {service_name} executable was not found: {}",
                    command.first().cloned().unwrap_or_default()
                ),
                "Install the service command or correct its workspace command.",
            ));
        }
        let health_url = service
            .get("readyUrl")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        if let Some(url) = &health_url {
            let parsed = reqwest::Url::parse(url).map_err(|error| {
                SandboxError::new(
                    "invalid_workload_url",
                    format!("Workspace service {service_name} readyUrl is invalid: {error}"),
                    "Set readyUrl to an absolute HTTP URL.",
                )
            })?;
            if !matches!(parsed.scheme(), "http" | "https") {
                return Err(SandboxError::new(
                    "invalid_workload_url",
                    format!("Workspace service {service_name} readyUrl must use HTTP."),
                    "Set readyUrl to an absolute HTTP URL.",
                ));
            }
        }
        let workload_id = format!("{service_name}-api");
        let identity = format!("local-dev://{}/{service_name}/api", composition.app_id);
        let store_path = owned_root
            .join("services")
            .join(&service_name)
            .join("store");
        let mut env = BTreeMap::new();
        env.insert("LENSO_APP_ID".to_owned(), composition.app_id.clone());
        env.insert(
            "LENSO_APP_COMPOSITION_DIGEST".to_owned(),
            composition.content_digest.clone(),
        );
        env.insert("LENSO_SYSTEM_ID".to_owned(), composition.app_id.clone());
        env.insert("LENSO_SERVICE_ID".to_owned(), service_name.clone());
        env.insert("LENSO_WORKLOAD_ID".to_owned(), workload_id.clone());
        env.insert("LENSO_WORKLOAD_IDENTITY".to_owned(), identity.clone());
        env.insert("LENSO_SERVICE_REFERENCE".to_owned(), service_reference);
        env.insert(
            "LENSO_SERVICE_STORE_PATH".to_owned(),
            store_path.display().to_string(),
        );
        services.push(PlannedService {
            service_id: service_name.clone(),
            store_path: store_path.clone(),
        });
        workloads.push(PlannedWorkload {
            service_id: service_name,
            workload_id,
            role: WorkloadRole::Api,
            identity,
            command,
            scenario_command: Vec::new(),
            cwd,
            env,
            endpoint: None,
            health_url,
            health_timeout_ms: service
                .get("readyTimeoutMs")
                .and_then(Value::as_u64)
                .unwrap_or(default_health_timeout_ms()),
        });
    }

    Ok(SandboxPlan {
        protocol: LOCAL_CONTROL_ADAPTER_PLAN_PROTOCOL.to_owned(),
        composition_digest: Some(composition.content_digest),
        system_id: composition.app_id,
        system_file: composition_file.to_path_buf(),
        sandbox_file: composition_file.to_path_buf(),
        owned_root,
        services,
        workloads,
    })
}

fn service_name_from_reference(reference: &str) -> Option<String> {
    reference
        .strip_prefix("service:")?
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

fn adapter_state_path(plan: &SandboxPlan) -> PathBuf {
    plan.system_file
        .parent()
        .unwrap_or(Path::new("."))
        .join(LOCAL_CONTROL_ADAPTER_DIR)
        .join(&plan.system_id)
        .join("state.json")
}

fn adapter_credential_path(plan: &SandboxPlan) -> PathBuf {
    adapter_state_path(plan).with_file_name(WORKLOAD_CONTROL_CREDENTIAL_FILE)
}

fn adapter_id_for(plan: &SandboxPlan) -> String {
    format!("workload-control:{}", plan.system_id)
}

fn validate_workload_control_plan(
    plan: &SandboxPlan,
    adapter_id: &str,
) -> std::result::Result<(), SandboxError> {
    let valid_workloads = plan.workloads.iter().all(|workload| {
        [
            plan.system_id.as_str(),
            workload.service_id.as_str(),
            workload.workload_id.as_str(),
        ]
        .into_iter()
        .all(valid_workload_control_scalar)
    });
    if !valid_workload_control_scalar(adapter_id) || !valid_workloads {
        return Err(SandboxError::new(
            "workload_control_identity_invalid",
            "The local plan contains an identity that Workload Control v1 cannot represent.",
            "Use non-blank System, Service, Workload, and Adapter identities of at most 255 characters.",
        ));
    }
    Ok(())
}

fn adapter_state_for(
    plan: &SandboxPlan,
    adapter_pid: Option<u32>,
    phase: SandboxPhase,
) -> LocalControlAdapterState {
    adapter_state_with_control(
        plan,
        adapter_pid,
        phase,
        None,
        workload_control_credential_reference(plan),
    )
}

fn adapter_state_with_control(
    plan: &SandboxPlan,
    adapter_pid: Option<u32>,
    phase: SandboxPhase,
    endpoint: Option<String>,
    credential_file: Option<PathBuf>,
) -> LocalControlAdapterState {
    LocalControlAdapterState {
        protocol: LOCAL_CONTROL_ADAPTER_PROTOCOL.to_owned(),
        schema: LOCAL_CONTROL_ADAPTER_STATE_SCHEMA.to_owned(),
        adapter_id: adapter_id_for(plan),
        app_id: plan.system_id.clone(),
        composition_digest: plan.composition_digest.clone().unwrap_or_default(),
        endpoint,
        workload_control_protocol: WORKLOAD_CONTROL_PROTOCOL.to_owned(),
        workload_control_schema_digest: workload_control_schema_digest(),
        capabilities: standard_local_capabilities(),
        credential_file,
        adapter_pid,
        phase,
        sandbox_root: plan.owned_root.clone(),
        workload_identities: plan
            .workloads
            .iter()
            .map(|workload| workload.identity.clone())
            .collect(),
    }
}

fn workload_control_credential_reference(plan: &SandboxPlan) -> Option<PathBuf> {
    std::env::var(WORKLOAD_CONTROL_TOKEN_ENV)
        .ok()
        .filter(|token| !token.is_empty())
        .is_none()
        .then(|| adapter_credential_path(plan))
}

struct WorkloadControlCredential {
    token: String,
    file: Option<PathBuf>,
}

fn resolve_workload_control_credential(
    plan: &SandboxPlan,
    token_override: Option<String>,
) -> std::result::Result<WorkloadControlCredential, SandboxError> {
    if let Some(token) = token_override.filter(|token| !token.is_empty()) {
        return Ok(WorkloadControlCredential { token, file: None });
    }
    let path = adapter_credential_path(plan);
    if path.exists() {
        ensure_owner_only_credential(&path)?;
        let token = fs::read_to_string(&path).map_err(|error| {
            io_error(
                "adapter_credential_invalid",
                "read the Workload Control credential",
                &error,
            )
        })?;
        if token.len() < 32 || token.trim() != token {
            return Err(SandboxError::new(
                "adapter_credential_invalid",
                "The Workload Control credential file is invalid.",
                "Remove only the local adapter credential file and restart the adapter.",
            ));
        }
        return Ok(WorkloadControlCredential {
            token,
            file: Some(path),
        });
    }
    let parent = path.parent().ok_or_else(|| {
        SandboxError::new(
            "adapter_credential_write_failed",
            "The Workload Control credential file has no parent directory.",
            "Run the adapter from a writable App directory.",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        io_error(
            "adapter_credential_write_failed",
            "create the Workload Control credential directory",
            &error,
        )
    })?;
    let mut random = [0_u8; 32];
    getrandom::fill(&mut random).map_err(|_| {
        SandboxError::new(
            "adapter_credential_write_failed",
            "The operating system could not generate a Workload Control credential.",
            "Restore the operating system random source and restart the adapter.",
        )
    })?;
    let token = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&path).map_err(|error| {
        io_error(
            "adapter_credential_write_failed",
            "create the Workload Control credential",
            &error,
        )
    })?;
    file.write_all(token.as_bytes()).map_err(|error| {
        io_error(
            "adapter_credential_write_failed",
            "write the Workload Control credential",
            &error,
        )
    })?;
    file.sync_all().map_err(|error| {
        io_error(
            "adapter_credential_write_failed",
            "flush the Workload Control credential",
            &error,
        )
    })?;
    ensure_owner_only_credential(&path)?;
    Ok(WorkloadControlCredential {
        token,
        file: Some(path),
    })
}

fn ensure_owner_only_credential(path: &Path) -> std::result::Result<(), SandboxError> {
    #[cfg(unix)]
    {
        let mode = fs::metadata(path)
            .map_err(|error| {
                io_error(
                    "adapter_credential_invalid",
                    "inspect the Workload Control credential",
                    &error,
                )
            })?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            return Err(SandboxError::new(
                "adapter_credential_invalid",
                "The Workload Control credential file is accessible by another user.",
                "Restrict the credential file to owner read/write access and retry.",
            ));
        }
    }
    Ok(())
}

fn write_adapter_state(
    path: &Path,
    state: &LocalControlAdapterState,
) -> std::result::Result<(), SandboxError> {
    let parent = path.parent().ok_or_else(|| {
        SandboxError::new(
            "adapter_state_write_failed",
            "Local Control Adapter state has no parent directory.",
            "Run the command from a writable App directory.",
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| {
        io_error(
            "adapter_state_write_failed",
            "create adapter state directory",
            &error,
        )
    })?;
    let source = serde_json::to_vec_pretty(state).map_err(|error| {
        SandboxError::new(
            "adapter_state_write_failed",
            error.to_string(),
            "Inspect the local adapter state and retry.",
        )
    })?;
    let temporary = path.with_file_name(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state.json"),
        std::process::id()
    ));
    let result = (|| -> std::result::Result<(), SandboxError> {
        fs::write(&temporary, source).map_err(|error| {
            io_error("adapter_state_write_failed", "write adapter state", &error)
        })?;
        fs::rename(&temporary, path).map_err(|error| {
            io_error(
                "adapter_state_write_failed",
                "atomically replace adapter state",
                &error,
            )
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn start_adapter_child(composition_file: &Path) -> Result<std::process::Child> {
    let executable = std::env::current_exe().context("resolve lenso executable")?;
    let app_dir = composition_file.parent().unwrap_or(Path::new("."));
    std::process::Command::new(executable)
        .args(["system", "dev", "--adapter-child", "--system-file"])
        .arg(composition_file)
        .current_dir(app_dir)
        .env("LENSO_LOCAL_CONTROL_ADAPTER", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("start Local Control Adapter")
}

async fn stop_local_adapter(
    state: &LocalControlAdapterState,
    plan: &SandboxPlan,
) -> std::result::Result<(), SandboxError> {
    if state.protocol != LOCAL_CONTROL_ADAPTER_PROTOCOL
        || state.app_id != plan.system_id
        || state.sandbox_root != plan.owned_root
    {
        return Err(SandboxError::new(
            "adapter_state_invalid",
            "Local Control Adapter state does not belong to this App Composition.",
            "Inspect the adapter state and remove only state proven to belong to this App.",
        ));
    }
    let Some(pid) = state.adapter_pid else {
        return Ok(());
    };
    if matches!(state.phase, SandboxPhase::Stopped | SandboxPhase::Failed) {
        return Ok(());
    }
    let output = std::process::Command::new("ps")
        .args(["eww", "-p", &pid.to_string(), "-o", "command="])
        .output()
        .map_err(|error| {
            SandboxError::new(
                "adapter_cleanup_incomplete",
                format!("Could not inspect Local Control Adapter process {pid}: {error}"),
                "Inspect the adapter process and rerun cleanup.",
            )
        })?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(());
    }
    let command = String::from_utf8_lossy(&output.stdout);
    if !command.contains("LENSO_LOCAL_CONTROL_ADAPTER=1") {
        return Err(SandboxError::new(
            "adapter_ownership_unproven",
            format!(
                "Refusing to stop process {pid} because its Local Control Adapter marker does not match."
            ),
            "Inspect the recorded adapter PID and remove only state proven to belong to this App.",
        ));
    }
    terminate_recorded_process(pid).await.map_err(|error| {
        SandboxError::new(
            "adapter_cleanup_incomplete",
            format!("Could not stop Local Control Adapter process {pid}: {error}"),
            "Stop the recorded adapter process, then rerun cleanup.",
        )
    })
}

async fn wait_for_adapter_ready(
    child: &mut std::process::Child,
    state_path: &Path,
) -> Result<LocalControlAdapterState> {
    for _ in 0..300 {
        if state_path.exists() {
            let state: LocalControlAdapterState =
                read_typed(state_path, "Local Control Adapter state")?;
            match state.phase {
                SandboxPhase::Ready => return Ok(state),
                SandboxPhase::Failed | SandboxPhase::Stopped => {
                    bail!("Local Control Adapter stopped during startup")
                }
                SandboxPhase::Starting | SandboxPhase::Completed | SandboxPhase::Stopping => {}
            }
        }
        if let Some(status) = child.try_wait().context("observe Local Control Adapter")? {
            bail!("Local Control Adapter exited during startup with {status}")
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("Local Control Adapter did not become ready within 30 seconds")
}

async fn run_composition_adapter_child(plan: SandboxPlan) -> Result<()> {
    let state_path = adapter_state_path(&plan);
    let pid = std::process::id();
    validate_workload_control_plan(&plan, &adapter_id_for(&plan))
        .map_err(|error| command_error(error, false))?;
    let credential =
        resolve_workload_control_credential(&plan, std::env::var(WORKLOAD_CONTROL_TOKEN_ENV).ok())
            .map_err(|error| command_error(error, false))?;
    write_adapter_state(
        &state_path,
        &adapter_state_with_control(
            &plan,
            Some(pid),
            SandboxPhase::Starting,
            None,
            credential.file.clone(),
        ),
    )
    .map_err(|error| command_error(error, false))?;
    let running = match launch(plan.clone(), false).await {
        Ok(running) => running,
        Err(error) => {
            let _ = write_adapter_state(
                &state_path,
                &adapter_state_with_control(
                    &plan,
                    Some(pid),
                    SandboxPhase::Failed,
                    None,
                    credential.file,
                ),
            );
            return Err(command_error(error, false));
        }
    };
    let mut server = match start_workload_control_server(
        running,
        adapter_id_for(&plan),
        credential.token,
        Duration::ZERO,
    )
    .await
    {
        Ok(server) => server,
        Err(error) => {
            let _ = cleanup_recorded(&plan.owned_root).await;
            let _ = write_adapter_state(
                &state_path,
                &adapter_state_with_control(
                    &plan,
                    Some(pid),
                    SandboxPhase::Failed,
                    None,
                    credential.file,
                ),
            );
            return Err(command_error(error, false));
        }
    };
    let endpoint = server.endpoint();
    if let Err(original) = write_adapter_state(
        &state_path,
        &adapter_state_with_control(
            &plan,
            Some(pid),
            SandboxPhase::Ready,
            Some(endpoint.clone()),
            credential.file.clone(),
        ),
    ) {
        let error = match server.shutdown().await {
            Ok(()) => original,
            Err(cleanup) => SandboxError::new(
                "cleanup_incomplete",
                format!(
                    "{} Cleanup also failed: {}",
                    original.message, cleanup.message
                ),
                cleanup.next_action,
            ),
        };
        let _ = write_adapter_state(
            &state_path,
            &adapter_state_with_control(
                &plan,
                Some(pid),
                SandboxPhase::Failed,
                None,
                credential.file,
            ),
        );
        return Err(command_error(error, false));
    }
    if let Err(original) = server.wait_for_stop().await {
        let error = match server.shutdown().await {
            Ok(()) => original,
            Err(cleanup) => SandboxError::new(
                "cleanup_incomplete",
                format!(
                    "{} Cleanup also failed: {}",
                    original.message, cleanup.message
                ),
                cleanup.next_action,
            ),
        };
        let _ = write_adapter_state(
            &state_path,
            &adapter_state_with_control(
                &plan,
                Some(pid),
                SandboxPhase::Failed,
                Some(endpoint),
                credential.file,
            ),
        );
        return Err(command_error(error, false));
    }
    match server.shutdown().await {
        Ok(()) => {
            write_adapter_state(
                &state_path,
                &adapter_state_with_control(
                    &plan,
                    Some(pid),
                    SandboxPhase::Stopped,
                    None,
                    credential.file,
                ),
            )
            .map_err(|error| command_error(error, false))?;
            Ok(())
        }
        Err(error) => {
            let _ = write_adapter_state(
                &state_path,
                &adapter_state_with_control(
                    &plan,
                    Some(pid),
                    SandboxPhase::Failed,
                    Some(endpoint),
                    credential.file,
                ),
            );
            Err(command_error(error, false))
        }
    }
}

fn print_adapter_state(state: &LocalControlAdapterState, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(state)?);
        return Ok(());
    }
    let phase_value = serde_json::to_value(state.phase)?;
    let phase = phase_value.as_str().unwrap_or("unknown");
    println!("Local Control Adapter {}: {phase}", state.app_id);
    println!("adapter id: {}", state.adapter_id);
    println!("composition digest: {}", state.composition_digest);
    println!(
        "workload control: {} ({})",
        state.workload_control_protocol, state.workload_control_schema_digest
    );
    if let Some(endpoint) = &state.endpoint {
        println!("endpoint: {endpoint}");
    }
    if let Some(credential_file) = &state.credential_file {
        println!("credential file: {}", credential_file.display());
    }
    for identity in &state.workload_identities {
        println!("workload identity: {identity}");
    }
    println!("Next: lenso system dev --cleanup");
    Ok(())
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
        composition_digest: None,
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
        let service = remaining
            .iter()
            .find(|(_, dependencies)| dependencies.iter().all(|item| ordered.contains(item)))
            .map(|(service, _)| service.clone())
            // Service APIs do not make business calls during Sandbox startup. A contract
            // cycle therefore needs a stable bootstrap order, not a false deployment
            // rejection; established calls remain governed by readiness and Call Policy.
            .or_else(|| remaining.keys().next().cloned())
            .expect("remaining Services are not empty");
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
        .env_remove(WORKLOAD_CONTROL_TOKEN_ENV)
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
                    self.check_for_unexpected_exit().await?;
                }
            }
        }
    }

    async fn check_for_unexpected_exit(&mut self) -> std::result::Result<(), SandboxError> {
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
        Ok(())
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
    match tokio::fs::remove_dir_all(root).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(
            "cleanup_incomplete",
            "remove the owned sandbox root",
            &error,
        )),
    }
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
    fn plan_starts_cyclic_service_contracts_in_deterministic_order() {
        let root = test_root("cyclic-plan");
        fs::create_dir_all(&root).unwrap();
        let system_file = root.join(DEFAULT_SYSTEM_FILE);
        let sandbox_file = root.join(DEFAULT_SANDBOX_FILE);
        let mut system = system_fixture();
        system["contracts"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "contractId": "notifications-event.v1",
                "version": "v1",
                "producerKind": "autonomous_service",
                "producerId": "notifications",
                "artifact": { "format": "json_schema", "path": "notifications.v1.json" },
                "tenancyMode": "none"
            }));
        system["consumers"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "consumerId": "support-notifications",
                "ownerKind": "autonomous_service",
                "ownerId": "support",
                "contractId": "notifications-event.v1",
                "tenancyMode": "none"
            }));

        let plan = build_plan(
            &system,
            &sandbox_fixture(false),
            &system_file,
            &sandbox_file,
        )
        .unwrap();

        assert_eq!(
            plan.workloads
                .iter()
                .map(|item| item.service_id.as_str())
                .collect::<Vec<_>>(),
            [
                "notifications",
                "notifications",
                "notifications",
                "support",
                "support",
                "support",
            ]
        );
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

    #[tokio::test]
    async fn workload_control_observe_requires_bearer_and_never_leaks_process_identity() {
        let root = test_root("workload-control-observe");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let running = launch(plan, false).await.unwrap();
        let mut server = start_workload_control_server(
            running,
            "workload-control:support-platform".to_owned(),
            "server-side-secret".to_owned(),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let observe_url = format!("{}/workload-control/v1/observe", server.endpoint());
        let request = serde_json::json!({
            "protocol": WORKLOAD_CONTROL_PROTOCOL,
            "workload": {
                "systemId": "support-platform",
                "serviceId": "support",
                "workloadId": "support-api"
            }
        });

        assert!(server.local_addr().ip().is_loopback());
        let unauthorized = client
            .post(&observe_url)
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(
            unauthorized.json::<Value>().await.unwrap()["code"],
            "unauthenticated"
        );

        let malformed = client
            .post(&observe_url)
            .bearer_auth("server-side-secret")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body("{")
            .send()
            .await
            .unwrap();
        assert_eq!(malformed.status(), reqwest::StatusCode::BAD_REQUEST);
        let malformed: Value = malformed.json().await.unwrap();
        assert_eq!(malformed["code"], "incompatible_protocol");
        assert!(malformed.get("state").is_none());

        let mut invalid_reference = request.clone();
        invalid_reference["workload"]["serviceId"] = Value::String("   ".to_owned());
        let invalid_reference = client
            .post(&observe_url)
            .bearer_auth("server-side-secret")
            .json(&invalid_reference)
            .send()
            .await
            .unwrap();
        assert_eq!(invalid_reference.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(
            invalid_reference.json::<Value>().await.unwrap()["code"],
            "incompatible_protocol"
        );

        let response = client
            .post(observe_url)
            .bearer_auth("server-side-secret")
            .json(&request)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let source = response.text().await.unwrap();
        let observation: Value = serde_json::from_str(&source).unwrap();
        assert_eq!(observation["protocol"], WORKLOAD_CONTROL_PROTOCOL);
        assert_eq!(observation["state"], "running");
        assert!(observation["observedRevision"].is_string());
        assert!(observation["observedAtUnixMs"].is_u64());
        assert!(observation["activeOperation"].is_null());
        assert_eq!(
            observation["capabilities"],
            serde_json::json!(["suspend", "resume"])
        );
        assert!(!source.contains("processId"));
        assert!(!source.contains("adapterPid"));
        assert!(!source.contains("ownershipToken"));
        assert!(!source.contains("server-side-secret"));

        server.shutdown().await.unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn workload_control_suspends_and_resumes_only_the_target_through_async_operations() {
        let root = test_root("workload-control-suspend-resume");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let running = launch(plan, false).await.unwrap();
        let mut server = start_workload_control_server(
            running,
            "workload-control:support-platform".to_owned(),
            "control-token".to_owned(),
            Duration::from_millis(5),
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let target = serde_json::json!({
            "systemId": "support-platform",
            "serviceId": "support",
            "workloadId": "support-api"
        });
        let other = serde_json::json!({
            "systemId": "support-platform",
            "serviceId": "notifications",
            "workloadId": "notifications-api"
        });
        let target_before = observe_workload(&client, server.endpoint(), &target).await;
        let other_before = observe_workload(&client, server.endpoint(), &other).await;

        let suspended = submit_workload_operation(
            &client,
            server.endpoint(),
            &target,
            target_before["observedRevision"].as_str().unwrap(),
            "suspend-support",
            "suspend",
        )
        .await;
        assert_eq!(suspended["phase"], "accepted");
        let suspend_record = wait_for_workload_operation(
            &client,
            server.endpoint(),
            suspended["operationId"].as_str().unwrap(),
        )
        .await;
        assert_eq!(suspend_record["phase"], "succeeded");
        assert_eq!(suspend_record["authority"]["decision"], "accepted");
        assert_eq!(suspend_record["result"]["state"], "suspended");
        let target_suspended = observe_workload(&client, server.endpoint(), &target).await;
        let other_during_suspend = observe_workload(&client, server.endpoint(), &other).await;
        assert_eq!(target_suspended["state"], "suspended");
        assert_ne!(
            target_suspended["observedRevision"],
            target_before["observedRevision"]
        );
        assert_eq!(other_during_suspend["state"], "running");
        assert_eq!(
            other_during_suspend["observedRevision"],
            other_before["observedRevision"]
        );

        let resumed = submit_workload_operation(
            &client,
            server.endpoint(),
            &target,
            target_suspended["observedRevision"].as_str().unwrap(),
            "resume-support",
            "resume",
        )
        .await;
        let resume_record = wait_for_workload_operation(
            &client,
            server.endpoint(),
            resumed["operationId"].as_str().unwrap(),
        )
        .await;
        assert_eq!(resume_record["phase"], "succeeded");
        assert_eq!(resume_record["result"]["state"], "running");
        let target_resumed = observe_workload(&client, server.endpoint(), &target).await;
        let other_after_resume = observe_workload(&client, server.endpoint(), &other).await;
        assert_eq!(target_resumed["state"], "running");
        assert_eq!(other_after_resume["state"], "running");
        for response in [suspend_record, resume_record, target_resumed] {
            let source = serde_json::to_string(&response).unwrap();
            assert!(!source.contains("processId"));
            assert!(!source.contains("health"));
        }

        server.shutdown().await.unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn workload_control_failure_reconciles_state_and_advances_its_revision() {
        let root = test_root("workload-control-failure-revision");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let running = launch(plan, false).await.unwrap();
        let mut server = start_workload_control_server(
            running,
            "workload-control:support-platform".to_owned(),
            "control-token".to_owned(),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let target = serde_json::json!({
            "systemId": "support-platform",
            "serviceId": "support",
            "workloadId": "support-api"
        });
        let before = observe_workload(&client, server.endpoint(), &target).await;
        {
            let mut sandbox = server.sandbox.lock().await;
            let state_index = sandbox
                .state
                .workloads
                .iter()
                .position(|workload| {
                    workload.service_id == "support" && workload.workload_id == "support-api"
                })
                .unwrap();
            let process_index = sandbox
                .processes
                .iter()
                .position(|process| process.state_index == state_index)
                .unwrap();
            let mut process = sandbox.processes.remove(process_index);
            stop_owned_child(&mut process.child, process.process_group_id)
                .await
                .unwrap();
        }

        let accepted = submit_workload_operation(
            &client,
            server.endpoint(),
            &target,
            before["observedRevision"].as_str().unwrap(),
            "missing-process",
            "suspend",
        )
        .await;
        let failed = wait_for_workload_operation(
            &client,
            server.endpoint(),
            accepted["operationId"].as_str().unwrap(),
        )
        .await;
        assert_eq!(failed["phase"], "failed");
        assert_eq!(failed["failure"]["code"], "authority_unavailable");
        let after = observe_workload(&client, server.endpoint(), &target).await;
        assert_eq!(after["state"], "failed");
        assert_ne!(after["observedRevision"], before["observedRevision"]);

        server.shutdown().await.unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn workload_control_enforces_idempotency_revision_and_one_active_mutation() {
        let root = test_root("workload-control-concurrency");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let running = launch(plan, false).await.unwrap();
        let mut server = start_workload_control_server(
            running,
            "workload-control:support-platform".to_owned(),
            "control-token".to_owned(),
            Duration::from_millis(100),
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let target = serde_json::json!({
            "systemId": "support-platform",
            "serviceId": "support",
            "workloadId": "support-api"
        });
        let observed = observe_workload(&client, server.endpoint(), &target).await;
        let revision = observed["observedRevision"].as_str().unwrap();

        let first = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            revision,
            "same-key",
            serde_json::json!({ "kind": "suspend" }),
        )
        .await;
        assert_eq!(first.status(), reqwest::StatusCode::ACCEPTED);
        let first: Value = first.json().await.unwrap();

        let retry = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            revision,
            "same-key",
            serde_json::json!({ "kind": "suspend" }),
        )
        .await;
        assert_eq!(retry.status(), reqwest::StatusCode::ACCEPTED);
        let retry: Value = retry.json().await.unwrap();
        assert_eq!(retry["operationId"], first["operationId"]);

        let conflict = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            revision,
            "same-key",
            serde_json::json!({ "kind": "resume" }),
        )
        .await;
        assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
        let conflict: Value = conflict.json().await.unwrap();
        assert_eq!(conflict["code"], "idempotency_conflict");
        assert_eq!(conflict["operationId"], first["operationId"]);

        let active = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            revision,
            "different-key",
            serde_json::json!({ "kind": "suspend" }),
        )
        .await;
        assert_eq!(active.status(), reqwest::StatusCode::CONFLICT);
        let active: Value = active.json().await.unwrap();
        assert_eq!(active["code"], "active_mutation");
        assert_eq!(active["activeOperation"], first["operationId"]);

        let completed = wait_for_workload_operation(
            &client,
            server.endpoint(),
            first["operationId"].as_str().unwrap(),
        )
        .await;
        assert_eq!(completed["phase"], "succeeded");

        let completed_retry = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            revision,
            "same-key",
            serde_json::json!({ "kind": "suspend" }),
        )
        .await;
        assert_eq!(completed_retry.status(), reqwest::StatusCode::ACCEPTED);
        assert_eq!(
            completed_retry.json::<Value>().await.unwrap()["operationId"],
            first["operationId"]
        );

        let stale = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            revision,
            "stale-key",
            serde_json::json!({ "kind": "resume" }),
        )
        .await;
        assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
        let stale: Value = stale.json().await.unwrap();
        assert_eq!(stale["code"], "stale_revision");
        assert_eq!(
            stale["currentRevision"],
            completed["result"]["observedRevision"]
        );

        let suspended = observe_workload(&client, server.endpoint(), &target).await;
        let operation_count = server.runtime.lock().await.operations.len();
        for (idempotency_key, action) in [
            (
                "suspend-extra-field",
                serde_json::json!({ "kind": "suspend", "targetCapacity": 2 }),
            ),
            (
                "restart-extra-field",
                serde_json::json!({ "kind": "restart", "providerId": "local" }),
            ),
            (
                "resume-extra-field",
                serde_json::json!({ "kind": "resume", "providerId": "local" }),
            ),
            (
                "scale-snake-case",
                serde_json::json!({ "kind": "scale", "target_capacity": 2 }),
            ),
            (
                "scale-extra-field",
                serde_json::json!({ "kind": "scale", "targetCapacity": 2, "replicas": 2 }),
            ),
        ] {
            let malformed = post_workload_operation(
                &client,
                server.endpoint(),
                &target,
                suspended["observedRevision"].as_str().unwrap(),
                idempotency_key,
                action,
            )
            .await;
            assert_eq!(malformed.status(), reqwest::StatusCode::BAD_REQUEST);
            assert_eq!(
                malformed.json::<Value>().await.unwrap()["code"],
                "incompatible_protocol"
            );
        }
        assert_eq!(
            server.runtime.lock().await.operations.len(),
            operation_count
        );

        let unsupported = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            suspended["observedRevision"].as_str().unwrap(),
            "restart-key",
            serde_json::json!({ "kind": "restart" }),
        )
        .await;
        assert_eq!(
            unsupported.status(),
            reqwest::StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            unsupported.json::<Value>().await.unwrap()["code"],
            "unsupported_action"
        );
        let unsupported_scale = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            suspended["observedRevision"].as_str().unwrap(),
            "scale-key",
            serde_json::json!({ "kind": "scale", "targetCapacity": 2 }),
        )
        .await;
        assert_eq!(
            unsupported_scale.status(),
            reqwest::StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            unsupported_scale.json::<Value>().await.unwrap()["code"],
            "unsupported_action"
        );
        let invalid_capacity = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            suspended["observedRevision"].as_str().unwrap(),
            "invalid-scale-key",
            serde_json::json!({ "kind": "scale", "targetCapacity": 0 }),
        )
        .await;
        assert_eq!(invalid_capacity.status(), reqwest::StatusCode::BAD_REQUEST);
        assert_eq!(
            invalid_capacity.json::<Value>().await.unwrap()["code"],
            "invalid_capacity"
        );
        assert_eq!(
            server.runtime.lock().await.operations.len(),
            operation_count
        );

        server.shutdown().await.unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn workload_control_reports_unknown_without_queueing_and_protects_control_plane() {
        let root = test_root("workload-control-unavailable-protected");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let running = launch(plan, false).await.unwrap();
        let mut server = start_workload_control_server(
            running,
            "workload-control:support-platform".to_owned(),
            "control-token".to_owned(),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let target = serde_json::json!({
            "systemId": "support-platform",
            "serviceId": "support",
            "workloadId": "support-api"
        });
        let before = observe_workload(&client, server.endpoint(), &target).await;
        server.runtime.lock().await.available = false;

        let unavailable = observe_workload(&client, server.endpoint(), &target).await;
        assert_eq!(unavailable["state"], "unknown");
        assert!(unavailable.get("observedRevision").is_none());
        let rejected = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            before["observedRevision"].as_str().unwrap(),
            "must-not-queue",
            serde_json::json!({ "kind": "suspend" }),
        )
        .await;
        assert_eq!(rejected.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        let rejected: Value = rejected.json().await.unwrap();
        assert_eq!(rejected["code"], "authority_unavailable");
        assert!(rejected.get("state").is_none());

        server.runtime.lock().await.available = true;
        let restored = observe_workload(&client, server.endpoint(), &target).await;
        assert_eq!(restored["state"], "running");
        assert_eq!(restored["observedRevision"], before["observedRevision"]);
        assert!(restored["activeOperation"].is_null());

        for protected in [
            serde_json::json!({
                "systemId": "support-platform",
                "serviceId": "lenso-console",
                "workloadId": "lenso-console"
            }),
            serde_json::json!({
                "systemId": "support-platform",
                "serviceId": "control-plane",
                "workloadId": "workload-control:support-platform"
            }),
            serde_json::json!({
                "systemId": "support-platform",
                "serviceId": "workload-control:support-platform",
                "workloadId": "adapter-process"
            }),
        ] {
            let observation = observe_workload(&client, server.endpoint(), &protected).await;
            assert_eq!(observation["protection"], "control_plane");
            assert_eq!(observation["state"], "unknown");
            assert!(observation.get("observedRevision").is_none());
            let response = post_workload_operation(
                &client,
                server.endpoint(),
                &protected,
                "protected-revision",
                "protected-key",
                serde_json::json!({ "kind": "suspend" }),
            )
            .await;
            assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
            assert_eq!(
                response.json::<Value>().await.unwrap()["code"],
                "protected_workload"
            );
        }

        server.shutdown().await.unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn generated_workload_control_credential_is_owner_only_and_state_contains_only_its_reference() {
        let root = test_root("workload-control-credential");
        fs::create_dir_all(&root).unwrap();
        let plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();

        let credential = resolve_workload_control_credential(&plan, None).unwrap();
        let credential_file = credential.file.clone().unwrap();
        assert_eq!(
            fs::read_to_string(&credential_file).unwrap(),
            credential.token
        );
        #[cfg(unix)]
        assert_eq!(
            fs::metadata(&credential_file).unwrap().permissions().mode() & 0o077,
            0
        );
        let state = adapter_state_with_control(
            &plan,
            Some(1234),
            SandboxPhase::Ready,
            Some("http://127.0.0.1:43210".to_owned()),
            credential.file,
        );
        let document = serde_json::to_value(&state).unwrap();
        assert_eq!(
            document["workloadControlSchemaDigest"],
            "sha256:d3666bb1fd85576f9af4205dbcc70029acd81462678c47d2b315c40ef1a9161d"
        );
        let source = serde_json::to_string(&document).unwrap();
        assert!(source.contains("credentialFile"));
        assert!(!source.contains(&credential.token));

        let overridden =
            resolve_workload_control_credential(&plan, Some("console-server-token".to_owned()))
                .unwrap();
        assert!(overridden.file.is_none());
        let overridden_state = adapter_state_with_control(
            &plan,
            Some(1234),
            SandboxPhase::Ready,
            Some("http://127.0.0.1:43210".to_owned()),
            overridden.file,
        );
        let overridden_source = serde_json::to_string(&overridden_state).unwrap();
        assert!(!overridden_source.contains("credentialFile"));
        assert!(!overridden_source.contains(&overridden.token));

        fs::remove_dir_all(root).ok();
    }

    #[tokio::test]
    async fn workload_control_bearer_is_not_inherited_by_workloads() {
        let root = test_root("workload-control-secret-boundary");
        fs::create_dir_all(&root).unwrap();
        let mut plan = build_plan(
            &system_fixture(),
            &sandbox_fixture(false),
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let leak_marker = root.join("workload-received-control-token");
        let workload = plan
            .workloads
            .iter_mut()
            .find(|workload| workload.role != WorkloadRole::Migration)
            .unwrap();
        workload.env.insert(
            WORKLOAD_CONTROL_TOKEN_ENV.to_owned(),
            "must-not-reach-workload".to_owned(),
        );
        workload.command = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "if [ -n \"$LENSO_WORKLOAD_CONTROL_TOKEN\" ]; then : > '{}'; fi; exec sleep 30",
                leak_marker.display()
            ),
        ];

        let mut running = launch(plan, false).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!leak_marker.exists());
        running.shutdown().await.unwrap();
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn workload_control_request_scalars_use_the_shared_unicode_bound() {
        let mut request = WorkloadMutationRequest {
            protocol: WORKLOAD_CONTROL_PROTOCOL.to_owned(),
            workload: WorkloadReference {
                system_id: "support-platform".to_owned(),
                service_id: "support".to_owned(),
                workload_id: "support-api".to_owned(),
            },
            action: WorkloadControlAction::Suspend,
            observed_revision: "revision".to_owned(),
            idempotency_key: "idempotency-key".to_owned(),
            actor: WorkloadControlActor {
                kind: WorkloadControlActorKind::Operator,
                subject: "operator:test".to_owned(),
            },
        };

        request.workload.service_id = "界".repeat(255);
        request.observed_revision = "界".repeat(255);
        request.idempotency_key = "界".repeat(255);
        request.actor.subject = "界".repeat(255);
        assert!(validate_mutation_request(&request).is_ok());

        for invalid in [" ".to_owned(), "界".repeat(256)] {
            request.observed_revision = invalid.clone();
            assert!(validate_mutation_request(&request).is_err());
            request.observed_revision = "revision".to_owned();

            request.idempotency_key = invalid.clone();
            assert!(validate_mutation_request(&request).is_err());
            request.idempotency_key = "idempotency-key".to_owned();

            request.actor.subject = invalid;
            assert!(validate_mutation_request(&request).is_err());
            request.actor.subject = "operator:test".to_owned();
        }
    }

    #[tokio::test]
    async fn workload_operation_remains_observable_and_rejects_conflict_during_real_execution() {
        let root = test_root("workload-control-executing");
        fs::create_dir_all(&root).unwrap();
        let mut definition = sandbox_fixture(false);
        let target_definition = definition
            .services
            .iter_mut()
            .find(|service| service.service_id == "support")
            .unwrap()
            .workloads
            .iter_mut()
            .find(|workload| workload.workload_id == "support-api")
            .unwrap();
        target_definition.command = vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "trap '' TERM; while :; do sleep 1; done".to_owned(),
        ];
        let plan = build_plan(
            &system_fixture(),
            &definition,
            &root.join(DEFAULT_SYSTEM_FILE),
            &root.join(DEFAULT_SANDBOX_FILE),
        )
        .unwrap();
        let running = launch(plan, false).await.unwrap();
        let mut server = start_workload_control_server(
            running,
            "workload-control:support-platform".to_owned(),
            "control-token".to_owned(),
            Duration::ZERO,
        )
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let target = serde_json::json!({
            "systemId": "support-platform",
            "serviceId": "support",
            "workloadId": "support-api"
        });
        let observed = observe_workload(&client, server.endpoint(), &target).await;
        let revision = observed["observedRevision"].as_str().unwrap();
        let accepted = post_workload_operation(
            &client,
            server.endpoint(),
            &target,
            revision,
            "slow-suspend",
            serde_json::json!({ "kind": "suspend" }),
        )
        .await;
        let accepted: Value = accepted.json().await.unwrap();
        let operation_id = accepted["operationId"].as_str().unwrap();
        let active_observation = observe_workload(&client, server.endpoint(), &target).await;
        let active_operation = active_observation["activeOperation"].as_str().unwrap();
        assert_eq!(active_operation, operation_id);
        assert!(valid_workload_control_scalar(active_operation));

        let executing = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                let response = client
                    .get(format!(
                        "{}/workload-control/v1/operations/{operation_id}",
                        server.endpoint()
                    ))
                    .bearer_auth("control-token")
                    .send()
                    .await
                    .unwrap();
                let record: Value = response.json().await.unwrap();
                if record["phase"] == "executing" {
                    break record;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("executing Operation Record must remain observable");
        assert_eq!(executing["phase"], "executing");
        let conflict = tokio::time::timeout(
            Duration::from_millis(250),
            post_workload_operation(
                &client,
                server.endpoint(),
                &target,
                revision,
                "must-not-queue",
                serde_json::json!({ "kind": "suspend" }),
            ),
        )
        .await
        .expect("same-Workload conflict must not wait behind process control");
        assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
        assert_eq!(
            conflict.json::<Value>().await.unwrap()["code"],
            "active_mutation"
        );

        let completed = wait_for_workload_operation(&client, server.endpoint(), operation_id).await;
        assert_eq!(completed["phase"], "succeeded");
        server.shutdown().await.unwrap();
        fs::remove_dir_all(root).ok();
    }

    async fn observe_workload(
        client: &reqwest::Client,
        endpoint: String,
        workload: &Value,
    ) -> Value {
        let response = client
            .post(format!("{endpoint}/workload-control/v1/observe"))
            .bearer_auth("control-token")
            .json(&serde_json::json!({
                "protocol": WORKLOAD_CONTROL_PROTOCOL,
                "workload": workload
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        response.json().await.unwrap()
    }

    async fn submit_workload_operation(
        client: &reqwest::Client,
        endpoint: String,
        workload: &Value,
        observed_revision: &str,
        idempotency_key: &str,
        capability: &str,
    ) -> Value {
        let response = post_workload_operation(
            client,
            endpoint,
            workload,
            observed_revision,
            idempotency_key,
            serde_json::json!({ "kind": capability }),
        )
        .await;
        assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
        response.json().await.unwrap()
    }

    async fn post_workload_operation(
        client: &reqwest::Client,
        endpoint: String,
        workload: &Value,
        observed_revision: &str,
        idempotency_key: &str,
        action: Value,
    ) -> reqwest::Response {
        client
            .post(format!("{endpoint}/workload-control/v1/operations"))
            .bearer_auth("control-token")
            .json(&serde_json::json!({
                "protocol": WORKLOAD_CONTROL_PROTOCOL,
                "workload": workload,
                "observedRevision": observed_revision,
                "idempotencyKey": idempotency_key,
                "action": action,
                "actor": {
                    "kind": "operator",
                    "subject": "operator:test"
                }
            }))
            .send()
            .await
            .unwrap()
    }

    async fn wait_for_workload_operation(
        client: &reqwest::Client,
        endpoint: String,
        operation_id: &str,
    ) -> Value {
        for _ in 0..200 {
            let response = client
                .get(format!(
                    "{endpoint}/workload-control/v1/operations/{operation_id}"
                ))
                .bearer_auth("control-token")
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            let record: Value = response.json().await.unwrap();
            if matches!(
                record["phase"].as_str(),
                Some("succeeded" | "failed" | "denied")
            ) {
                return record;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("Workload operation did not reach a terminal phase")
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
