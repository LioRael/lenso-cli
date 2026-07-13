use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

use serde_json::Value;

struct ScenarioExpectation {
    scenario_id: &'static str,
    kind: &'static str,
    outcome: &'static str,
    attempts: u32,
    retried: bool,
    retry_reason: &'static str,
    controlled_time_end_ms: u64,
    health_reason: &'static str,
}

fn fixture_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    PathBuf::from("/tmp").join(format!(
        "lenso-cli-system-scenarios-{}-{nonce}",
        std::process::id()
    ))
}

fn run_scenario(root: &Path, scenario_id: &str) -> Value {
    run_command_json(
        root,
        &["system", "dev", "--scenario", scenario_id, "--json"],
        true,
    )
}

fn run_json(root: &Path, arguments: &[&str]) -> Value {
    run_command_json(root, arguments, true)
}

fn run_json_failure(root: &Path, arguments: &[&str]) -> Value {
    run_command_json(root, arguments, false)
}

fn run_command_json(root: &Path, arguments: &[&str], expect_success: bool) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .current_dir(root)
        .args(arguments)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        expect_success,
        "unexpected command status: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not JSON: {error}; stderr={}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn declared_failure_scenarios_are_repeatable_and_leave_durable_evidence() {
    let root = fixture_root();
    fs::create_dir_all(&root).unwrap();
    let fixture_binary = compile_workload_fixture(&root);
    write_system_fixture(&root);
    write_sandbox_fixture(&root, &fixture_binary);

    let ordinary_plan = run_json(&root, &["system", "dev", "--dry-run", "--json"]);
    assert_eq!(ordinary_plan["protocol"], "lenso.system-sandbox-plan.v1");
    assert!(ordinary_plan.get("scenarios").is_none());
    for workload in ordinary_plan["workloads"].as_array().unwrap() {
        assert!(
            workload["env"]
                .as_object()
                .unwrap()
                .keys()
                .all(|key| !key.contains("FAILURE") && !key.contains("SCENARIO"))
        );
    }
    assert!(!root.join(".lenso/system-sandbox/support-platform").exists());

    let missing = run_json_failure(&root, &["system", "dev", "--scenario", "missing", "--json"]);
    assert_eq!(missing["artifactVersion"], "lenso.command-error.v1");
    assert_eq!(missing["code"], "scenario_not_found");
    assert!(missing["nextAction"].is_string());
    assert!(!root.join(".lenso/system-sandbox/support-platform").exists());

    for expected in [
        ScenarioExpectation {
            scenario_id: "deadline-timeout",
            kind: "timeout",
            outcome: "deadline_exceeded",
            attempts: 1,
            retried: false,
            retry_reason: "deadline_exhausted",
            controlled_time_end_ms: 100,
            health_reason: "workload_deadline_rejected",
        },
        ScenarioExpectation {
            scenario_id: "slow-support",
            kind: "slow_dependency",
            outcome: "deadline_exceeded",
            attempts: 1,
            retried: false,
            retry_reason: "deadline_exhausted",
            controlled_time_end_ms: 250,
            health_reason: "dependency_deadline_rejected",
        },
        ScenarioExpectation {
            scenario_id: "crash-support-api",
            kind: "workload_crash",
            outcome: "workload_crashed",
            attempts: 1,
            retried: false,
            retry_reason: "unsafe_operation",
            controlled_time_end_ms: 0,
            health_reason: "sandbox_injected_workload_crash",
        },
        ScenarioExpectation {
            scenario_id: "reject-overload",
            kind: "overload",
            outcome: "overload_rejected",
            attempts: 2,
            retried: true,
            retry_reason: "retry_limit_reached",
            controlled_time_end_ms: 0,
            health_reason: "workload_capacity_gate_rejected",
        },
        ScenarioExpectation {
            scenario_id: "support-api-unavailable",
            kind: "partial_unavailability",
            outcome: "partial_unavailability_observed",
            attempts: 1,
            retried: false,
            retry_reason: "unsafe_operation",
            controlled_time_end_ms: 0,
            health_reason: "target_failed_while_other_service_workloads_remained_ready",
        },
    ] {
        assert_scenario(&root, &expected);
    }

    fs::remove_dir_all(root).unwrap();
}

