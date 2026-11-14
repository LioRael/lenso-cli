use std::{path::PathBuf, sync::Arc, time::Duration};

use clap::{Parser, Subcommand};
use lenso_agent::{Agent, CancellationToken, Config};
use lenso_cli::{Diagnostic, DiagnosticCode, Error};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(
    version,
    about = "Adapt explicitly selected native CLI commands to JSON tools or optional MCP"
)]
struct Args {
    #[arg(long)]
    program: PathBuf,
    #[arg(long)]
    project: PathBuf,
    /// Exact space-separated alias, repeat once per selected operation; empty selects nothing.
    #[arg(long = "allow")]
    commands: Vec<String>,
    #[arg(long)]
    offline: bool,
    #[arg(long)]
    locked: bool,
    #[arg(long, default_value_t = 30000)]
    timeout_ms: u64,
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Export tools derived from the target's machine-readable command catalog.
    Tools,
    /// Call one selected tool; full results are the one-shot default.
    Call {
        #[arg(long)]
        name: String,
        #[arg(long, default_value = "{}")]
        arguments: String,
        /// Save this invocation's full report to a new local file; required for summary mode.
        #[arg(long)]
        report: Option<PathBuf>,
    },
    /// Serve the selected tools and same-invocation report resources over MCP stdio.
    #[cfg(feature = "mcp")]
    Mcp,
}

fn invalid() -> Error {
    Diagnostic::new(DiagnosticCode::InvalidInput)
        .with_retryable(false)
        .into()
}

#[tokio::main]
async fn main() {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return;
        }
        Err(_) => {
            println!("{}", json!(invalid().diagnostic()));
            std::process::exit(2);
        }
    };
    #[cfg(feature = "mcp")]
    let mcp = matches!(args.command, Action::Mcp);
    #[cfg(not(feature = "mcp"))]
    let mcp = false;
    let code = match run(args).await {
        Ok(code) => code,
        Err(error) => {
            if mcp {
                eprintln!("{}", json!(error.diagnostic()));
            } else {
                println!("{}", json!(error.diagnostic()));
            }
            error.exit_code()
        }
    };
    std::process::exit(code);
}

async fn run(args: Args) -> Result<i32, Error> {
    let agent = Arc::new(
        Agent::connect(Config {
            program: args.program,
            project: args.project,
            commands: args
                .commands
                .iter()
                .map(|path| path.split_whitespace().map(str::to_owned).collect())
                .collect(),
            offline: args.offline,
            locked: args.locked,
            timeout: Duration::from_millis(args.timeout_ms),
        })
        .await?,
    );
    match args.command {
        Action::Tools => println!(
            "{}",
            json!({"type":"agent_tools","schema_version":1,"tools":agent.tools()})
        ),
        Action::Call {
            name,
            arguments,
            report,
        } => {
            let mut arguments: Value = serde_json::from_str(&arguments).map_err(|_| invalid())?;
            let object = arguments.as_object_mut().ok_or_else(invalid)?;
            object.entry("mode").or_insert(json!("full"));
            if object["mode"] == "summary" && report.is_none() {
                return Err(invalid());
            }
            let mut file = report
                .as_ref()
                .map(|path| {
                    let mut options = std::fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        options.mode(0o600);
                    }
                    options.open(path)
                })
                .transpose()?;
            let cancel = CancellationToken::new();
            let signal = cancel.clone();
            let watcher = tokio::spawn(async move {
                let _ = lenso_agent::shutdown_signal().await;
                signal.cancel();
            });
            let result = agent.call(&name, arguments, cancel).await;
            watcher.abort();
            let mut result = result?;
            if let Some(file) = &mut file {
                let mut full = agent
                    .report(result.result["report"].as_str().ok_or_else(invalid)?)
                    .await?;
                full["report"] = json!(report);
                save_report(file, &full, &result.result)?;
                result.result["report"] = json!(report);
            } else {
                result
                    .result
                    .as_object_mut()
                    .expect("CLI result is object")
                    .remove("report");
            }
            println!("{}", json!(result));
            return Ok(result.exit_code);
        }
        #[cfg(feature = "mcp")]
        Action::Mcp => lenso_agent::mcp::serve_stdio(agent).await?,
    }
    Ok(0)
}

fn save_report(file: &mut std::fs::File, full: &Value, result: &Value) -> Result<(), Error> {
    serde_json::to_writer(&mut *file, full).map_err(Error::from)
        .and_then(|()| file.sync_all().map_err(Error::from))
        .map_err(|_| Diagnostic::new(DiagnosticCode::IoError)
            .with_details(json!({"report_saved":false,"operation_may_have_completed":true,"target_result":result}))
            .with_retryable(false)
            .with_recovery("inspect_execution", "Inspect the returned execution evidence before retrying; report storage failed after execution.")
            .into())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn failed_report_storage_retains_outcome_and_forbids_automatic_retries() {
        let mut directory = std::fs::File::open(".").unwrap();
        let result = json!({"status":"complete","execution":{"id":"completed-execution"}});
        let error =
            save_report(&mut directory, &json!({"outputs":{"work":true}}), &result).unwrap_err();
        let public = json!(error.diagnostic());
        assert_eq!(public["code"], "io_error");
        assert_eq!(public["retryable"], false);
        assert_eq!(public["details"]["operation_may_have_completed"], true);
        assert_eq!(public["details"]["target_result"], result);
    }
}
