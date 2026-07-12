use std::{fs, path::PathBuf, process::Command, time::SystemTime};

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
