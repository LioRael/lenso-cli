//! Adapt explicitly selected CLI aliases without duplicating their commands or authority.

#[cfg(feature = "mcp")]
pub mod mcp;
mod process;

use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    time::Duration,
};

use lenso_cli::{Diagnostic, DiagnosticCode, Error};
use process::{MAX_JSON, failure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    process::Command,
    sync::{Mutex, Semaphore},
};
pub use tokio_util::sync::CancellationToken;

/// Trusted local launch policy. MCP callers cannot change these values.
pub struct Config {
    /// Explicit native CLI executable; resolved once, with no shell or PATH lookup.
    pub program: PathBuf,
    /// One fixed project directory for this adapter's lifetime.
    pub project: PathBuf,
    /// Exact aliases to expose. Empty selects nothing; prefixes and wildcards are unsupported.
    pub commands: Vec<Vec<String>>,
    /// Disallow dependency downloads in target CLI processes.
    pub offline: bool,
    /// Require the target CLI's existing project lock.
    pub locked: bool,
    /// Maximum duration for each discovery or execution process, excluding bounded cleanup.
    pub timeout: Duration,
}

/// Description generated from the existing command catalog, also usable without MCP.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// Unambiguous command path joined with double underscores.
    pub name: String,
    /// Existing command description.
    pub description: String,
    /// Validated JSON input wrapper around the command's declared workflow input.
    pub input_schema: Value,
    /// The public invocation envelope, distinct from authored handler outputs.
    pub output_schema: Value,
    /// Read-only hint only when the author explicitly declares no effects; never authority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
    /// Original catalog record, including unknown contracts and provider versions.
    #[serde(rename = "_meta")]
    pub metadata: Value,
}

struct Selected {
    tool: Tool,
    path: Vec<String>,
    catalog: Value,
    validator: jsonschema::Validator,
}

/// Actual process exit code and the unchanged CLI result/diagnostic envelope.
#[derive(Debug, Serialize)]
pub struct Invocation {
    /// CLI exit status, or the corresponding adapter diagnostic status.
    pub exit_code: i32,
    /// Existing result, question or public diagnostic; report locations become opaque URIs.
    pub result: Value,
    /// Whether the retained resource contains the target's full report rather than its public result only.
    pub report_complete: bool,
}

/// Invocation parameters. Neither target identity nor grants are caller-controlled.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    #[serde(default = "empty_object")]
    input: Value,
    #[serde(default)]
    answers: BTreeMap<String, Value>,
    #[serde(default)]
    mode: Mode,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Full,
    #[default]
    Summary,
}

fn empty_object() -> Value {
    json!({})
}

#[derive(Default)]
struct Reports {
    entries: VecDeque<(String, Value, usize)>,
    bytes: usize,
    next: u64,
}

/// Optional process adapter, independent of CLI Core and the standalone Lenso binary.
pub struct Agent {
    config: Config,
    selected: BTreeMap<String, Selected>,
    scratch: tempfile::TempDir,
    reports: Mutex<Reports>,
    calls: Semaphore,
    shutdown: CancellationToken,
}

