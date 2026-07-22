use std::{fs, path::PathBuf, process::Command};

use serde_json::{Value, json};

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("lenso-ga-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_json(root: &std::path::Path, name: &str, value: &Value) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

fn manifest() -> Value {
    json!({
        "protocol": "lenso.ga-support-manifest.v1",
        "manifestId": "ga-support:test",
        "manifestDigest": format!("sha256:{}", "a".repeat(64)),
        "status": "candidate",
        "manifestFormats": [
            {"kind":"system", "version":"lenso.system.v1"},
            {"kind":"system", "version":"lenso.system.v2"}
        ],
        "stateVersions": ["service-store.v1", "service-store.v2"],
        "combinations": [{
            "combinationId": "candidate-1",
            "componentReferences": ["cli:@lenso/cli@0.1.30", "runtime:lenso-service@0.1.4"],
            "stateVersion": "service-store.v2",
            "status": "candidate"
        }],
        "upgradeEdges": [
            {"edgeId":"system-v1-v2", "sourceFormat":"lenso.system.v1", "targetFormat":"lenso.system.v2", "mixedVersionReferences":[], "rollbackSafe":true},
            {"edgeId":"store-v1-v2", "sourceFormat":"service-store.v1", "targetFormat":"service-store.v2", "mixedVersionReferences":["runtime:lenso-service@0.1.4"], "rollbackSafe":false}
        ]
    })
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn support_check_requires_an_exact_declared_combination() {
    let root = temp_root("support");
    let manifest = write_json(&root, "support.json", &manifest());
    let output = run(&[
        "ga",
        "support-check",
        "--manifest",
        manifest.to_str().unwrap(),
        "--component",
        "runtime:lenso-service@0.1.4",
        "--component",
        "cli:@lenso/cli@0.1.30",
        "--state-version",
        "service-store.v2",
        "--json",
    ]);
    assert!(!output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["decision"], "unsupported");
    assert_eq!(report["issues"][0]["code"], "ga_combination_unsupported");

    let unknown = run(&[
        "ga",
        "support-check",
        "--manifest",
        manifest.to_str().unwrap(),
        "--component",
        "runtime:lenso-service@0.1.5",
        "--state-version",
        "service-store.v2",
        "--json",
    ]);
    assert!(!unknown.status.success());
    let report: Value = serde_json::from_slice(&unknown.stdout).unwrap();
    assert_eq!(report["decision"], "unknown");
    assert_eq!(report["issues"][0]["code"], "ga_combination_unknown");
}

#[test]
#[allow(clippy::too_many_lines)]
fn migration_upgrade_retirement_and_failure_commands_emit_stable_plans() {
    let root = temp_root("plans");
    let manifest = write_json(&root, "support.json", &manifest());
    let source = write_json(
        &root,
        "system.json",
        &json!({
            "protocol":"lenso.system.v1", "systemId":"support", "services":[]
        }),
    );
    let migration = run(&[
        "ga",
        "manifest-migrate",
        "--manifest",
        manifest.to_str().unwrap(),
        "--source",
        source.to_str().unwrap(),
        "--target-format",
        "lenso.system.v2",
        "--identity-pointer",
        "/systemId",
        "--dry-run",
        "--json",
    ]);
    assert!(
        migration.status.success(),
        "{}",
        String::from_utf8_lossy(&migration.stderr)
    );
    let migration: Value = serde_json::from_slice(&migration.stdout).unwrap();
    assert_eq!(migration["protocol"], "lenso.manifest-migration-plan.v1");
    assert_eq!(migration["effects"]["mutatesSource"], false);

    let target = root.join("system-v2.json");
    let apply_args = [
        "ga",
        "manifest-migrate",
        "--manifest",
        manifest.to_str().unwrap(),
        "--source",
        source.to_str().unwrap(),
        "--target-format",
        "lenso.system.v2",
        "--identity-pointer",
        "/systemId",
        "--target",
        target.to_str().unwrap(),
        "--json",
    ];
    let first = run(&apply_args);
    assert!(first.status.success());
    let repeated = run(&apply_args);
    assert!(repeated.status.success());
    assert_eq!(first.stdout, repeated.stdout);
    let receipt: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(receipt["protocol"], "lenso.manifest-migration-receipt.v1");

    let upgrade_input = write_json(
        &root,
        "upgrade.json",
        &json!({
            "serviceId":"support", "fromReleaseId":"old", "fromReleaseDigest":format!("sha256:{}", "1".repeat(64)),
            "toReleaseId":"new", "toReleaseDigest":format!("sha256:{}", "2".repeat(64)),
            "configRevisionId":"config-1", "configRevisionDigest":format!("sha256:{}", "3".repeat(64)),
            "sourceStateVersion":"service-store.v1", "targetStateVersion":"service-store.v2",
            "workflowArtifactDigests":[]
        }),
    );
    let upgrade = run(&[
        "ga",
        "service-upgrade",
        "--manifest",
        manifest.to_str().unwrap(),
        "--input",
        upgrade_input.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        upgrade.status.success(),
        "{}",
        String::from_utf8_lossy(&upgrade.stderr)
    );
    let upgrade: Value = serde_json::from_slice(&upgrade.stdout).unwrap();
    assert_eq!(upgrade["steps"][0]["workload"], "migration");
    assert_eq!(upgrade["steps"][1]["workload"], "api");
    assert_eq!(upgrade["steps"][2]["workload"], "worker");
    assert_eq!(upgrade["rollback"]["automaticAllowed"], false);

    let retirement_input = write_json(
        &root,
        "retirement.json",
        &json!({
            "systemGraphDigest":format!("sha256:{}", "4".repeat(64)),
            "environmentEvidenceDigest":format!("sha256:{}", "5".repeat(64)),
            "evidenceFresh":true, "contractId":"support-http", "retiringVersion":"v1",
            "replacementVersion":"v2", "deprecationWindowComplete":true,
            "consumers":[{"consumerId":"console", "activeVersion":"v1", "replacementVerified":false}]
        }),
    );
    let retirement = run(&[
        "ga",
        "contract-retire",
        "--input",
        retirement_input.to_str().unwrap(),
        "--json",
    ]);
    assert!(!retirement.status.success());
    let retirement: Value = serde_json::from_slice(&retirement.stdout).unwrap();
    assert_eq!(retirement["decision"], "unsupported");
    assert_eq!(retirement["effects"]["retiresContract"], false);

    let failure_input = write_json(
        &root,
        "failure.json",
        &json!({
            "scenarioId":"system-plane-outage", "condition":"system_plane_unavailable",
            "expected":"pause_coordinated_mutation", "observations":[{
                "subject":"promotion", "outcome":"pause_coordinated_mutation",
                "evidenceDigest":format!("sha256:{}", "6".repeat(64))
            }], "effects":[], "cleanupComplete":true
        }),
    );
    let failure = run(&[
        "ga",
        "failure-evaluate",
        "--input",
        failure_input.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        failure.status.success(),
        "{}",
        String::from_utf8_lossy(&failure.stderr)
    );
    let failure: Value = serde_json::from_slice(&failure.stdout).unwrap();
    assert_eq!(failure["protocol"], "lenso.failure-scenario-evidence.v1");
    assert_eq!(failure["decision"], "supported");
}
