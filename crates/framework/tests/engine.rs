use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use lenso_cli::serde_json::{Value, json};
use lenso_cli::{Engine, Error, Outcome, Question, QuestionKind, Status, Step, Workflow};

#[tokio::test]
async fn answering_and_revising_invalidates_only_dependent_steps() -> Result<(), Error> {
    let mut engine = Engine::new();
    let unrelated_calls = Arc::new(AtomicUsize::new(0));
    let calls = unrelated_calls.clone();
    engine.handler("independent", move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(Outcome::complete(json!("kept"))) }
    })?;
    engine.handler("ask", |ctx| async move {
        match ctx.answers.get("name") {
            Some(value) => Ok(Outcome::complete(value.clone())),
            None => Ok(Outcome::NeedsInput {
                question: Question {
                    id: "name".into(),
                    message: "Name?".into(),
                    kind: QuestionKind::Text,
                },
            }),
        }
    })?;
    engine.handler("render", |ctx| async move {
        Ok(Outcome::complete(json!({"name":ctx.steps["ask"]})))
    })?;
    let mut render = Step::new("render", "render");
    render.needs.push("ask".into());
    engine.workflow(Workflow {
        id: "test".into(),
        steps: vec![
            Step::new("independent", "independent"),
            render,
            Step::new("ask", "ask"),
        ],
    })?;
    let mut session = engine.session(
        "test",
        std::path::Path::new("."),
        json!({}),
        BTreeMap::new(),
    )?;
    assert_eq!(session.advance(&engine).await?.status, Status::NeedsInput);
    assert!(
        session.answer("name", Value::Bool(true)).is_err(),
        "text questions must reject boolean answers"
    );
    session.answer("name", json!("Rust"))?;
    assert_eq!(
        session.advance(&engine).await?.outputs["render"]["name"],
        "Rust"
    );
    session.answer("name", json!("Wasm"))?;
    assert_eq!(
        session.advance(&engine).await?.outputs["render"]["name"],
        "Wasm"
    );
    assert_eq!(
        unrelated_calls.load(Ordering::SeqCst),
        1,
        "unrelated completed work must not replay"
    );
    Ok(())
}

#[tokio::test]
async fn consecutive_questions_in_one_step_preserve_earlier_answers() -> Result<(), Error> {
    let mut engine = Engine::new();
    engine.handler("confirm", |ctx| async move {
        for id in ["files", "commands"] {
            if !ctx.answers.contains_key(id) {
                return Ok(Outcome::NeedsInput {
                    question: Question {
                        id: id.into(),
                        message: format!("Approve {id}?"),
                        kind: QuestionKind::Confirm,
                    },
                });
            }
        }
        Ok(Outcome::complete(json!("approved")))
    })?;
    engine.workflow(Workflow {
        id: "confirmations".into(),
        steps: vec![Step::new("confirm", "confirm")],
    })?;
    let mut session = engine.session(
        "confirmations",
        std::path::Path::new("."),
        json!({}),
        BTreeMap::new(),
    )?;
    assert_eq!(
        session.advance(&engine).await?.question.unwrap().id,
        "files"
    );
    session.answer("files", json!(true))?;
    assert_eq!(
        session.advance(&engine).await?.question.unwrap().id,
        "commands"
    );
    session.answer("commands", json!(true))?;
    assert_eq!(
        session.advance(&engine).await?.status,
        Status::Complete,
        "second answer must preserve first approval"
    );
    session.answer("files", json!(false))?;
    assert_eq!(
        session.advance(&engine).await?.question.unwrap().id,
        "commands",
        "revising earlier consent must discard later consent"
    );
    Ok(())
}

#[tokio::test]
async fn hooks_receive_primary_output_and_failures_stop_execution() -> Result<(), Error> {
    let mut engine = Engine::new();
    engine.handler("work", |_| async {
        Ok(Outcome::complete(json!({"ok":true})))
    })?;
    engine.handler("check", |ctx| async move {
        assert_eq!(ctx.hooks["result"]["ok"], true);
        Err(Error::Operation("policy rejected".into()))
    })?;
    let mut step = Step::new("work", "work");
    step.hooks.validate.push("check".into());
    engine.workflow(Workflow {
        id: "test".into(),
        steps: vec![step],
    })?;
    let mut session = engine.session(
        "test",
        std::path::Path::new("."),
        json!({}),
        BTreeMap::new(),
    )?;
    assert!(
        session
            .advance(&engine)
            .await
            .unwrap_err()
            .to_string()
            .contains("policy rejected")
    );
    Ok(())
}

#[test]
fn graph_registration_rejects_cycles_unknown_handlers_and_duplicate_ids() -> Result<(), Error> {
    let mut engine = Engine::new();
    engine.handler("work", |_| async { Ok(Outcome::complete(Value::Null)) })?;
    let mut a = Step::new("a", "work");
    a.needs.push("b".into());
    let mut b = Step::new("b", "work");
    b.needs.push("a".into());
    assert!(
        engine
            .workflow(Workflow {
                id: "cycle".into(),
                steps: vec![a, b]
            })
            .is_err()
    );
    assert!(
        engine
            .workflow(Workflow {
                id: "unknown".into(),
                steps: vec![Step::new("a", "missing")]
            })
            .is_err()
    );
    assert!(
        engine
            .workflow(Workflow {
                id: "duplicate".into(),
                steps: vec![Step::new("a", "work"), Step::new("a", "work")]
            })
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn changing_upstream_answers_discards_downstream_consent() -> Result<(), Error> {
    let mut engine = Engine::new();
    engine.handler("ask", |ctx| async move {
        match ctx.answers.get("name") {
            Some(value) => Ok(Outcome::complete(value.clone())),
            None => Ok(Outcome::NeedsInput {
                question: Question {
                    id: "name".into(),
                    message: "Name?".into(),
                    kind: QuestionKind::Text,
                },
            }),
        }
    })?;
    engine.handler("approve", |ctx| async move {
        match ctx.answers.get("approval") {
            Some(Value::Bool(true)) => Ok(Outcome::complete(ctx.steps["ask"].clone())),
            _ => Ok(Outcome::NeedsInput {
                question: Question {
                    id: "approval".into(),
                    message: "Approve?".into(),
                    kind: QuestionKind::Confirm,
                },
            }),
        }
    })?;
    let mut approve = Step::new("approve", "approve");
    approve.needs.push("ask".into());
    engine.workflow(Workflow {
        id: "consent".into(),
        steps: vec![Step::new("ask", "ask"), approve],
    })?;
    let mut session = engine.session(
        "consent",
        std::path::Path::new("."),
        json!({}),
        BTreeMap::new(),
    )?;
    session.advance(&engine).await?;
    session.answer("name", json!("Rust"))?;
    session.advance(&engine).await?;
    session.answer("approval", json!(true))?;
    assert_eq!(session.advance(&engine).await?.status, Status::Complete);
    session.answer("name", json!("Wasm"))?;
    let report = session.advance(&engine).await?;
    assert_eq!(report.status, Status::NeedsInput);
    assert_eq!(
        report.question.unwrap().id,
        "approval",
        "old consent must not authorize changed inputs"
    );
    Ok(())
}