impl Agent {
    /// Read static metadata and compile schemas for the explicitly selected aliases.
    ///
    /// # Errors
    /// Rejects invalid identities, aliases, schemas, protocol output and unsupported platforms.
    pub async fn connect(mut config: Config) -> Result<Self, Error> {
        if !cfg!(unix) {
            return Err(failure(
                DiagnosticCode::InvalidInput,
                "unix_supervision_required",
            ));
        }
        config.program = config.program.canonicalize()?;
        config.project = config.project.canonicalize()?;
        if !config.program.is_file()
            || !config.project.is_dir()
            || config.timeout.is_zero()
            || config.timeout > Duration::from_secs(86400)
        {
            return Err(failure(
                DiagnosticCode::InvalidInput,
                "invalid_launch_policy",
            ));
        }
        let mut agent = Self {
            config,
            selected: BTreeMap::new(),
            scratch: tempfile::tempdir()?,
            reports: Mutex::new(Reports::default()),
            calls: Semaphore::new(4),
            shutdown: CancellationToken::new(),
        };
        let catalog = agent.catalog(&agent.shutdown).await?;
        for path in &agent.config.commands {
            if path.is_empty()
                || path.iter().any(|part| {
                    part.is_empty()
                        || part.starts_with('-')
                        || !part
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                })
            {
                return Err(failure(DiagnosticCode::InvalidInput, "invalid_selection"));
            }
            let record = catalog
                .iter()
                .find(|c| c["path"] == json!(path))
                .ok_or_else(|| failure(DiagnosticCode::InvalidInput, "unknown_selection"))?;
            let name = path.join("__");
            if name.len() > 128 || agent.selected.contains_key(&name) {
                return Err(failure(
                    DiagnosticCode::InvalidInput,
                    "duplicate_or_long_selection",
                ));
            }
            let input = record
                .pointer("/contract/input_schema")
                .filter(|s| !s.is_null())
                .unwrap_or(&record["cli_input_schema"])
                .clone();
            let mut schema = json!({"type":"object","additionalProperties":false,"properties":{
                "input":input,"answers":{"type":"object"},
                "mode":{"type":"string","enum":["full","summary"],"default":"summary"}
            }});
            // External resolvers are disabled at compile time; metadata cannot cause HTTP/file reads.
            let mut validator = jsonschema::validator_for(&schema)
                .map_err(|_| failure(DiagnosticCode::InvalidInput, "invalid_or_external_schema"))?;
            if !validator.is_valid(&json!({"input":{}})) {
                schema["required"] = json!(["input"]);
                validator = jsonschema::validator_for(&schema).map_err(|_| {
                    failure(DiagnosticCode::InvalidInput, "invalid_or_external_schema")
                })?;
            }
            let tool = Tool {
                name: name.clone(),
                description: record["about"].as_str().unwrap_or_default().into(),
                input_schema: schema,
                output_schema: json!({"type":"object","required":["exit_code","result"],
                    "properties":{"exit_code":{"type":"integer"},"result":{"type":"object"},"report_complete":{"type":"boolean"}}}),
                annotations: record
                    .pointer("/contract/effects")
                    .and_then(Value::as_array)
                    .filter(|effects| effects.is_empty())
                    .map(|_| json!({"readOnlyHint":true,"destructiveHint":false})),
                metadata: json!({"lenso/command":record}),
            };
            agent.selected.insert(
                name,
                Selected {
                    tool,
                    path: path.clone(),
                    catalog: record.clone(),
                    validator,
                },
            );
        }
        Ok(agent)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.config.program);
        command
            .current_dir(&self.config.project)
            .arg("--project")
            .arg(&self.config.project)
            .env("LENSO_NO_UPDATE_CHECK", "1");
        if self.config.offline {
            command.arg("--offline");
        }
        if self.config.locked {
            command.arg("--locked");
        }
        command
    }

    async fn catalog(&self, cancel: &CancellationToken) -> Result<Vec<Value>, Error> {
        let mut command = self.command();
        command.args(["help", "--json"]);
        let output = process::run(command, cancel, self.config.timeout).await?;
        if let Some(code) = output.stopped {
            return Err(failure(code, "discovery_interrupted"));
        }
        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|_| failure(DiagnosticCode::OperationFailed, "invalid_catalog_json"))?;
        if output.code != 0 && value["type"] == "diagnostic" {
            let code =
                serde_json::from_value::<DiagnosticCode>(value["code"].clone()).map_err(|_| {
                    failure(
                        DiagnosticCode::InvalidResponse,
                        "invalid_catalog_diagnostic",
                    )
                })?;
            let mut diagnostic = Diagnostic::new(code).with_details(value["details"].clone());
            if let Some(retryable) = value["retryable"].as_bool() {
                diagnostic = diagnostic.with_retryable(retryable);
            }
            if let (Some(kind), Some(message)) = (
                value.pointer("/recovery/kind").and_then(Value::as_str),
                value.pointer("/recovery/message").and_then(Value::as_str),
            ) {
                diagnostic = diagnostic.with_recovery(kind, message);
            }
            return Err(diagnostic.into());
        }
        if output.code != 0 || value["type"] != "command_catalog" || value["schema_version"] != 1 {
            return Err(failure(
                DiagnosticCode::OperationFailed,
                "catalog_unavailable",
            ));
        }
        value["commands"]
            .as_array()
            .cloned()
            .ok_or_else(|| failure(DiagnosticCode::OperationFailed, "invalid_catalog"))
    }

    /// Return only selected descriptions. Discovery is not execution authorization.
    pub fn tools(&self) -> Vec<Tool> {
        self.selected.values().map(|s| s.tool.clone()).collect()
    }

    /// Invoke an exact selected alias using the native CLI JSON path, without implicit approvals.
    ///
    /// # Errors
    /// Returns public boundary errors; target CLI failures remain inside `Invocation`.
    pub async fn call(
        &self,
        name: &str,
        arguments: Value,
        cancel: CancellationToken,
    ) -> Result<Invocation, Error> {
        let selected = self
            .selected
            .get(name)
            .ok_or_else(|| failure(DiagnosticCode::PermissionDenied, "tool_not_selected"))?;
        if serde_json::to_vec(&arguments)?.len() > 1024 * 1024 {
            return Err(failure(
                DiagnosticCode::InvalidInput,
                "tool_arguments_rejected",
            ));
        }
        let args: Arguments = serde_json::from_value(arguments)
            .map_err(|_| failure(DiagnosticCode::InvalidInput, "tool_arguments_rejected"))?;
        if !args.input.is_object() {
            return Err(failure(
                DiagnosticCode::InvalidInput,
                "input_requires_object",
            ));
        }
        let normalized = serde_json::to_value(&args)?;
        let issues: Vec<_> = selected
            .validator
            .iter_errors(&normalized)
            .take(8)
            .map(|error| json!({"schema_path":error.schema_path().as_str()}))
            .collect();
        if !issues.is_empty() {
            return Err(Diagnostic::new(DiagnosticCode::InvalidInput)
                .with_details(json!({"adapter":"lenso-agent","reason":"tool_arguments_rejected","issues":issues}))
                .with_retryable(false).into());
        }
        let permit = tokio::select! {
            biased;
            _ = self.shutdown.cancelled() => return Err(Error::Cancelled),
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            permit = self.calls.acquire() => permit.map_err(|_| Error::Cancelled)?,
        };
        // Both request cancellation and transport shutdown interrupt the CLI supervisor.
        let stopped = cancel.child_token();
        let bridge = self.shutdown.clone();
        let child = stopped.clone();
        let bridge = tokio::spawn(async move {
            bridge.cancelled().await;
            child.cancel();
        });
        struct Abort(tokio::task::JoinHandle<()>);
        impl Drop for Abort {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _bridge = Abort(bridge);
        let current = self.catalog(&stopped).await?;
        if current.iter().find(|c| c["path"] == json!(selected.path)) != Some(&selected.catalog) {
            return Err(failure(
                DiagnosticCode::StalePlan,
                "command_contract_changed",
            ));
        }
        let work = tempfile::tempdir_in(self.scratch.path())?;
        let answers = work.path().join("answers.json");
        std::fs::write(&answers, serde_json::to_vec(&args.answers)?)?;
        let report = work.path().join("report.json");
        let mut command = self.command();
        command
            .args(&selected.path)
            .arg("--json")
            .arg("--input")
            .arg(serde_json::to_string(&args.input)?)
            .arg("--answers")
            .arg(answers)
            .arg("--report")
            .arg(&report);
        if matches!(args.mode, Mode::Summary) {
            command.arg("--summary");
        }
        let output = process::run(command, &stopped, self.config.timeout).await?;
        let parsed = serde_json::from_slice::<Value>(&output.stdout)
            .ok()
            .filter(|v| {
                v.is_object() && matches!(v["type"].as_str(), Some("result" | "diagnostic"))
            });
        let mut result = if let Some(code) = output.stopped {
            if code == DiagnosticCode::Cancelled
                && parsed.as_ref().is_some_and(|v| v["code"] == "cancelled")
            {
                parsed.expect("checked parsed diagnostic")
            } else {
                serde_json::to_value(
                    Diagnostic::new(code)
                        .with_details(json!({
                    "adapter":"lenso-agent","target_result":parsed,"cleanup":"not_reported",
                    "operation_may_have_completed":true}))
                        .with_retryable(false),
                )?
            }
        } else {
            parsed
                .ok_or_else(|| failure(DiagnosticCode::OperationFailed, "invalid_execution_json"))?
        };
        let full = if report.is_file() && report.metadata()?.len() <= MAX_JSON as u64 {
            serde_json::from_slice::<Value>(&std::fs::read(report)?)
                .ok()
                .filter(Value::is_object)
        } else {
            None
        };
        let report_complete = full.is_some();
        let mut full = full.unwrap_or_else(|| result.clone());
        if !report_complete {
            full["report_complete"] = json!(false);
        }
        if output.stopped.is_some() && result["code"] != full["code"] {
            if let Some(execution) = full.get("execution") {
                result["execution"] = execution.clone();
            }
            full["adapter_diagnostic"] = result.clone();
        }
        let mut reports = self.reports.lock().await;
        let uri = format!(
            "lenso-report://{}/{}",
            self.scratch
                .path()
                .file_name()
                .expect("tempdir has a name")
                .to_string_lossy(),
            reports.next
        );
        reports.next += 1;
        result["report"] = json!(uri);
        full["report"] = json!(uri);
        let bytes = serde_json::to_vec(&full)?.len();
        while reports.entries.len() >= 16 || reports.bytes + bytes > 16 * 1024 * 1024 {
            let Some((_, _, old)) = reports.entries.pop_front() else {
                break;
            };
            reports.bytes -= old;
        }
        reports.bytes += bytes;
        reports.entries.push_back((uri, full, bytes));
        drop(permit);
        let code = match output.stopped {
            Some(DiagnosticCode::Cancelled) => 130,
            Some(_) => 1,
            None => output.code,
        };
        Ok(Invocation {
            exit_code: code,
            result,
            report_complete,
        })
    }

    /// Read a retained same-invocation full report without replaying the operation.
    ///
    /// # Errors
    /// Rejects unknown/evicted URIs; no URI is interpreted as a filesystem path.
    pub async fn report(&self, uri: &str) -> Result<Value, Error> {
        self.reports
            .lock()
            .await
            .entries
            .iter()
            .find(|(id, _, _)| id == uri)
            .map(|(_, value, _)| value.clone())
            .ok_or_else(|| failure(DiagnosticCode::InvalidInput, "unknown_report"))
    }

    /// List only report URIs retained by this adapter instance.
    pub async fn reports(&self) -> Vec<String> {
        self.reports
            .lock()
            .await
            .entries
            .iter()
            .map(|(id, _, _)| id.clone())
            .collect()
    }

    /// Cancel the active invocation and wait for its bounded supervision cleanup.
    pub async fn shutdown(&self) {
        self.shutdown.cancel();
        let _permit = self.calls.acquire_many(4).await;
    }
}

/// Wait for a local shutdown signal without forwarding untrusted reason text.
///
/// # Errors
/// Returns an operating-system signal registration error.
pub async fn shutdown_signal() -> Result<(), std::io::Error> {
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let terminated = async {
        #[cfg(unix)]
        {
            terminate.recv().await;
        }
        #[cfg(not(unix))]
        std::future::pending::<()>().await;
    };
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminated => Ok(()),
    }
}
