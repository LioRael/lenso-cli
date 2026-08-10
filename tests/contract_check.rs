#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime},
};

use lenso_service::{
    LEGACY_SERVICE_V1_FIXTURE_JSON, LEGACY_SYSTEM_V1_FIXTURE_JSON, MIXED_SYSTEM_V2_FIXTURE_JSON,
};
use serde_json::Value;

fn fixture_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "lenso-cli-{name}-{}-{nonce}.json",
        std::process::id()
    ))
}

fn fixture_dir(name: &str) -> PathBuf {
    fixture_path(name).with_extension("")
}

fn run_json(arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_json_failure(arguments: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(arguments)
        .output()
        .unwrap();
    assert!(!output.status.success(), "command unexpectedly succeeded");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not JSON: {error}; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn run_owned(arguments: &[String]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(arguments)
        .output()
        .unwrap()
}

async fn observe_local_adapter(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    workload: &Value,
) -> Value {
    let response = client
        .post(format!("{endpoint}/workload-control/v1/observe"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "protocol": "lenso.workload-control.v1",
            "workload": workload.clone()
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    response.json().await.unwrap()
}

async fn mutate_local_adapter(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    workload: &Value,
    revision: &str,
    key: &str,
    action: &str,
) -> Value {
    let response = client
        .post(format!("{endpoint}/workload-control/v1/operations"))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "protocol": "lenso.workload-control.v1",
            "workload": workload,
            "action": { "kind": action },
            "observedRevision": revision,
            "idempotencyKey": key,
            "actor": { "kind": "operator", "subject": "operator:integration-test" }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::ACCEPTED);
    response.json().await.unwrap()
}

async fn wait_for_local_adapter_operation(
    client: &reqwest::Client,
    endpoint: &str,
    token: &str,
    operation_id: &str,
) -> Value {
    for _ in 0..200 {
        let response = client
            .get(format!(
                "{endpoint}/workload-control/v1/operations/{operation_id}"
            ))
            .bearer_auth(token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let record: Value = response.json().await.unwrap();
        if matches!(record["phase"].as_str(), Some("succeeded" | "failed")) {
            return record;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("local adapter operation did not finish")
}

fn write_contract_variant(name: &str, fixture: &str, protocol: Option<&str>) -> PathBuf {
    let path = fixture_path(name);
    let mut artifact: Value = serde_json::from_str(fixture).unwrap();
    match protocol {
        Some(protocol) => artifact["protocol"] = Value::String(protocol.to_owned()),
        None => {
            artifact.as_object_mut().unwrap().remove("protocol");
        }
    }
    fs::write(&path, serde_json::to_vec_pretty(&artifact).unwrap()).unwrap();
    path
}

#[test]
fn service_check_reports_shared_provider_semantics() {
    let path = fixture_path("service-provider");
    fs::write(&path, LEGACY_SERVICE_V1_FIXTURE_JSON).unwrap();

    let report = run_json(&["service", "check", path.to_str().unwrap(), "--json"]);

    assert_eq!(report["detectedProtocol"], "lenso.service.v1");
    assert_eq!(report["artifactVersion"], "lenso.service-check.v1");
    assert_eq!(report["semanticKind"], "provider");
    assert_eq!(report["providerSemantics"]["authOwner"], "host");
    fs::remove_file(path).unwrap();
}

#[test]
fn system_check_reports_shared_provider_system_semantics() {
    let path = fixture_path("system-provider");
    fs::write(&path, LEGACY_SYSTEM_V1_FIXTURE_JSON).unwrap();

    let report = run_json(&[
        "system",
        "check",
        "--system-file",
        path.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(report["detectedProtocol"], "lenso.system.v1");
    assert_eq!(report["artifactVersion"], "lenso.system-plan.v1");
    assert_eq!(report["semanticKind"], "provider_system");
    assert_eq!(report["providerSemantics"]["runtimeQueueOwner"], "host");
    fs::remove_file(path).unwrap();
}

#[test]
fn system_v2_check_and_graph_report_explicit_mixed_topology_kinds() {
    let path = fixture_path("mixed-system-v2");
    fs::write(&path, MIXED_SYSTEM_V2_FIXTURE_JSON).unwrap();

    let check = run_json(&[
        "system",
        "check",
        "--system-file",
        path.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(check["detectedProtocol"], "lenso.system.v2");
    assert_eq!(check["semanticKind"], "mixed_system");
    assert_eq!(
        check["kinds"],
        serde_json::json!([
            "autonomous_service",
            "consumer",
            "host",
            "module",
            "producer",
            "provider",
            "workload"
        ])
    );

    let graph = run_json(&[
        "system",
        "graph",
        "--system-file",
        path.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(graph["artifactProtocol"], "lenso.system.v2");
    assert_eq!(graph["artifactVersion"], "lenso.system-graph.v1");
    let kinds = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| node["kind"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        kinds,
        [
            "autonomous_service",
            "consumer",
            "host",
            "module",
            "producer",
            "provider",
            "workload",
        ]
        .into_iter()
        .collect()
    );
    fs::remove_file(path).unwrap();
}

#[test]
fn system_machine_workflows_are_versioned_and_dry_run_does_not_mutate_state() {
    let system_path = fixture_path("agent-safe-system");
    let repo_root = fixture_dir("agent-safe-repo");
    fs::write(&system_path, LEGACY_SYSTEM_V1_FIXTURE_JSON).unwrap();
    fs::create_dir_all(&repo_root).unwrap();

    let first_plan = run_json(&[
        "system",
        "plan",
        "--system-file",
        system_path.to_str().unwrap(),
        "--json",
    ]);
    let mut reordered: Value = serde_json::from_str(LEGACY_SYSTEM_V1_FIXTURE_JSON).unwrap();
    for field in ["environments", "services", "modules", "dependencies"] {
        if let Some(values) = reordered.get_mut(field).and_then(Value::as_array_mut) {
            values.reverse();
        }
    }
    if let Some(services) = reordered.get_mut("services").and_then(Value::as_array_mut) {
        for service in services {
            if let Some(modules) = service.get_mut("modules").and_then(Value::as_array_mut) {
                modules.reverse();
            }
        }
    }
    fs::write(&system_path, serde_json::to_vec_pretty(&reordered).unwrap()).unwrap();
    let reordered_plan = run_json(&[
        "system",
        "plan",
        "--system-file",
        system_path.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(first_plan, reordered_plan);

    let graph = run_json(&[
        "system",
        "graph",
        "--system-file",
        system_path.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(graph["artifactVersion"], "lenso.system-graph.v1");

    for command in ["diff", "doctor"] {
        let report = run_json(&[
            "system",
            command,
            "--system-file",
            system_path.to_str().unwrap(),
            "--repo-root",
            repo_root.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(report["artifactVersion"], "lenso.system-drift.v1");
        assert!(report["nextActions"].is_array());
    }

    let before = fs::read_dir(&repo_root).unwrap().count();
    let preview_args = [
        "system",
        "apply",
        "--system-file",
        system_path.to_str().unwrap(),
        "--repo-root",
        repo_root.to_str().unwrap(),
        "--dry-run",
        "--json",
    ];
    let preview = run_json(&preview_args);
    let repeated_preview = run_json(&preview_args);
    let after = fs::read_dir(&repo_root).unwrap().count();
    assert_eq!(preview["artifactVersion"], "lenso.system-drift.v1");
    assert_eq!(preview["approvalBoundaries"][0]["executed"], false);
    assert_eq!(
        preview, repeated_preview,
        "dry-run proposal was not deterministic"
    );
    assert_eq!(before, after, "dry-run created local state");
    assert!(!repo_root.join(".lenso").exists());

    fs::remove_file(system_path).unwrap();
    fs::remove_dir_all(repo_root).unwrap();
}

#[test]
fn service_doctor_machine_output_is_versioned_and_actionable() {
    let repo_root = fixture_dir("agent-safe-service-doctor");
    fs::create_dir_all(&repo_root).unwrap();

    let report = run_json(&[
        "service",
        "doctor",
        "--repo-root",
        repo_root.to_str().unwrap(),
        "--json",
    ]);

    assert_eq!(report["artifactVersion"], "lenso.service-doctor.v1");
    assert!(report["nextActions"].is_array());
    fs::remove_dir_all(repo_root).unwrap();
}

#[test]
fn service_diff_and_upgrade_plan_share_one_versioned_deterministic_proposal() {
    let repo_root = fixture_dir("agent-safe-service-diff");
    let candidate_path = fixture_path("agent-safe-service-candidate");
    fs::create_dir_all(repo_root.join(".lenso")).unwrap();
    let current: Value = serde_json::from_str(LEGACY_SERVICE_V1_FIXTURE_JSON).unwrap();
    let service_name = current["name"].as_str().unwrap();
    fs::write(
        repo_root.join(".lenso/module-installs.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "modules": [{
                "moduleName": service_name,
                "source": "service",
                "serviceManifestSnapshot": current,
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(&candidate_path, LEGACY_SERVICE_V1_FIXTURE_JSON).unwrap();

    let arguments = [
        service_name,
        candidate_path.to_str().unwrap(),
        "--repo-root",
        repo_root.to_str().unwrap(),
        "--json",
    ];
    let diff = run_json(&[&["service", "diff"], &arguments[..]].concat());
    let plan = run_json(&[&["service", "upgrade-plan"], &arguments[..]].concat());
    let ledger_path = repo_root.join(".lenso/module-installs.json");
    let ledger_before = fs::read(&ledger_path).unwrap();
    let dry_run = run_json(
        &[
            &["service", "upgrade"],
            &arguments[..arguments.len() - 1],
            &["--dry-run", "--json"],
        ]
        .concat(),
    );

    assert_eq!(diff["artifactVersion"], "lenso.service-diff.v1");
    assert_eq!(diff["approvalBoundaries"][0]["executed"], false);
    assert_eq!(diff, plan);
    assert_eq!(diff, dry_run);
    assert_eq!(ledger_before, fs::read(ledger_path).unwrap());

    fs::remove_file(candidate_path).unwrap();
    fs::remove_dir_all(repo_root).unwrap();
}

#[test]
fn service_check_reports_stable_protocol_errors_as_json() {
    for (name, protocol, code) in [
        ("service-ambiguous", None, "ambiguous_protocol"),
        (
            "service-unsupported",
            Some("lenso.service.v99"),
            "unsupported_protocol",
        ),
    ] {
        let path = write_contract_variant(name, LEGACY_SERVICE_V1_FIXTURE_JSON, protocol);
        let error = run_json_failure(&["service", "check", path.to_str().unwrap(), "--json"]);
        assert_eq!(error["code"], code);
        assert!(
            error["nextAction"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn system_check_reports_stable_protocol_errors_as_json() {
    for (name, protocol, code) in [
        ("system-ambiguous", None, "ambiguous_protocol"),
        (
            "system-unsupported",
            Some("lenso.system.v99"),
            "unsupported_protocol",
        ),
    ] {
        let path = write_contract_variant(name, LEGACY_SYSTEM_V1_FIXTURE_JSON, protocol);
        let error = run_json_failure(&[
            "system",
            "check",
            "--system-file",
            path.to_str().unwrap(),
            "--json",
        ]);
        assert_eq!(error["code"], code);
        assert!(
            error["nextAction"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn exact_support_desk_composition_is_revision_guarded_and_locally_runnable() {
    let root = fixture_dir("exact-support-desk");
    let app = root.join("support-desk");
    fs::create_dir_all(&root).unwrap();

    let compose = vec![
        "app".to_owned(),
        "compose".to_owned(),
        app.to_string_lossy().into_owned(),
        "--apply".to_owned(),
    ];
    let created = run_owned(&compose);
    assert!(
        created.status.success(),
        "app compose failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let composition_path = app.join("lenso.app.json");
    let first_source = fs::read(&composition_path).unwrap();
    let first: Value = serde_json::from_slice(&first_source).unwrap();
    assert_eq!(first["protocol"], "lenso.app-composition.v1");
    assert_eq!(first["revision"], 1);
    assert!(
        first["contentDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(first["modules"].as_array().unwrap().len(), 3);
    assert_eq!(
        first["modules"][0]["release"]["contentDigest"]
            .as_str()
            .unwrap()
            .len(),
        71
    );
    assert_eq!(first["modules"][1]["dependencies"][0]["version"], "0.1.0");
    assert_eq!(first["modules"][2]["implementation"]["kind"], "service");
    assert!(
        first["modules"][2]["implementation"]["serviceReference"]
            .as_str()
            .unwrap()
            .starts_with("service:")
    );
    assert!(!app.join(".lenso.app.json.lock").exists());

    let stale = vec![
        "app".to_owned(),
        "compose".to_owned(),
        "--repo-root".to_owned(),
        app.to_string_lossy().into_owned(),
        "--addon".to_owned(),
        "support-sla".to_owned(),
        "--apply".to_owned(),
        "--observed-revision".to_owned(),
        "0".to_owned(),
    ];
    let conflict = run_owned(&stale);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("revision conflict"));
    assert_eq!(first_source, fs::read(&composition_path).unwrap());

    let plan = vec![
        "system".to_owned(),
        "dev".to_owned(),
        "--system-file".to_owned(),
        composition_path.to_string_lossy().into_owned(),
        "--dry-run".to_owned(),
        "--json".to_owned(),
    ];
    let plan_output = run_owned(&plan);
    assert!(
        plan_output.status.success(),
        "system dev dry-run failed: {}",
        String::from_utf8_lossy(&plan_output.stderr)
    );
    let adapter_plan: Value = serde_json::from_slice(&plan_output.stdout).unwrap();
    assert_eq!(
        adapter_plan["protocol"],
        "lenso.local-control-adapter-plan.v1"
    );
    assert_eq!(adapter_plan["compositionDigest"], first["contentDigest"]);
    assert!(
        adapter_plan["workloads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|workload| workload["identity"] == "local-dev://support-desk/support-api/api")
    );
    assert!(!app.join("lenso.system.json").exists());
    assert!(!app.join("lenso.system-sandbox.json").exists());

    let addon = vec![
        "app".to_owned(),
        "compose".to_owned(),
        "--repo-root".to_owned(),
        app.to_string_lossy().into_owned(),
        "--addon".to_owned(),
        "support-sla".to_owned(),
        "--apply".to_owned(),
        "--observed-revision".to_owned(),
        "1".to_owned(),
    ];
    let added = run_owned(&addon);
    assert!(
        added.status.success(),
        "adding addon failed: {}",
        String::from_utf8_lossy(&added.stderr)
    );
    let second: Value = serde_json::from_slice(&fs::read(&composition_path).unwrap()).unwrap();
    assert_eq!(second["revision"], 2);
    assert!(
        second["provenance"]["addons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|addon| addon == "support-sla")
    );

    let linked = vec![
        "app".to_owned(),
        "compose".to_owned(),
        "--repo-root".to_owned(),
        app.to_string_lossy().into_owned(),
        "--implementation".to_owned(),
        "support-api=linked".to_owned(),
        "--apply".to_owned(),
        "--observed-revision".to_owned(),
        "2".to_owned(),
    ];
    let rebound = run_owned(&linked);
    assert!(
        rebound.status.success(),
        "implementation update failed: {}",
        String::from_utf8_lossy(&rebound.stderr)
    );
    let third: Value = serde_json::from_slice(&fs::read(&composition_path).unwrap()).unwrap();
    assert_eq!(third["revision"], 3);
    assert_eq!(
        third["modules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|module| module["moduleId"] == "support-api")
            .unwrap()["implementation"]["kind"],
        "linked"
    );

    let before_noop = fs::read(&composition_path).unwrap();
    let noop = run_owned(&[
        "app".to_owned(),
        "compose".to_owned(),
        "--repo-root".to_owned(),
        app.to_string_lossy().into_owned(),
        "--implementation".to_owned(),
        "support-api=linked".to_owned(),
        "--apply".to_owned(),
        "--observed-revision".to_owned(),
        "3".to_owned(),
    ]);
    assert!(
        noop.status.success(),
        "idempotent composition update failed: {}",
        String::from_utf8_lossy(&noop.stderr)
    );
    assert_eq!(before_noop, fs::read(&composition_path).unwrap());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn local_control_adapter_keeps_workloads_after_coordinator_exits() {
    let root = fixture_dir("local-control-adapter");
    let app = root.join("support-desk");
    fs::create_dir_all(&root).unwrap();
    let compose = vec![
        "app".to_owned(),
        "compose".to_owned(),
        app.to_string_lossy().into_owned(),
        "--apply".to_owned(),
    ];
    let created = run_owned(&compose);
    assert!(created.status.success());

    let workspace_path = app.join("lenso.workspace.json");
    let leak_marker = root.join("workload-received-control-token");
    let workload_script = root.join("workload.sh");
    fs::write(
        &workload_script,
        format!(
            "if [ -n \"${{LENSO_WORKLOAD_CONTROL_TOKEN:-}}\" ]; then : > '{}'; fi\nexec sleep 30\n",
            leak_marker.display()
        ),
    )
    .unwrap();
    let mut workspace: Value = serde_json::from_slice(&fs::read(&workspace_path).unwrap()).unwrap();
    for service in workspace["services"].as_array_mut().unwrap() {
        service["cwd"] = Value::String(".".to_owned());
        service["command"] = Value::String(format!("sh {}", workload_script.display()));
        service["readyUrl"] = Value::String(String::new());
    }
    fs::write(
        &workspace_path,
        serde_json::to_vec_pretty(&workspace).unwrap(),
    )
    .unwrap();

    let start = vec![
        "system".to_owned(),
        "dev".to_owned(),
        "--system-file".to_owned(),
        app.join("lenso.app.json").to_string_lossy().into_owned(),
        "--json".to_owned(),
    ];
    let started = run_owned(&start);
    assert!(
        started.status.success(),
        "adapter start failed: {}",
        String::from_utf8_lossy(&started.stderr)
    );
    let state: Value = serde_json::from_slice(&started.stdout).unwrap();
    assert!(state.get("adapterOwnershipToken").is_none());
    let adapter_state_path = app.join(".lenso/local-control-adapter/support-desk/state.json");
    let persisted_ready_state: Value =
        serde_json::from_slice(&fs::read(&adapter_state_path).unwrap()).unwrap();
    let adapter_ownership_token = persisted_ready_state["adapterOwnershipToken"]
        .as_str()
        .unwrap();
    assert_eq!(adapter_ownership_token.len(), 64);
    assert!(
        adapter_ownership_token
            .bytes()
            .all(|byte| { byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) })
    );
    assert_eq!(state["protocol"], "lenso.local-control-adapter.v1");
    assert_eq!(state["phase"], "ready");
    assert!(state["adapterPid"].as_u64().unwrap() > 0);
    assert_eq!(state["workloadIdentities"].as_array().unwrap().len(), 2);
    assert_eq!(state["adapterId"], "workload-control:support-desk");
    assert_eq!(
        state["adapterWorkload"],
        serde_json::json!({
            "systemId": "support-desk",
            "serviceId": "lenso-local-control-adapter",
            "workloadId": "workload-control:support-desk"
        })
    );
    assert_eq!(
        state["workloadControlProtocol"],
        "lenso.workload-control.v1"
    );
    assert_eq!(
        state["workloadControlSchemaDigest"],
        "sha256:d3666bb1fd85576f9af4205dbcc70029acd81462678c47d2b315c40ef1a9161d"
    );
    assert_eq!(
        state["capabilities"],
        serde_json::json!(["suspend", "resume"])
    );
    let credential_file = PathBuf::from(state["credentialFile"].as_str().unwrap());
    let control_token = fs::read_to_string(&credential_file).unwrap();
    #[cfg(unix)]
    assert_eq!(
        fs::metadata(&credential_file).unwrap().permissions().mode() & 0o077,
        0
    );
    assert!(
        !serde_json::to_string(&state)
            .unwrap()
            .contains(&control_token)
    );

    let endpoint = state["endpoint"].as_str().unwrap();
    assert!(endpoint.starts_with("http://127.0.0.1:"));
    let identity = state["workloadIdentities"][0].as_str().unwrap();
    let service_id = identity
        .strip_prefix("local-dev://support-desk/")
        .and_then(|identity| identity.strip_suffix("/api"))
        .unwrap();
    let workload = serde_json::json!({
        "systemId": "support-desk",
        "serviceId": service_id,
        "workloadId": format!("{service_id}-api")
    });
    let client = reqwest::Client::new();
    let unauthorized = client
        .post(format!("{endpoint}/workload-control/v1/observe"))
        .json(&serde_json::json!({
            "protocol": "lenso.workload-control.v1",
            "workload": workload.clone()
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let running = observe_local_adapter(&client, endpoint, &control_token, &workload).await;
    assert_eq!(running["state"], "running");
    let suspend = mutate_local_adapter(
        &client,
        endpoint,
        &control_token,
        &workload,
        running["observedRevision"].as_str().unwrap(),
        "integration-suspend",
        "suspend",
    )
    .await;
    let suspended = wait_for_local_adapter_operation(
        &client,
        endpoint,
        &control_token,
        suspend["operationId"].as_str().unwrap(),
    )
    .await;
    assert_eq!(suspended["phase"], "succeeded");
    assert_eq!(suspended["result"]["state"], "suspended");

    let suspended_observation =
        observe_local_adapter(&client, endpoint, &control_token, &workload).await;
    let resume = mutate_local_adapter(
        &client,
        endpoint,
        &control_token,
        &workload,
        suspended_observation["observedRevision"].as_str().unwrap(),
        "integration-resume",
        "resume",
    )
    .await;
    let resumed = wait_for_local_adapter_operation(
        &client,
        endpoint,
        &control_token,
        resume["operationId"].as_str().unwrap(),
    )
    .await;
    assert_eq!(resumed["phase"], "succeeded");
    assert_eq!(resumed["result"]["state"], "running");

    let cleanup = vec![
        "system".to_owned(),
        "dev".to_owned(),
        "--system-file".to_owned(),
        app.join("lenso.app.json").to_string_lossy().into_owned(),
        "--cleanup".to_owned(),
        "--json".to_owned(),
    ];
    let stopped = run_owned(&cleanup);
    assert!(
        stopped.status.success(),
        "adapter cleanup failed: {}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    let stopped_state: Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(stopped_state["phase"], "stopped");
    let adapter_state: Value =
        serde_json::from_slice(&fs::read(&adapter_state_path).unwrap()).unwrap();
    assert_eq!(adapter_state["phase"], "stopped");
    assert!(!app.join(".lenso/system-sandbox/support-desk").exists());

    let restarted = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(&start)
        .env(
            "LENSO_WORKLOAD_CONTROL_TOKEN",
            "must-not-reach-managed-workloads",
        )
        .output()
        .unwrap();
    assert!(
        restarted.status.success(),
        "adapter restart with token override failed: {}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    let restarted_state: Value = serde_json::from_slice(&restarted.stdout).unwrap();
    assert_eq!(restarted_state["phase"], "ready");
    assert!(restarted_state.get("credentialFile").is_none());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !leak_marker.exists(),
        "server-side bearer token reached a managed workload"
    );

    let stopped_again = run_owned(&cleanup);
    assert!(
        stopped_again.status.success(),
        "adapter cleanup after token isolation check failed: {}",
        String::from_utf8_lossy(&stopped_again.stderr)
    );
    assert!(!app.join(".lenso/system-sandbox/support-desk").exists());

    fs::remove_dir_all(root).unwrap();
}