fn assert_scenario(root: &Path, expected: &ScenarioExpectation) {
    let first = run_scenario(root, expected.scenario_id);
    let second = run_scenario(root, expected.scenario_id);

    assert_eq!(first, second);
    assert_eq!(first["artifactVersion"], "lenso.failure-scenario-result.v1");
    assert_eq!(first["scenarioId"], expected.scenario_id);
    assert_eq!(first["injectedFault"]["kind"], expected.kind);
    assert_eq!(first["affected"]["serviceId"], "support");
    assert_eq!(first["affected"]["workloadId"], "support-api");
    assert_eq!(first["attempts"], expected.attempts);
    assert_eq!(first["retryDecision"]["attempted"], expected.retried);
    assert_eq!(first["retryDecision"]["reason"], expected.retry_reason);
    assert_eq!(first["callPolicyEvidence"]["controlledTimeStartMs"], 0);
    assert_eq!(
        first["callPolicyEvidence"]["controlledTimeEndMs"],
        expected.controlled_time_end_ms
    );
    assert!(first["healthTransitions"].is_array());
    assert_eq!(
        first["healthTransitions"][1]["reason"],
        expected.health_reason
    );
    assert_eq!(first["observedServiceBehavior"]["serviceId"], "support");
    assert!(first["observedServiceBehavior"]["workloads"].is_array());
    assert_eq!(first["outcome"], expected.outcome);
    assert_eq!(first["cleanup"]["completed"], true);
    assert!(first["nextActions"].is_array());

    let evidence_path = root
        .join(".lenso/system-sandbox-results/support-platform")
        .join(expected.scenario_id)
        .join("story-segment.json");
    let evidence: Value = serde_json::from_slice(&fs::read(evidence_path).unwrap()).unwrap();
    assert_eq!(evidence["artifactVersion"], "lenso.story-segment.v1");
    assert_eq!(
        evidence["storyId"],
        format!("sandbox:{}", expected.scenario_id)
    );
    assert_eq!(evidence["result"], first);
    assert!(!root.join(".lenso/system-sandbox/support-platform").exists());
}

fn write_system_fixture(root: &Path) {
    let system = serde_json::json!({
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
    });
    fs::write(
        root.join("lenso.system.json"),
        serde_json::to_vec_pretty(&system).unwrap(),
    )
    .unwrap();
}

fn compile_workload_fixture(root: &Path) -> PathBuf {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sandbox_scenario_workload.rs");
    let binary = root.join("sandbox-scenario-workload");
    let output = Command::new("rustc")
        .args(["--edition=2024", source.to_str().unwrap(), "-o"])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "fixture compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    binary
}

fn write_sandbox_fixture(root: &Path, fixture_binary: &Path) {
    let socket = root.join("support-api.sock");
    let workload = |service: &str, suffix: &str, role: &str| {
        let is_scenario_target = service == "support" && role == "api";
        let scenario_command = if is_scenario_target {
            serde_json::json!([
                fixture_binary.display().to_string(),
                "drive",
                socket.display().to_string()
            ])
        } else {
            serde_json::json!([])
        };
        serde_json::json!({
            "workloadId": format!("{service}-{suffix}"),
            "command": if is_scenario_target {
                serde_json::json!([
                    fixture_binary.display().to_string(),
                    "serve",
                    socket.display().to_string()
                ])
            } else if role == "migration" {
                serde_json::json!(["sh", "-c", "exit 0"])
            } else {
                serde_json::json!(["sh", "-c", "exec sleep 30"])
            },
            "scenarioCommand": scenario_command,
            "endpoint": (role == "api").then(|| format!("http://127.0.0.1/{service}"))
        })
    };
    let scenarios = [
        ("deadline-timeout", "timeout", 100_u64, true),
        ("slow-support", "slow_dependency", 250, true),
        ("crash-support-api", "workload_crash", 0, false),
        ("reject-overload", "overload", 0, true),
        (
            "support-api-unavailable",
            "partial_unavailability",
            0,
            false,
        ),
    ]
    .into_iter()
    .map(|(scenario_id, kind, delay_ms, idempotent)| {
        serde_json::json!({
            "scenarioId": scenario_id,
            "fault": {
                "kind": kind,
                "serviceId": "support",
                "workloadId": "support-api",
                "delayMs": delay_ms,
                "capacity": 1,
                "demand": 2
            },
            "callPolicy": {
                "deadlineMs": 100,
                "maxAttempts": 2,
                "idempotent": idempotent
            }
        })
    })
    .collect::<Vec<_>>();
    let services = ["notifications", "support"]
        .into_iter()
        .map(|service| {
            serde_json::json!({
                "serviceId": service,
                "workloads": [
                    workload(service, "api", "api"),
                    workload(service, "worker", "worker"),
                    workload(service, "migrate", "migration")
                ]
            })
        })
        .collect::<Vec<_>>();
    let sandbox = serde_json::json!({
        "protocol": "lenso.system-sandbox.v1",
        "services": services,
        "scenarios": scenarios
    });
    fs::write(
        root.join("lenso.system-sandbox.json"),
        serde_json::to_vec_pretty(&sandbox).unwrap(),
    )
    .unwrap();
}
