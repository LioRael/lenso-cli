use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extraction/support-ticket")
}

fn command_arguments(profile: &str, evidence: bool, json: bool) -> Vec<String> {
    let root = fixture_root();
    let profile_root = root.join(profile);
    let mut arguments = vec![
        "module".to_owned(),
        "extraction".to_owned(),
        "readiness".to_owned(),
        "support-ticket".to_owned(),
        "--module-manifest".to_owned(),
        root.join("lenso.module.json").display().to_string(),
        "--system-file".to_owned(),
        root.join("lenso.system.json").display().to_string(),
        "--repo-root".to_owned(),
        profile_root.display().to_string(),
    ];
    if evidence {
        arguments.extend(["--evidence-file".to_owned(), "evidence.json".to_owned()]);
    }
    if json {
        arguments.push("--json".to_owned());
    }
    arguments
}

fn run(profile: &str, evidence: bool, json: bool) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(command_arguments(profile, evidence, json))
        .output()
        .unwrap()
}

fn parse_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not JSON: {error}; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn directory_snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    fn collect(root: &Path, current: &Path, snapshot: &mut Vec<(String, Vec<u8>)>) {
        let mut paths = fs::read_dir(current)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                collect(root, &path, snapshot);
            } else {
                snapshot.push((
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                    fs::read(path).unwrap(),
                ));
            }
        }
    }

    let mut snapshot = Vec::new();
    collect(root, root, &mut snapshot);
    snapshot
}

fn issue_codes(report: &Value) -> Vec<&str> {
    report["issueCodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|code| code.as_str().unwrap())
        .collect()
}

#[test]
fn public_command_reports_blocked_and_corrected_support_ticket_deterministically() {
    let root = fixture_root();
    let before = directory_snapshot(&root);

    let blocked_output = run("blocked", true, true);
    assert!(!blocked_output.status.success());
    let blocked = parse_json(&blocked_output);
    assert_eq!(blocked["protocol"], "lenso.extraction-readiness-report.v1");
    assert_eq!(blocked["classification"], "blocked");
    assert_eq!(blocked["ready"], false);
    let blocked_codes = issue_codes(&blocked);
    for code in [
        "active_consumer_breaking",
        "cross_module_import",
        "in_process_boundary_call",
        "required_event_contract_missing",
        "required_service_contract_missing",
    ] {
        assert!(blocked_codes.contains(&code), "missing {code}: {blocked}");
    }
    assert_eq!(
        blocked["effects"],
        serde_json::json!({
            "writesRepositoryFiles": false,
            "startsWorkloads": false,
            "movesData": false,
            "changesAuthority": false,
        })
    );

    let repeated_blocked = parse_json(&run("blocked", true, true));
    assert_eq!(blocked, repeated_blocked);

    let corrected_output = run("corrected", true, true);
    assert!(
        corrected_output.status.success(),
        "corrected command failed: {}",
        String::from_utf8_lossy(&corrected_output.stderr)
    );
    let corrected = parse_json(&corrected_output);
    assert_eq!(corrected["classification"], "needs_attention");
    assert_eq!(corrected["ready"], true);
    let corrected_codes = issue_codes(&corrected);
    assert!(!corrected_codes.contains(&"cross_module_import"));
    assert!(!corrected_codes.contains(&"in_process_boundary_call"));
    assert!(!corrected_codes.contains(&"required_event_contract_missing"));
    assert!(!corrected_codes.contains(&"required_service_contract_missing"));
    assert_eq!(corrected, parse_json(&run("corrected", true, true)));

    assert_eq!(before, directory_snapshot(&root));
}

#[test]
fn public_command_human_output_projects_the_same_readiness_decision() {
    let blocked = run("blocked", true, false);
    assert!(!blocked.status.success());
    let blocked = String::from_utf8(blocked.stdout).unwrap();
    assert!(blocked.contains("Extraction readiness: support-ticket"));
    assert!(blocked.contains("Result: blocked (not ready)"));
    assert!(blocked.contains("cross_module_import"));
    assert!(blocked.contains("Effects: read-only"));

    let corrected = run("corrected", true, false);
    assert!(corrected.status.success());
    let corrected = String::from_utf8(corrected.stdout).unwrap();
    assert!(corrected.contains("Result: needs_attention (ready)"));
    assert!(corrected.contains("- workflows: ticket_triage@v1"));
}

#[test]
fn omitted_structured_evidence_fails_closed_without_mutation() {
    let root = fixture_root();
    let before = directory_snapshot(&root);
    let output = run("corrected", false, true);
    assert!(!output.status.success());
    let report = parse_json(&output);
    assert_eq!(report["classification"], "blocked");
    let codes = issue_codes(&report);
    assert!(codes.contains(&"contract_evidence_missing"));
    assert!(codes.contains(&"active_consumer_compatibility_missing"));
    assert_eq!(before, directory_snapshot(&root));
}
