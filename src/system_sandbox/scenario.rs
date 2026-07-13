use std::collections::{BTreeMap, BTreeSet};

use lenso_service::WorkloadRole;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use super::{
    PlannedWorkload, RunningSandbox, SandboxError, SandboxPhase, SandboxWorkload, persist_state,
    rollback_launch, stop_owned_child, write_json_async,
};

const RESULT_PROTOCOL: &str = "lenso.failure-scenario-result.v1";
const STORY_PROTOCOL: &str = "lenso.story-segment.v1";
const WORKLOAD_OBSERVATION_PROTOCOL: &str = "lenso.sandbox-workload-observation.v1";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailureScenario {
    scenario_id: String,
    fault: InjectedFault,
    call_policy: ScenarioCallPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InjectedFault {
    kind: FaultKind,
    service_id: String,
    workload_id: String,
    #[serde(default)]
    delay_ms: u64,
    #[serde(default)]
    capacity: u32,
    #[serde(default)]
    demand: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FaultKind {
    Timeout,
    SlowDependency,
    WorkloadCrash,
    Overload,
    PartialUnavailability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScenarioCallPolicy {
    deadline_ms: u64,
    max_attempts: u32,
    idempotent: bool,
}

#[derive(Debug)]
struct InjectionObservation {
    failure: ScenarioFailure,
    attempts: u32,
    retry_attempted: bool,
    retry_reason: String,
    controlled_time_end_ms: u64,
    final_health: String,
    health_reason: String,
}

#[derive(Debug, Clone, Copy)]
enum ScenarioFailure {
    DeadlineExceeded,
    WorkloadCrashed,
    OverloadRejected,
    PartialUnavailability,
}

impl ScenarioFailure {
    const fn outcome(self) -> &'static str {
        match self {
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::WorkloadCrashed => "workload_crashed",
            Self::OverloadRejected => "overload_rejected",
            Self::PartialUnavailability => "partial_unavailability_observed",
        }
    }

    const fn expected_final_health(self) -> &'static str {
        match self {
            Self::DeadlineExceeded | Self::OverloadRejected => "ready",
            Self::WorkloadCrashed | Self::PartialUnavailability => "failed",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkloadScenarioObservation {
    artifact_version: String,
    outcome: String,
    attempts: u32,
    retry_attempted: bool,
    retry_reason: String,
    controlled_time_end_ms: u64,
    final_health: String,
    health_reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FailureScenarioResult {
    artifact_version: &'static str,
    pub(super) scenario_id: String,
    injected_fault: InjectedFault,
    affected: AffectedWorkload,
    attempts: u32,
    retry_decision: RetryDecision,
    call_policy_evidence: CallPolicyEvidence,
    health_transitions: Vec<HealthTransition>,
    observed_service_behavior: ObservedServiceBehavior,
    pub(super) outcome: String,
    cleanup: CleanupResult,
    next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AffectedWorkload {
    service_id: String,
    workload_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RetryDecision {
    attempted: bool,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CallPolicyEvidence {
    deadline_ms: u64,
    max_attempts: u32,
    idempotent: bool,
    controlled_time_start_ms: u64,
    controlled_time_end_ms: u64,
    remaining_deadline_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthTransition {
    service_id: String,
    workload_id: String,
    from: String,
    to: String,
    controlled_time_ms: u64,
    reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservedServiceBehavior {
    service_id: String,
    decision: String,
    workloads: Vec<ObservedWorkload>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservedWorkload {
    workload_id: String,
    phase: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CleanupResult {
    completed: bool,
    managed_processes_stopped: usize,
    sandbox_state_removed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScenarioStorySegment<'a> {
    artifact_version: &'static str,
    story_id: String,
    segment_id: String,
    service_id: &'a str,
    workload_id: &'a str,
    kind: &'static str,
    result: &'a FailureScenarioResult,
}

pub(super) fn validate(
    scenarios: &[FailureScenario],
    configured: &BTreeMap<(String, String), &SandboxWorkload>,
    declared: &BTreeMap<(String, String), WorkloadRole>,
) -> Result<(), SandboxError> {
    let mut ids = BTreeSet::new();
    for scenario in scenarios {
        if !super::is_safe_identity(&scenario.scenario_id) {
            return Err(invalid_scenario(format!(
                "Failure Scenario identity is not a safe path component: {}",
                scenario.scenario_id
            )));
        }
        if !ids.insert(scenario.scenario_id.as_str()) {
            return Err(invalid_scenario(format!(
                "Failure Scenario {} is duplicated.",
                scenario.scenario_id
            )));
        }
        let key = (
            scenario.fault.service_id.clone(),
            scenario.fault.workload_id.clone(),
        );
        if !configured.contains_key(&key) || !declared.contains_key(&key) {
            return Err(invalid_scenario(format!(
                "Failure Scenario {} targets undeclared Workload {}/{}.",
                scenario.scenario_id, scenario.fault.service_id, scenario.fault.workload_id
            )));
        }
        if matches!(
            scenario.fault.kind,
            FaultKind::Timeout | FaultKind::SlowDependency | FaultKind::Overload
        ) && configured[&key].scenario_command.is_empty()
        {
            return Err(invalid_scenario(format!(
                "Failure Scenario {} requires scenarioCommand on Workload {}/{}.",
                scenario.scenario_id, scenario.fault.service_id, scenario.fault.workload_id
            )));
        }
        if declared[&key] == WorkloadRole::Migration
            && matches!(
                scenario.fault.kind,
                FaultKind::WorkloadCrash | FaultKind::PartialUnavailability
            )
        {
            return Err(invalid_scenario(format!(
                "Failure Scenario {} must target a long-running API or Worker Workload.",
                scenario.scenario_id
            )));
        }
        if scenario.call_policy.deadline_ms == 0 || scenario.call_policy.max_attempts == 0 {
            return Err(invalid_scenario(format!(
                "Failure Scenario {} requires a positive Deadline and maxAttempts.",
                scenario.scenario_id
            )));
        }
        match scenario.fault.kind {
            FaultKind::Timeout if scenario.fault.delay_ms < scenario.call_policy.deadline_ms => {
                return Err(invalid_scenario(format!(
                    "Timeout Scenario {} delayMs must reach or exceed its Deadline.",
                    scenario.scenario_id
                )));
            }
            FaultKind::SlowDependency
                if scenario.fault.delay_ms < scenario.call_policy.deadline_ms =>
            {
                return Err(invalid_scenario(format!(
                    "Slow-dependency Scenario {} delayMs must reach or exceed its Deadline.",
                    scenario.scenario_id
                )));
            }
            FaultKind::Overload
                if scenario.fault.capacity == 0
                    || scenario.fault.demand <= scenario.fault.capacity =>
            {
                return Err(invalid_scenario(format!(
                    "Overload Scenario {} requires positive capacity and demand greater than capacity.",
                    scenario.scenario_id
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn ensure_declared(
    scenarios: &[FailureScenario],
    scenario_id: &str,
) -> Result<(), SandboxError> {
    declared_scenario(scenarios, scenario_id).map(|_| ())
}

fn declared_scenario<'a>(
    scenarios: &'a [FailureScenario],
    scenario_id: &str,
) -> Result<&'a FailureScenario, SandboxError> {
    scenarios
        .iter()
        .find(|scenario| scenario.scenario_id == scenario_id)
        .ok_or_else(|| {
            SandboxError::new(
                "scenario_not_found",
                format!("Failure Scenario is not declared: {scenario_id}"),
                "Choose a scenarioId declared in lenso.system-sandbox.json.",
            )
        })
}

pub(super) async fn run(
    running: &mut RunningSandbox,
    scenarios: &[FailureScenario],
    scenario_id: &str,
) -> Result<FailureScenarioResult, SandboxError> {
    let scenario = declared_scenario(scenarios, scenario_id)?.clone();
    let managed_processes = running.processes.len();
    let observation = match inject(running, &scenario).await {
        Ok(observation) => observation,
        Err(error) => return Err(rollback_launch(running, error).await),
    };
    if let Err(error) = persist_state(&running.plan.owned_root, &running.state).await {
        return Err(rollback_launch(running, error).await);
    }
    let outcome = observation.failure.outcome();
    let observed_service_behavior = observe_service(running, &scenario, outcome);
    running.shutdown().await?;

    let remaining_deadline_ms = scenario
        .call_policy
        .deadline_ms
        .saturating_sub(observation.controlled_time_end_ms);
    let result = FailureScenarioResult {
        artifact_version: RESULT_PROTOCOL,
        scenario_id: scenario.scenario_id.clone(),
        injected_fault: scenario.fault.clone(),
        affected: AffectedWorkload {
            service_id: scenario.fault.service_id.clone(),
            workload_id: scenario.fault.workload_id.clone(),
        },
        attempts: observation.attempts,
        retry_decision: RetryDecision {
            attempted: observation.retry_attempted,
            reason: observation.retry_reason.clone(),
        },
        call_policy_evidence: CallPolicyEvidence {
            deadline_ms: scenario.call_policy.deadline_ms,
            max_attempts: scenario.call_policy.max_attempts,
            idempotent: scenario.call_policy.idempotent,
            controlled_time_start_ms: 0,
            controlled_time_end_ms: observation.controlled_time_end_ms,
            remaining_deadline_ms,
        },
        health_transitions: vec![
            HealthTransition {
                service_id: scenario.fault.service_id.clone(),
                workload_id: scenario.fault.workload_id.clone(),
                from: "starting".to_owned(),
                to: "ready".to_owned(),
                controlled_time_ms: 0,
                reason: "sandbox_health_gate_passed".to_owned(),
            },
            HealthTransition {
                service_id: scenario.fault.service_id.clone(),
                workload_id: scenario.fault.workload_id.clone(),
                from: "ready".to_owned(),
                to: observation.final_health.clone(),
                controlled_time_ms: observation.controlled_time_end_ms,
                reason: observation.health_reason.clone(),
            },
        ],
        observed_service_behavior,
        outcome: outcome.to_owned(),
        cleanup: CleanupResult {
            completed: true,
            managed_processes_stopped: managed_processes,
            sandbox_state_removed: true,
        },
        next_actions: next_actions(scenario.fault.kind),
    };
    persist_evidence(running, &result).await?;
    Ok(result)
}

async fn inject(
    running: &mut RunningSandbox,
    scenario: &FailureScenario,
) -> Result<InjectionObservation, SandboxError> {
    match scenario.fault.kind {
        FaultKind::Timeout | FaultKind::SlowDependency | FaultKind::Overload => {
            run_workload_scenario(running, scenario).await
        }
        FaultKind::WorkloadCrash => {
            stop_target(running, &scenario.fault).await?;
            Ok(process_failure_observation(
                ScenarioFailure::WorkloadCrashed,
                "sandbox_injected_workload_crash",
                scenario.call_policy.idempotent,
            ))
        }
        FaultKind::PartialUnavailability => {
            stop_target(running, &scenario.fault).await?;
            if !other_workload_is_ready(running, &scenario.fault) {
                return Err(SandboxError::new(
                    "partial_unavailability_not_observed",
                    format!(
                        "Service {} has no other ready Workload after the injected failure.",
                        scenario.fault.service_id
                    ),
                    "Target one Workload in a Service that keeps another API or Worker ready.",
                ));
            }
            Ok(process_failure_observation(
                ScenarioFailure::PartialUnavailability,
                "target_failed_while_other_service_workloads_remained_ready",
                scenario.call_policy.idempotent,
            ))
        }
    }
}

async fn run_workload_scenario(
    running: &mut RunningSandbox,
    scenario: &FailureScenario,
) -> Result<InjectionObservation, SandboxError> {
    let state_index = target_state_index(&running.plan.workloads, &scenario.fault)?;
    let workload = &running.plan.workloads[state_index];
    let mut command = Command::new(&workload.scenario_command[0]);
    command
        .args(&workload.scenario_command[1..])
        .current_dir(&workload.cwd)
        .envs(&workload.env)
        .env("LENSO_SANDBOX_SCENARIO_ID", &scenario.scenario_id)
        .env("LENSO_SANDBOX_FAULT_KIND", fault_kind(scenario.fault.kind))
        .env(
            "LENSO_SANDBOX_DELAY_MS",
            scenario.fault.delay_ms.to_string(),
        )
        .env(
            "LENSO_SANDBOX_CAPACITY",
            scenario.fault.capacity.to_string(),
        )
        .env("LENSO_SANDBOX_DEMAND", scenario.fault.demand.to_string())
        .env(
            "LENSO_SANDBOX_DEADLINE_MS",
            scenario.call_policy.deadline_ms.to_string(),
        )
        .env(
            "LENSO_SANDBOX_MAX_ATTEMPTS",
            scenario.call_policy.max_attempts.to_string(),
        )
        .env(
            "LENSO_SANDBOX_IDEMPOTENT",
            scenario.call_policy.idempotent.to_string(),
        )
        .env(
            "LENSO_SANDBOX_OWNERSHIP_TOKEN",
            &running.state.ownership_token,
        )
        .kill_on_drop(true);
    let output = command.output().await.map_err(|error| {
        SandboxError::new(
            "scenario_command_failed",
            format!(
                "Could not run scenarioCommand for {}/{}: {error}",
                workload.service_id, workload.workload_id
            ),
            "Fix the Workload scenarioCommand and rerun the scenario.",
        )
    })?;
    if !output.status.success() {
        return Err(SandboxError::new(
            "scenario_command_failed",
            format!(
                "Workload {}/{} scenarioCommand exited with {}: {}",
                workload.service_id,
                workload.workload_id,
                output.status,
                String::from_utf8_lossy(&output.stderr)
            ),
            "Inspect the scenario command output and rerun the scenario.",
        ));
    }
    let observed: WorkloadScenarioObservation =
        serde_json::from_slice(&output.stdout).map_err(|error| {
            SandboxError::new(
                "scenario_observation_invalid",
                format!("Workload scenario observation is invalid: {error}"),
                "Return one lenso.sandbox-workload-observation.v1 JSON document.",
            )
        })?;
    validate_workload_observation(scenario, observed)
}

fn validate_workload_observation(
    scenario: &FailureScenario,
    observed: WorkloadScenarioObservation,
) -> Result<InjectionObservation, SandboxError> {
    let failure = match observed.outcome.as_str() {
        "deadline_exceeded"
            if matches!(
                scenario.fault.kind,
                FaultKind::Timeout | FaultKind::SlowDependency
            ) =>
        {
            ScenarioFailure::DeadlineExceeded
        }
        "overload_rejected" if scenario.fault.kind == FaultKind::Overload => {
            ScenarioFailure::OverloadRejected
        }
        _ => {
            return Err(SandboxError::new(
                "scenario_observation_mismatch",
                format!(
                    "Workload reported outcome {} for injected fault {}.",
                    observed.outcome,
                    fault_kind(scenario.fault.kind)
                ),
                "Make scenarioCommand report the behavior observed for the injected fault.",
            ));
        }
    };
    let valid = observed.artifact_version == WORKLOAD_OBSERVATION_PROTOCOL
        && observed.attempts > 0
        && observed.attempts <= scenario.call_policy.max_attempts
        && observed.retry_attempted == (observed.attempts > 1)
        && !observed.retry_reason.is_empty()
        && observed.final_health == failure.expected_final_health()
        && !observed.health_reason.is_empty()
        && (!matches!(failure, ScenarioFailure::DeadlineExceeded)
            || observed.controlled_time_end_ms >= scenario.call_policy.deadline_ms);
    if !valid {
        return Err(SandboxError::new(
            "scenario_observation_invalid",
            "Workload scenario observation violates the declared Call Policy or fault contract.",
            "Align attempts, controlled time, health, and protocol with the scenario declaration.",
        ));
    }
    Ok(InjectionObservation {
        failure,
        attempts: observed.attempts,
        retry_attempted: observed.retry_attempted,
        retry_reason: observed.retry_reason,
        controlled_time_end_ms: observed.controlled_time_end_ms,
        final_health: observed.final_health,
        health_reason: observed.health_reason,
    })
}

fn process_failure_observation(
    failure: ScenarioFailure,
    health_reason: &str,
    idempotent: bool,
) -> InjectionObservation {
    InjectionObservation {
        failure,
        attempts: 1,
        retry_attempted: false,
        retry_reason: if idempotent {
            "target_unavailable"
        } else {
            "unsafe_operation"
        }
        .to_owned(),
        controlled_time_end_ms: 0,
        final_health: failure.expected_final_health().to_owned(),
        health_reason: health_reason.to_owned(),
    }
}

const fn fault_kind(kind: FaultKind) -> &'static str {
    match kind {
        FaultKind::Timeout => "timeout",
        FaultKind::SlowDependency => "slow_dependency",
        FaultKind::WorkloadCrash => "workload_crash",
        FaultKind::Overload => "overload",
        FaultKind::PartialUnavailability => "partial_unavailability",
    }
}

fn other_workload_is_ready(running: &RunningSandbox, fault: &InjectedFault) -> bool {
    running.state.workloads.iter().any(|workload| {
        workload.service_id == fault.service_id
            && workload.workload_id != fault.workload_id
            && workload.phase == SandboxPhase::Ready
    })
}

fn observe_service(
    running: &RunningSandbox,
    scenario: &FailureScenario,
    outcome: &str,
) -> ObservedServiceBehavior {
    let workloads = running
        .state
        .workloads
        .iter()
        .filter(|workload| workload.service_id == scenario.fault.service_id)
        .map(|workload| ObservedWorkload {
            workload_id: workload.workload_id.clone(),
            phase: phase_name(workload.phase),
        })
        .collect();
    ObservedServiceBehavior {
        service_id: scenario.fault.service_id.clone(),
        decision: outcome.to_owned(),
        workloads,
    }
}

const fn phase_name(phase: SandboxPhase) -> &'static str {
    match phase {
        SandboxPhase::Starting => "starting",
        SandboxPhase::Ready => "ready",
        SandboxPhase::Completed => "completed",
        SandboxPhase::Stopping => "stopping",
        SandboxPhase::Stopped => "stopped",
        SandboxPhase::Failed => "failed",
    }
}

async fn stop_target(
    running: &mut RunningSandbox,
    fault: &InjectedFault,
) -> Result<(), SandboxError> {
    let state_index = target_state_index(&running.plan.workloads, fault)?;
    let process_index = running
        .processes
        .iter()
        .position(|process| process.state_index == state_index)
        .ok_or_else(|| {
            SandboxError::new(
                "scenario_target_not_running",
                format!(
                    "Failure Scenario target is not running: {}/{}",
                    fault.service_id, fault.workload_id
                ),
                "Target a long-running API or Worker Workload and rerun the scenario.",
            )
        })?;
    let process = &mut running.processes[process_index];
    stop_owned_child(&mut process.child, process.process_group_id)
        .await
        .map_err(|error| {
            SandboxError::new(
                "scenario_injection_failed",
                format!("Could not stop the target Workload: {error}"),
                "Run sandbox cleanup, inspect the target process group, and retry.",
            )
        })?;
    running.processes.remove(process_index);
    let workload = &mut running.state.workloads[state_index];
    workload.phase = SandboxPhase::Failed;
    workload.process_id = None;
    Ok(())
}

fn target_state_index(
    workloads: &[PlannedWorkload],
    fault: &InjectedFault,
) -> Result<usize, SandboxError> {
    workloads
        .iter()
        .position(|workload| {
            workload.service_id == fault.service_id && workload.workload_id == fault.workload_id
        })
        .ok_or_else(|| {
            SandboxError::new(
                "scenario_target_missing",
                format!(
                    "Failure Scenario target is missing from the Sandbox plan: {}/{}",
                    fault.service_id, fault.workload_id
                ),
                "Fix the Failure Scenario target and rerun the dry-run.",
            )
        })
}

async fn persist_evidence(
    running: &RunningSandbox,
    result: &FailureScenarioResult,
) -> Result<(), SandboxError> {
    let lenso_dir = running
        .plan
        .owned_root
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or_else(|| {
            SandboxError::new(
                "scenario_evidence_failed",
                "Sandbox result root could not be derived.",
                "Keep Sandbox state under the System .lenso directory.",
            )
        })?;
    let result_dir = lenso_dir
        .join("system-sandbox-results")
        .join(&running.plan.system_id)
        .join(&result.scenario_id);
    tokio::fs::create_dir_all(&result_dir)
        .await
        .map_err(|error| {
            super::io_error(
                "scenario_evidence_failed",
                "create Failure Scenario evidence directory",
                &error,
            )
        })?;
    write_json_async(&result_dir.join("result.json"), result).await?;
    let story = ScenarioStorySegment {
        artifact_version: STORY_PROTOCOL,
        story_id: format!("sandbox:{}", result.scenario_id),
        segment_id: format!(
            "sandbox:{}:{}/{}",
            result.scenario_id, result.affected.service_id, result.affected.workload_id
        ),
        service_id: &result.affected.service_id,
        workload_id: &result.affected.workload_id,
        kind: "failure_scenario",
        result,
    };
    write_json_async(&result_dir.join("story-segment.json"), &story).await
}

fn next_actions(kind: FaultKind) -> Vec<String> {
    let inspect = match kind {
        FaultKind::Timeout => "Inspect Deadline propagation and timeout evidence.",
        FaultKind::SlowDependency => {
            "Inspect remaining Deadline behavior at the dependency boundary."
        }
        FaultKind::WorkloadCrash => "Inspect failed Workload health and retry suppression.",
        FaultKind::Overload => "Inspect declared capacity and overload rejection evidence.",
        FaultKind::PartialUnavailability => {
            "Inspect the unavailable Workload while confirming other Workloads stayed ready."
        }
    };
    vec![
        inspect.to_owned(),
        "Rerun the same scenario to verify an equivalent result.".to_owned(),
    ]
}

fn invalid_scenario(message: String) -> SandboxError {
    SandboxError::new(
        "invalid_failure_scenario",
        message,
        "Fix the declared Failure Scenario and rerun the dry-run.",
    )
}
