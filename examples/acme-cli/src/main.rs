use lenso_cli::{
    Argument, Cli, CommandContract, CommandSpec, Error, Outcome, Question, QuestionKind, serde_json,
};

fn configure(project: &std::path::Path) -> Result<Cli, Error> {
    let mut cli = Cli::new("acme");
    cli.command(
        CommandSpec {
            path: vec!["hello".into()],
            workflow: "acme/hello".into(),
            about: "Greet someone".into(),
            provider: "acme".into(),
            arguments: vec![Argument {
                name: "name".into(),
                help: "Who to greet".into(),
                positional: false,
                kind: Default::default(),
            }],
        },
        |ctx| async move {
            let name = ctx.input.get("name").or_else(|| ctx.answers.get("name"));
            match name
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.trim().is_empty())
            {
                Some(name) => Ok(Outcome::complete(
                    serde_json::json!({"greeting":format!("Hello, {name}!")}),
                )),
                None => Ok(Outcome::NeedsInput {
                    question: Question {
                        id: "name".into(),
                        message: "Who should Acme greet?".into(),
                        kind: QuestionKind::Text,
                    },
                }),
            }
        },
    )?;
    cli.provider_version("acme", env!("CARGO_PKG_VERSION"))?;
    cli.contract(
        "acme/hello",
        CommandContract {
            input_schema: Some(serde_json::json!({
                "type":"object","properties":{"name":{"type":"string","minLength":1}}
            })),
            output_schema: Some(serde_json::json!({
                "type":"object","required":["greeting"],"properties":{"greeting":{"type":"string"}}
            })),
            capabilities: Some(vec![]),
            effects: Some(vec![]),
            questions: Some(vec![Question {
                id: "name".into(),
                message: "Who should Acme greet?".into(),
                kind: QuestionKind::Text,
            }]),
            verification: None,
        },
    )?;
    configure_operations(&mut cli, project)?;
    Ok(cli)
}

fn configure_operations(cli: &mut Cli, project: &std::path::Path) -> Result<(), Error> {
    #[derive(Default, serde::Deserialize)]
    #[serde(default, deny_unknown_fields)]
    struct Policy {
        write_note: bool,
    }
    let policy = match std::fs::read(project.join("acme-policy.json")) {
        Ok(bytes) => serde_json::from_slice::<Policy>(&bytes)
            .map_err(|_| lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::InvalidInput))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Policy::default(),
        Err(error) => return Err(error.into()),
    };
    let enabled = policy.write_note;
    cli.command(
        CommandSpec {
            path: vec!["status".into()],
            workflow: "acme/status".into(),
            about: "Inspect Acme's local status without changing the project".into(),
            provider: "acme".into(),
            arguments: vec![],
        },
        move |_| async move {
            Ok(Outcome::complete(
                serde_json::json!({"state":"ready","note_write_enabled":enabled}),
            ))
        },
    )?;
    cli.contract(
        "acme/status",
        CommandContract {
            input_schema: Some(serde_json::json!({"type":"object","additionalProperties":false})),
            capabilities: Some(vec![]),
            effects: Some(vec![]),
            ..Default::default()
        },
    )?;
    lenso_project::register(cli.engine_mut())?;
    cli.engine_mut()
        .handler("acme/note-source", |ctx| async move {
            let text = ctx
                .input
                .get("text")
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty() && text.chars().count() <= 4096)
                .ok_or_else(|| {
                    lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::InvalidInput)
                        .with_details(serde_json::json!({"field":"text"}))
                })?;
            Ok(Outcome::complete(
                serde_json::json!({"files":[lenso_project::FileChange {
                    path: "acme-note.txt".into(), content: text.into(),
                }]}),
            ))
        })?;
    cli.engine_mut().workflow(lenso_cli::Workflow {
        id: "acme/note".into(),
        steps: vec![
            lenso_cli::Step::new("source", "acme/note-source"),
            lenso_cli::Step {
                needs: vec!["source".into()],
                ..lenso_cli::Step::new("plan", "project/plan")
            },
            lenso_cli::Step {
                needs: vec!["plan".into()],
                ..lenso_cli::Step::new("apply", "project/apply")
            },
        ],
    })?;
    cli.engine_mut().grant(
        "acme/note",
        "project.files",
        if enabled {
            serde_json::json!(["acme-note.txt"])
        } else {
            serde_json::json!([])
        },
    )?;
    cli.alias(CommandSpec {
        path: vec!["note".into()],
        workflow: "acme/note".into(),
        about: "Review and explicitly approve a change to acme-note.txt".into(),
        provider: "acme".into(),
        arguments: vec![Argument {
            name: "text".into(),
            help: "New note text".into(),
            positional: false,
            kind: Default::default(),
        }],
    })?;
    cli.contract("acme/note", CommandContract {
        input_schema: Some(serde_json::json!({"type":"object","additionalProperties":false,
            "required":["text"],"properties":{"text":{"type":"string","minLength":1,"maxLength":4096}}})),
        capabilities: Some(vec!["project.files".into()]), effects: Some(vec!["write:acme-note.txt".into()]),
        verification: Some(vec!["Read acme-note.txt and compare it with the approved note text.".into()]),
        ..Default::default()
    })?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let setup_args = args.clone();
    let setup = Cli::prepare(&args, move |events| {
        let args = setup_args;
        let mut options = lenso_runtime::LoadOptions::from_args(&args);
        options.events = events;
        let mut cli = configure(&Cli::project_from_args(&args)?)?;
        lenso_runtime::register_management_with_options(&mut cli, &options)?;
        let version_only = Cli::command_from_args(&args).is_none()
            && args.iter().any(|arg| arg == "--version" || arg == "-V");
        if !version_only
            && !matches!(
                Cli::command_from_args(&args),
                Some(
                    "new"
                        | "init"
                        | "ext"
                        | "preset"
                        | "update"
                        | "config"
                        | "migrate"
                        | "cache"
                        | "package"
                        | "publish"
                )
            )
        {
            lenso_runtime::load_project_with_options(
                &mut cli,
                &Cli::project_from_args(&args)?,
                &options,
            )?;
        }
        Ok::<_, Error>(cli)
    })
    .await;
    let code = match setup {
        Ok(cli) => cli.run_from(args).await,
        Err(error) => {
            Cli::diagnostic(&error, &args);
            1
        }
    };
    std::process::exit(code);
}
