use lenso_cli::{
    Argument, Cli, CommandContract, CommandSpec, Error, Outcome, Question, QuestionKind, serde_json,
};

fn configure() -> Result<Cli, Error> {
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
    Ok(cli)
}

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let setup_args = args.clone();
    let setup = Cli::prepare(&args, move |events| {
        let args = setup_args;
        let mut options = lenso_runtime::LoadOptions::from_args(&args);
        options.events = events;
        let mut cli = configure()?;
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
