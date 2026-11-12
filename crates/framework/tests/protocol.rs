use std::collections::BTreeMap;

use lenso_cli::{Context, EventSink, ExecutionEvent, Question, QuestionKind, serde_json::json};

#[test]
fn multiselect_requires_distinct_declared_strings() {
    let question = Question {
        id: "items".into(),
        message: "Choose items".into(),
        kind: QuestionKind::MultiSelect {
            options: vec!["button".into(), "input".into()],
        },
    };
    for answer in [json!([]), json!(["input", "button"])] {
        question.validate(&answer).unwrap();
    }
    for answer in [
        json!("button"),
        json!(["button", "button"]),
        json!(["other"]),
        json!([true]),
    ] {
        assert!(question.validate(&answer).is_err());
    }
    assert_eq!(
        lenso_cli::serde_json::to_value(&question).unwrap()["kind"]["type"],
        "multi_select"
    );
}

#[tokio::test]
async fn host_event_channel_does_not_change_the_json_context() {
    let (events, mut stream) = EventSink::channel();
    let context = Context {
        events,
        abi_version: 1,
        project: "/project".into(),
        input: json!({}),
        answers: BTreeMap::new(),
        steps: BTreeMap::new(),
        hooks: BTreeMap::new(),
        capabilities: BTreeMap::new(),
    };
    let wire = lenso_cli::serde_json::to_value(&context).unwrap();
    assert_eq!(
        wire,
        json!({
            "abi_version":1, "project":"/project", "input":{}, "answers":{},
            "steps":{}, "hooks":{}, "capabilities":{}
        })
    );
    let restored: Context = lenso_cli::serde_json::from_value(wire).unwrap();
    assert!(!restored.events.is_enabled());
    context.events.log(b"child log\n".to_vec()).await;
    assert!(
        matches!(stream.next().await, Some(ExecutionEvent::Log { bytes }) if bytes == b"child log\n")
    );
}
