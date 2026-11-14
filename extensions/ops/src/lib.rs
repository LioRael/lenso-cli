//! Optional native adapters for declared local process lifecycles, separate from file plans.

mod config;
mod supervisor;

use lenso_cli::{Context, Diagnostic, DiagnosticCode, Engine, Error, Outcome, StepSummary};
use serde_json::{Value, json};

/// Native provider version; registering the provider does not install its commands.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Register the `ops-host/inspect`, `ops-host/run`, and `ops-host/verify` native handlers.
///
/// # Errors
/// Rejects handler or summary registration conflicts. Commands come from a separately installed manifest.
pub fn register(engine: &mut Engine) -> Result<(), Error> {
    engine.handler("ops-host/inspect", inspect)?;
    engine.handler("ops-host/run", supervisor::run)?;
    engine.handler("ops-host/verify", verify)?;
    for handler in ["ops-host/inspect", "ops-host/run", "ops-host/verify"] {
        engine.summarizer(handler, |output| {
            Some(StepSummary {
                affected: None,
                identifiers: ["workflow", "state"]
                    .into_iter()
                    .filter_map(|key| {
                        output
                            .get(key)
                            .and_then(Value::as_str)
                            .map(|value| (key.into(), value.into()))
                    })
                    .collect(),
                operations: None,
            })
        })?;
    }
    Ok(())
}

async fn inspect(ctx: Context) -> Result<Outcome, Error> {
    let config = config::load(&ctx)?;
    let selected = config::selection(&ctx)?;
    if let Some(name) = selected {
        config.workflow(name)?;
    }
    let mut workflows = Vec::new();
    for (name, workflow) in &config.workflows {
        if selected.is_some_and(|selected| selected != name) {
            continue;
        }
        let issues = config::preflight(&ctx, workflow).await;
        let mut required = vec!["host.reads", "host.processes"];
        if !workflow.env.is_empty() {
            required.push("host.env");
        }
        if matches!(workflow.readiness, config::Readiness::Http { .. }) {
            required.push("host.http");
        }
        workflows.push(json!({"name":name,"definition":workflow,"cancellable":true,
            "required_capabilities":required,"inherited_environment":["PATH"],
            "effects":["spawn_process_group","process_owned_application_effects"],
            "ready_to_run":issues.is_empty(),"issues":issues}));
    }
    Ok(Outcome::complete(
        json!({"state":"inspected","workflows":workflows}),
    ))
}

async fn verify(ctx: Context) -> Result<Outcome, Error> {
    let config = config::load(&ctx)?;
    let Some(name) = config::selection(&ctx)? else {
        return Ok(config.question());
    };
    let workflow = config.workflow(name)?;
    let config::Readiness::Http { .. } = workflow.readiness else {
        return Err(Diagnostic::new(DiagnosticCode::InvalidInput)
            .with_details(
                json!({"field":"readiness","reason":"exit_evidence_requires_the_run_invocation"}),
            )
            .with_retryable(false)
            .into());
    };
    let client = config::health_client()?;
    let evidence = config::probe(&ctx, &client, &workflow.readiness).await?;
    let output = json!({"workflow":name,"state":evidence.state,"health":evidence,
        "process":"unknown","scope":"configured_endpoint_only"});
    if evidence.state != "ready" {
        return Err(Diagnostic::new(DiagnosticCode::VerificationFailed)
            .with_details(output)
            .with_retryable(false)
            .with_recovery(
                "inspect_process",
                "Inspect the foreground runner and the configured health endpoint before retrying.",
            )
            .into());
    }
    Ok(Outcome::complete(output))
}
