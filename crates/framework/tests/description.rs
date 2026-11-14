use lenso_cli::{Argument, ArgumentKind, Cli, CommandContract, CommandSpec, Outcome, serde_json};
use serde_json::json;

fn command(path: &[&str]) -> CommandSpec {
    CommandSpec {
        path: path.iter().map(|part| (*part).into()).collect(),
        workflow: "example/check".into(),
        about: "Inspect example state".into(),
        provider: "example".into(),
        arguments: vec![],
    }
}

#[test]
fn discovery_derives_legacy_shapes_without_invoking_handlers() {
    let mut cli = Cli::new("test");
    assert!(cli.describe(&[]).unwrap().commands.is_empty());
    let mut spec = command(&["tool", "check"]);
    spec.arguments = [ArgumentKind::Text, ArgumentKind::Flag, ArgumentKind::List]
        .into_iter()
        .zip(["name", "verbose", "items"])
        .map(|(kind, name)| Argument {
            name: name.into(),
            help: format!("Help for {name}"),
            positional: false,
            kind,
        })
        .collect();
    cli.command(spec, |_| async {
        panic!("discovery must not execute handlers")
    })
    .unwrap();
    let catalog = serde_json::to_value(cli.describe(&["tool".into()]).unwrap()).unwrap();
    assert_eq!(catalog["schema_version"], 1);
    let description = &catalog["commands"][0];
    assert!(description["contract"].is_null());
    assert!(description["provider_version"].is_null());
    let properties = &description["cli_input_schema"]["properties"];
    assert_eq!(properties["name"]["type"], "string");
    assert_eq!(properties["verbose"]["type"], "boolean");
    assert_eq!(properties["items"]["items"]["type"], "string");
    assert_eq!(properties["items"]["minItems"], 1);
    assert!(cli.describe(&["ops".into()]).is_err());
}

#[test]
fn aliases_share_contracts_but_never_expose_grants() {
    let mut cli = Cli::new("test");
    cli.command(command(&["z-check"]), |_| async {
        Ok(Outcome::complete(json!(null)))
    })
    .unwrap();
    cli.alias(command(&["a-check"])).unwrap();
    cli.provider_version("example", "1.0.0").unwrap();
    cli.contract(
        "example/check",
        CommandContract {
            input_schema: Some(json!({"type":"object","required":["mode"],"properties":{
                "mode":{"type":"string","enum":["fast","full"],"default":"fast"}
            }})),
            effects: Some(vec![]),
            ..Default::default()
        },
    )
    .unwrap();
    cli.engine_mut()
        .grant(
            "example/check",
            "host.credentials",
            json!({"token":"SECRET"}),
        )
        .unwrap();
    let wire = serde_json::to_value(cli.describe(&[]).unwrap()).unwrap();
    assert_eq!(wire["commands"][0]["path"], json!(["a-check"]));
    assert_eq!(
        wire["commands"][0]["contract"],
        wire["commands"][1]["contract"]
    );
    assert_eq!(wire["commands"][0]["contract"]["effects"], json!([]));
    assert!(wire["commands"][0]["contract"]["capabilities"].is_null());
    assert!(!wire.to_string().contains("SECRET"));
    assert!(cli.alias(command(&["a-check"])).is_err());
    assert!(cli.alias(command(&["help"])).is_err());
    assert!(cli.provider_version("example", "2.0.0").is_err());
    assert!(cli.contract("missing", CommandContract::default()).is_err());
    assert!(
        cli.contract("example/check", CommandContract::default())
            .is_err()
    );
}

#[tokio::test]
async fn machine_help_does_not_parse_workflow_input_or_answers() {
    let mut cli = Cli::new("test");
    cli.command(command(&["tool", "check"]), |_| async {
        panic!("must not execute")
    })
    .unwrap();
    let args = [
        "test",
        "help",
        "tool",
        "check",
        "--json",
        "--input",
        "invalid JSON",
        "--answers",
        "/missing",
    ];
    assert_eq!(cli.clone().run_from(args.map(Into::into).to_vec()).await, 0);
    assert_eq!(
        cli.run_from(
            ["test", "help", "missing", "--json"]
                .map(Into::into)
                .to_vec()
        )
        .await,
        1
    );
}
