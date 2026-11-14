use lenso_cli::{Diagnostic, DiagnosticCode, Engine, Error, Outcome, Step, Workflow, serde_json};
use serde_json::json;

#[test]
fn legacy_errors_never_publish_arbitrary_values_or_exception_text() {
    let forwarded: Error = Error::ProcessExit(7).diagnostic().into();
    assert_eq!(
        forwarded.exit_code(),
        7,
        "public projections preserve the original native exit status"
    );
    let secret =
        "Bearer secret-token https://user:password@example.test/?key=secret-value\nstack backtrace";
    for error in [
        Error::Invalid(secret.into()),
        Error::Operation(secret.into()),
        Error::Permission(secret.into()),
        Error::Io(std::io::Error::other(secret)),
    ] {
        let value = serde_json::to_value(error.diagnostic()).unwrap();
        assert_eq!(value["type"], "diagnostic");
        assert_eq!(value["status"], "failed");
        for private in ["secret-token", "password", "secret-value", "backtrace"] {
            assert!(
                !value.to_string().contains(private),
                "leaked {private}: {value}"
            );
        }
    }
    let value = serde_json::to_value(Diagnostic::missing_capability("host.processes")).unwrap();
    assert_eq!(value["code"], "permission_denied");
    assert_eq!(value["details"]["capability"], "host.processes");
    assert_eq!(value["retryable"], false);
    assert_eq!(value["recovery"]["kind"], "configure_capability");
}

#[tokio::test]
async fn failed_validation_retains_completed_primary_and_checks_not_run() {
    let mut engine = Engine::new();
    engine
        .handler("first", |_| async {
            Ok(Outcome::complete(json!({"private":"secret-token"})))
        })
        .unwrap();
    engine
        .handler("write", |_| async {
            Ok(Outcome::complete(json!({"changed":"file"})))
        })
        .unwrap();
    engine
        .summarizer("write", |_| {
            Some(lenso_cli::StepSummary {
                affected: Some(vec!["file".into()]),
                ..Default::default()
            })
        })
        .unwrap();
    engine
        .handler("check", |_| async {
            Err(Diagnostic::new(DiagnosticCode::OperationFailed)
                .with_retryable(false)
                .into())
        })
        .unwrap();
    engine
        .handler("never", |_| async {
            panic!("validation after failure must not run")
        })
        .unwrap();
    let mut step = Step::new("write", "write");
    step.needs.push("first".into());
    step.hooks.validate = vec!["check".into(), "never".into()];
    engine
        .workflow(Workflow {
            id: "test".into(),
            steps: vec![Step::new("first", "first"), step],
        })
        .unwrap();
    let mut session = engine
        .session(
            "test",
            std::path::Path::new("."),
            json!({"token":"secret-token"}),
            Default::default(),
        )
        .unwrap();
    let error = session.advance(&engine).await.unwrap_err();
    let diagnostic = serde_json::to_value(error.diagnostic()).unwrap();
    assert_eq!(diagnostic["execution"]["completed_steps"], json!(["first"]));
    assert_eq!(diagnostic["execution"]["calls"][1]["status"], "complete");
    assert_eq!(
        diagnostic["execution"]["calls"][1]["receipt"]["affected"],
        json!(["file"])
    );
    assert_eq!(diagnostic["execution"]["verification_status"], "failed");
    assert_eq!(
        diagnostic["execution"]["verification"][1]["status"],
        "not_run"
    );
    assert!(!diagnostic.to_string().contains("secret-token"));
    assert!(session.outputs().contains_key("first"));
    assert_eq!(error.exit_code(), 1);
}

#[tokio::test]
async fn successful_execution_and_validation_are_distinct_and_identifiers_are_per_session() {
    let mut engine = Engine::new();
    engine
        .handler("work", |_| async { Ok(Outcome::complete(json!({}))) })
        .unwrap();
    engine
        .workflow(Workflow {
            id: "plain".into(),
            steps: vec![Step::new("work", "work")],
        })
        .unwrap();
    let mut verified = Step::new("work", "work");
    verified.hooks.validate.push("work".into());
    engine
        .workflow(Workflow {
            id: "verified".into(),
            steps: vec![verified],
        })
        .unwrap();
    let mut plain = engine
        .session(
            "plain",
            std::path::Path::new("."),
            json!({}),
            Default::default(),
        )
        .unwrap();
    let mut checked = engine
        .session(
            "verified",
            std::path::Path::new("."),
            json!({}),
            Default::default(),
        )
        .unwrap();
    assert_eq!(checked.summary().verification_status, "not_run");
    plain.advance(&engine).await.unwrap();
    checked.advance(&engine).await.unwrap();
    assert_eq!(plain.summary().status, "complete");
    assert_eq!(plain.summary().verification_status, "not_declared");
    assert_eq!(checked.summary().verification_status, "passed");
    assert_ne!(plain.summary().id, checked.summary().id);
}

#[tokio::test]
async fn cancellation_and_process_status_survive_execution_wrapping() {
    let mut engine = Engine::new();
    engine
        .handler("cancel", |_| async { Err(Error::Cancelled) })
        .unwrap();
    engine
        .handler("exit", |_| async { Err(Error::ProcessExit(7)) })
        .unwrap();
    for (handler, code, status) in [("cancel", 130, "cancelled"), ("exit", 7, "failed")] {
        engine
            .workflow(Workflow {
                id: handler.into(),
                steps: vec![Step::new("main", handler)],
            })
            .unwrap();
        let mut session = engine
            .session(
                handler,
                std::path::Path::new("."),
                json!({}),
                Default::default(),
            )
            .unwrap();
        let error = session.advance(&engine).await.unwrap_err();
        assert_eq!(error.exit_code(), code);
        assert_eq!(session.summary().status, status);
        assert_eq!(session.summary().calls[0].status, "failed");
    }
}
