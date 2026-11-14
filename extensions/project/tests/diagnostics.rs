use std::collections::BTreeMap;

use lenso_cli::{Engine, Outcome, Step, Workflow, serde_json};
use lenso_project::{FileChange, Plan};
use serde_json::json;

fn engine(plan: Plan, processes: serde_json::Value) -> Engine {
    let mut engine = Engine::new();
    lenso_project::register(&mut engine).unwrap();
    engine
        .handler("prepare", move |_| {
            let plan = plan.clone();
            async move { Ok(Outcome::complete(json!({"plan":plan}))) }
        })
        .unwrap();
    let mut apply = Step::new("apply", "project/apply");
    apply.needs.push("prepare".into());
    engine
        .workflow(Workflow {
            id: "test".into(),
            steps: vec![Step::new("prepare", "prepare"), apply],
        })
        .unwrap();
    engine
        .grant("test", "project.files", json!(["file.txt"]))
        .unwrap();
    engine
        .grant("test", "host.reads", json!(["input.txt"]))
        .unwrap();
    engine.grant("test", "host.processes", processes).unwrap();
    engine
}

fn plan(root: &std::path::Path) -> Plan {
    Plan::build(
        root,
        vec![FileChange {
            path: "file.txt".into(),
            content: "new".into(),
        }],
    )
    .unwrap()
}

#[tokio::test]
async fn stale_read_dependencies_and_declined_approval_do_not_apply_files() {
    let root = tempfile::tempdir().unwrap();
    let evidence = lenso_runtime::Evidence {
        reads: BTreeMap::from([("input.txt".into(), None)]),
        ..Default::default()
    };
    let prepared = plan(root.path()).with_evidence(evidence).unwrap();
    let engine = engine(prepared, json!([]));
    let mut session = engine
        .session("test", root.path(), json!({}), BTreeMap::new())
        .unwrap();
    let question = session.advance(&engine).await.unwrap().question.unwrap();
    std::fs::write(root.path().join("input.txt"), "changed after review").unwrap();
    session.answer(&question.id, json!(true)).unwrap();
    let value =
        serde_json::to_value(session.advance(&engine).await.unwrap_err().diagnostic()).unwrap();
    assert_eq!(value["code"], "stale_plan");
    assert_eq!(value["details"]["path"], "input.txt");
    assert_eq!(value["recovery"]["kind"], "review_plan");
    assert!(!root.path().join("file.txt").exists());
    let mut declined = engine
        .session("test", root.path(), json!({}), BTreeMap::new())
        .unwrap();
    let question = declined.advance(&engine).await.unwrap().question.unwrap();
    declined.answer(&question.id, json!(false)).unwrap();
    let error = declined.advance(&engine).await.unwrap_err();
    assert_eq!(error.exit_code(), 130);
    assert_eq!(
        error.diagnostic().code(),
        lenso_cli::DiagnosticCode::Cancelled
    );
    assert!(!root.path().join("file.txt").exists());
}

#[tokio::test]
async fn successful_application_has_a_small_receipt_and_missing_permissions_do_not_write() {
    let root = tempfile::tempdir().unwrap();
    let engine = engine(plan(root.path()), json!([]));
    let mut session = engine
        .session("test", root.path(), json!({}), BTreeMap::new())
        .unwrap();
    let question = session.advance(&engine).await.unwrap().question.unwrap();
    session.answer(&question.id, json!(true)).unwrap();
    let report = session.advance(&engine).await.unwrap();
    assert_eq!(report.outputs["apply"]["applied"], json!(["file.txt"]));
    let summary = serde_json::to_value(session.summary()).unwrap();
    let receipt = &summary["calls"].as_array().unwrap().last().unwrap()["receipt"];
    assert_eq!(receipt["affected"], json!(["file.txt"]));
    assert_eq!(receipt["operations"], 0);
    assert_eq!(
        receipt["identifiers"]["plan_id"],
        report.outputs["apply"]["plan_id"]
    );
    assert_eq!(summary["verification_status"], "not_declared");
    let mut denied = engine;
    denied.grant("test", "project.files", json!([])).unwrap();
    let mut session = denied
        .session("test", root.path(), json!({}), BTreeMap::new())
        .unwrap();
    let diagnostic =
        serde_json::to_value(session.advance(&denied).await.unwrap_err().diagnostic()).unwrap();
    assert_eq!(diagnostic["code"], "permission_denied");
    assert_eq!(diagnostic["details"]["capability"], "project.files");
    assert_eq!(
        std::fs::read_to_string(root.path().join("file.txt")).unwrap(),
        "new"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn external_command_failure_reports_actual_files_and_completed_command_indices() {
    let root = tempfile::tempdir().unwrap();
    let commands = vec![
        lenso_runtime::ProcessSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "printf first > command-ran.txt".into()],
        },
        lenso_runtime::ProcessSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "exit 7".into(), "secret-argument".into()],
        },
    ];
    let plan = plan(root.path()).with_commands(commands.clone());
    let engine = engine(plan, serde_json::to_value(commands).unwrap());
    let mut session = engine
        .session("test", root.path(), json!({}), BTreeMap::new())
        .unwrap();
    for _ in 0..2 {
        let question = session.advance(&engine).await.unwrap().question.unwrap();
        session.answer(&question.id, json!(true)).unwrap();
    }
    let error = session.advance(&engine).await.unwrap_err();
    assert_eq!(
        error.exit_code(),
        1,
        "compound application preserves its exit-1 contract"
    );
    let diagnostic = serde_json::to_value(error.diagnostic()).unwrap();
    assert_eq!(diagnostic["code"], "partial_failure");
    assert_eq!(
        diagnostic["details"]["completed_files"],
        json!(["file.txt"])
    );
    assert_eq!(diagnostic["details"]["completed_commands"], json!([0]));
    assert_eq!(diagnostic["details"]["failed_command_index"], 1);
    assert_eq!(diagnostic["details"]["cause"]["code"], "process_failed");
    assert_eq!(diagnostic["details"]["cause"]["details"]["exit_code"], 7);
    assert_eq!(diagnostic["retryable"], false);
    assert!(!diagnostic.to_string().contains("secret-argument"));
    assert_eq!(
        std::fs::read_to_string(root.path().join("file.txt")).unwrap(),
        "new"
    );
    assert_eq!(
        std::fs::read_to_string(root.path().join("command-ran.txt")).unwrap(),
        "first"
    );
}
