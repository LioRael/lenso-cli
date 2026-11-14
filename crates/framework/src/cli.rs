use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::future::Future;

use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::terminal::{ColorMode, Prompt, Terminal, UiOptions};
use crate::{CommandCatalog, CommandContract, CommandDescription, Diagnostic, DiagnosticCode};
use crate::{Context, Engine, Error, EventSink, Outcome, Question, QuestionKind, Step, Workflow};

/// Argument value shape exposed by a dynamically installed command.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgumentKind {
    /// An optional string option or positional.
    #[default]
    Text,
    /// A boolean switch set by presence on the command line.
    Flag,
    /// One or more string values, such as multiple Registry item addresses.
    List,
}

/// A named argument exposed by a dynamically installed command.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    /// Option or positional name, also used as the input object key.
    pub name: String,
    /// Help text.
    pub help: String,
    /// Parse this value positionally instead of as a long option.
    #[serde(default)]
    pub positional: bool,
    /// Value shape, defaulting to text for existing extension manifests.
    #[serde(default)]
    pub kind: ArgumentKind,
}

/// Static command metadata; loading it never executes extension code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSpec {
    /// Command path, such as `["db", "status"]`.
    pub path: Vec<String>,
    /// Namespaced workflow identifier.
    pub workflow: String,
    /// Short help text.
    pub about: String,
    /// Extension or application that provides this command.
    pub provider: String,
    /// Optional string flags; missing values may be requested through questions.
    #[serde(default)]
    pub arguments: Vec<Argument>,
}

/// An embeddable CLI frontend shared by `lenso` and branded applications.
#[derive(Debug, Clone)]
pub struct Cli {
    name: String,
    engine: Engine,
    commands: Vec<CommandSpec>,
    contracts: BTreeMap<String, CommandContract>,
    provider_versions: BTreeMap<String, String>,
}

impl Cli {
    /// Construct a CLI with no project-specific commands.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            engine: Engine::new(),
            commands: Vec::new(),
            contracts: BTreeMap::new(),
            provider_versions: BTreeMap::new(),
        }
    }

    /// Access the registry for trusted native or sandboxed runtime adapters.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Attach optional static declarations to a registered workflow without changing its handler.
    ///
    /// # Errors
    /// Rejects unknown workflows and duplicate declarations.
    pub fn contract(&mut self, workflow: &str, contract: CommandContract) -> Result<(), Error> {
        if !self.engine.workflows().any(|id| id == workflow) {
            return Err(Error::Invalid(format!("unknown workflow: {workflow}")));
        }
        if self.contracts.contains_key(workflow) {
            return Err(Error::Invalid(format!("duplicate contract: {workflow}")));
        }
        self.contracts.insert(workflow.into(), contract);
        Ok(())
    }

    /// Declare a provider version without exposing its installation policy or credential bindings.
    ///
    /// # Errors
    /// Rejects empty identities and inconsistent versions for the same provider.
    pub fn provider_version(&mut self, provider: &str, version: &str) -> Result<(), Error> {
        if provider.is_empty()
            || version.is_empty()
            || self
                .provider_versions
                .get(provider)
                .is_some_and(|old| old != version)
        {
            return Err(Error::Invalid(format!(
                "invalid or conflicting provider version: {provider}"
            )));
        }
        self.provider_versions
            .insert(provider.into(), version.into());
        Ok(())
    }

    /// Describe aliases beneath a path without invoking handlers or reading execution contexts.
    ///
    /// # Errors
    /// Rejects paths with no registered aliases; the empty path returns the whole catalog.
    pub fn describe(&self, path: &[String]) -> Result<CommandCatalog<'_>, Error> {
        let mut commands: Vec<_> = self
            .commands
            .iter()
            .filter(|spec| spec.path.starts_with(path))
            .map(|spec| CommandDescription {
                command: spec,
                provider_version: self
                    .provider_versions
                    .get(&spec.provider)
                    .map(String::as_str),
                cli_input_schema: crate::description::input_schema(spec),
                contract: self.contracts.get(&spec.workflow),
            })
            .collect();
        if !path.is_empty() && commands.is_empty() {
            return Err(Error::Invalid(format!(
                "unknown command path: {}",
                path.join(" ")
            )));
        }
        commands.sort_by(|a, b| a.command.path.cmp(&b.command.path));
        Ok(CommandCatalog {
            r#type: "command_catalog",
            schema_version: 1,
            commands,
        })
    }

    /// Register a command alias for an existing workflow.
    ///
    /// # Errors
    /// Rejects reserved names, malformed paths, duplicate flags, and alias conflicts.
    pub fn alias(&mut self, spec: CommandSpec) -> Result<(), Error> {
        const RESERVED: &[&str] = &["run", "help", "version"];
        const FLAGS: &[&str] = &[
            "help",
            "json",
            "answers",
            "project",
            "yes",
            "input",
            "color",
            "debug",
            "no-interactive",
            "offline",
            "locked",
            "summary",
            "report",
        ];
        let valid = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !s.starts_with('-')
        };
        if spec.path.is_empty()
            || spec.path.iter().any(|s| !valid(s))
            || RESERVED.contains(&spec.path[0].as_str())
        {
            return Err(Error::Invalid(format!(
                "invalid or reserved command path: {:?}",
                spec.path
            )));
        }
        if !self.engine.workflows().any(|id| id == spec.workflow) {
            return Err(Error::Invalid(format!(
                "unknown workflow: {}",
                spec.workflow
            )));
        }
        let mut flags = BTreeSet::new();
        for arg in &spec.arguments {
            if !valid(&arg.name)
                || FLAGS.contains(&arg.name.as_str())
                || !flags.insert(&arg.name)
                || (arg.positional && matches!(arg.kind, ArgumentKind::Flag))
            {
                return Err(Error::Invalid(format!(
                    "invalid or duplicate argument: {}",
                    arg.name
                )));
            }
        }
        if let Some(other) = self
            .commands
            .iter()
            .find(|c| c.path.starts_with(&spec.path) || spec.path.starts_with(&c.path))
        {
            return Err(Diagnostic::new(DiagnosticCode::CommandConflict)
                .with_details(serde_json::json!({"commands":[
                    {"path":other.path,"provider":other.provider},
                    {"path":spec.path,"provider":spec.provider}
                ]}))
                .with_retryable(false)
                .with_recovery(
                    "configure_alias",
                    "Disable or rename one conflicting alias in the project declaration.",
                )
                .into());
        }
        self.commands.push(spec);
        Ok(())
    }

    /// Register a single native handler and its command without a multi-step definition.
    ///
    /// # Errors
    /// Returns any handler, workflow, or alias registration error without partial registration.
    pub fn command<F, Fut>(&mut self, spec: CommandSpec, handler: F) -> Result<(), Error>
    where
        F: Fn(Context) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Outcome, Error>> + Send + 'static,
    {
        let mut next = self.clone();
        let handler_id = format!("native/{}", spec.workflow);
        next.engine.handler(&handler_id, handler)?;
        next.engine.workflow(Workflow {
            id: spec.workflow.clone(),
            steps: vec![Step::new("main", handler_id)],
        })?;
        next.alias(spec)?;
        *self = next;
        Ok(())
    }

    /// Resolve the explicit project option before discovering installed manifests.
    ///
    /// # Errors
    /// Rejects a missing path or an inaccessible current directory.
    pub fn project_from_args(args: &[OsString]) -> Result<PathBuf, Error> {
        let mut project = std::env::current_dir()?;
        let mut explicit = false;
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            if arg == "--project" {
                explicit = true;
                project = PathBuf::from(
                    args.next()
                        .ok_or_else(|| Error::Invalid("--project needs a path".into()))?,
                );
            } else if let Some(path) = arg.to_str().and_then(|s| s.strip_prefix("--project=")) {
                explicit = true;
                project = PathBuf::from(path);
            }
        }
        if !explicit
            && let Some(root) = project
                .ancestors()
                .find(|path| path.join("lenso.toml").is_file())
        {
            project = root.to_owned();
        }
        Ok(project)
    }

    /// Identify the entry command before project discovery, respecting global value options.
    pub fn command_from_args(args: &[OsString]) -> Option<&str> {
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            if ["--project", "--input", "--answers", "--color", "--report"]
                .iter()
                .any(|flag| arg == *flag)
            {
                args.next();
                continue;
            }
            let value = arg.to_str()?;
            if !value.starts_with('-') {
                return Some(value);
            }
        }
        None
    }

    /// Initialize a CLI on a blocking worker while rendering host preparation events.
    ///
    /// # Errors
    /// Returns initialization failures or a failed worker task.
    pub async fn prepare<F>(args: &[OsString], initialize: F) -> Result<Self, Error>
    where
        F: FnOnce(EventSink) -> Result<Self, Error> + Send + 'static,
    {
        let mut terminal = Terminal::new(UiOptions::from_args(args));
        let (sink, mut events) = EventSink::channel();
        let task = tokio::task::spawn_blocking(move || initialize(sink));
        tokio::pin!(task);
        loop {
            tokio::select! {
                Some(event) = events.next() => terminal.event(event)?,
                result = &mut task => {
                    while let Some(event) = events.try_next() { terminal.event(event)?; }
                    terminal.stop();
                    return result.map_err(|error| Error::Operation(error.to_string()))?;
                }
            }
        }
    }

    /// Execute this CLI using the process argument vector and terminal capabilities.
    pub async fn run(self) -> i32 {
        self.run_from(std::env::args_os().collect()).await
    }

    /// Execute supplied arguments, emitting JSON results or terminal output.
    pub async fn run_from(self, args: Vec<OsString>) -> i32 {
        let options = UiOptions::from_args(&args);
        let mut terminal = Terminal::new(options);
        match self.execute(args, options, &mut terminal).await {
            Ok(code) => code,
            Err(error) => {
                terminal.diagnostic(&error);
                error.exit_code()
            }
        }
    }

    /// Render discovery errors using the same output/color flags as execution.
    pub fn diagnostic(error: &Error, args: &[OsString]) {
        Terminal::new(UiOptions::from_args(args)).diagnostic(error);
    }

    fn parser(&self, options: UiOptions) -> Command {
        let mut parser = Command::new(self.name.clone())
            .disable_help_subcommand(true)
            .version(env!("CARGO_PKG_VERSION"))
            .color(
                if options.json
                    || (options.color == ColorMode::Auto && std::env::var_os("NO_COLOR").is_some())
                {
                    clap::ColorChoice::Never
                } else {
                    match options.color {
                        ColorMode::Auto => clap::ColorChoice::Auto,
                        ColorMode::Always => clap::ColorChoice::Always,
                        ColorMode::Never => clap::ColorChoice::Never,
                    }
                },
            )
            .arg(
                Arg::new("offline")
                    .long("offline")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Use cached or local dependencies without downloading"),
            )
            .arg(
                Arg::new("locked")
                    .long("locked")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Require an existing unchanged lenso.lock"),
            )
            .arg(
                Arg::new("color")
                    .long("color")
                    .global(true)
                    .value_parser(["auto", "always", "never"])
                    .default_value("auto")
                    .help("Control terminal colors"),
            )
            .arg(
                Arg::new("debug")
                    .long("debug")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Include the complete execution report on stderr"),
            )
            .arg(
                Arg::new("no-interactive")
                    .long("no-interactive")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Never open terminal prompts"),
            )
            .about("Run explicitly installed commands and workflows")
            .arg(
                Arg::new("project")
                    .long("project")
                    .global(true)
                    .value_name("DIRECTORY"),
            )
            .arg(
                Arg::new("json")
                    .long("json")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Return a JSON result; never prompt"),
            )
            .arg(
                Arg::new("summary")
                    .long("summary")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .requires("json")
                    .help("Omit full step outputs and return an execution summary"),
            )
            .arg(
                Arg::new("report")
                    .long("report")
                    .global(true)
                    .value_name("NEW_FILE")
                    .requires("json")
                    .help("Save this invocation's full JSON report to a new file"),
            )
            .arg(
                Arg::new("answers")
                    .long("answers")
                    .global(true)
                    .value_name("FILE"),
            )
            .arg(
                Arg::new("yes")
                    .long("yes")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Answer explicit confirmation questions with true"),
            )
            .arg(
                Arg::new("input")
                    .long("input")
                    .global(true)
                    .value_name("JSON")
                    .help("Workflow input object"),
            )
            .subcommand(
                Command::new("help")
                    .about("Show help or a static command catalog with --json")
                    .arg(Arg::new("command").num_args(0..)),
            )
            .subcommand(
                Command::new("run")
                    .about("Run a workflow by its installed identifier")
                    .arg(Arg::new("workflow").required(true)),
            );
        fn add_tree(mut node: Command, prefix: &[String], specs: &[CommandSpec]) -> Command {
            let names: BTreeSet<_> = specs
                .iter()
                .filter(|s| s.path.starts_with(prefix))
                .filter_map(|s| s.path.get(prefix.len()))
                .collect();
            for name in names {
                let mut path = prefix.to_vec();
                path.push(name.clone());
                let mut child = Command::new(name.clone());
                if let Some(spec) = specs.iter().find(|s| s.path == path) {
                    child = child.about(format!("{} [{}]", spec.about, spec.provider));
                    let mut position = 0;
                    for arg in &spec.arguments {
                        let mut value = Arg::new(arg.name.clone()).help(arg.help.clone());
                        if arg.positional {
                            position += 1;
                            value = value.index(position);
                        } else {
                            value = value.long(arg.name.clone());
                        }
                        if matches!(arg.kind, ArgumentKind::Flag) {
                            value = value.action(ArgAction::SetTrue);
                        } else if matches!(arg.kind, ArgumentKind::List) {
                            value = value.num_args(1..).action(ArgAction::Append);
                        }
                        child = child.arg(value);
                    }
                } else {
                    child = add_tree(child.subcommand_required(true), &path, specs);
                }
                node = node.subcommand(child);
            }
            node
        }
        parser = add_tree(parser, &[], &self.commands);
        parser
    }

    async fn execute(
        &self,
        args: Vec<OsString>,
        options: UiOptions,
        terminal: &mut Terminal,
    ) -> Result<i32, Error> {
        let json = options.json;
        let mut parser = self.parser(options);
        let matches = match parser.clone().try_get_matches_from(&args) {
            Ok(matches) => matches,
            Err(error) => {
                if error.exit_code() != 0 {
                    terminal.diagnostic(
                        &Diagnostic::new(DiagnosticCode::InvalidInput)
                            .with_details(
                                serde_json::json!({"parser_kind":format!("{:?}",error.kind())}),
                            )
                            .with_retryable(false)
                            .with_recovery(
                                "query_help",
                                "Query help for the command's accepted arguments.",
                            )
                            .into(),
                    );
                    return Ok(error.exit_code());
                }
                error.print()?;
                return Ok(error.exit_code());
            }
        };
        let Some((name, sub)) = matches.subcommand() else {
            parser.print_help()?;
            println!();
            return Ok(0);
        };
        if name == "help" {
            let path: Vec<String> = sub
                .get_many::<String>("command")
                .map(|parts| parts.cloned().collect())
                .unwrap_or_default();
            if json {
                println!("{}", serde_json::to_string(&self.describe(&path)?)?);
            } else {
                let mut target = &mut parser;
                for part in &path {
                    target = target
                        .find_subcommand_mut(part)
                        .ok_or_else(|| Error::Invalid(format!("unknown command: {part}")))?;
                }
                target.print_long_help()?;
                println!();
            }
            return Ok(0);
        }
        let report_file = matches
            .get_one::<String>("report")
            .map(|path| {
                let mut options = std::fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let file = options.open(path)?;
                Ok::<_, std::io::Error>((std::fs::canonicalize(path)?, file))
            })
            .transpose()?;
        let mut report_file = report_file;
        let mut input: Value = matches
            .get_one::<String>("input")
            .map(|s| serde_json::from_str(s))
            .transpose().map_err(|error: serde_json::Error| Diagnostic::new(DiagnosticCode::InvalidInput)
                .with_details(serde_json::json!({"field":"input","line":error.line(),"column":error.column()})))?
            .unwrap_or_else(|| serde_json::json!({}));
        let object = input.as_object_mut().ok_or_else(|| {
            Diagnostic::new(DiagnosticCode::InvalidInput)
                .with_details(serde_json::json!({"field":"input","expected":"object"}))
        })?;
        let workflow;
        if name == "run" {
            workflow = sub
                .get_one::<String>("workflow")
                .ok_or_else(|| Error::Invalid("workflow is required".into()))?
                .clone();
        } else {
            let (path, leaf) = command_path(name, sub);
            let spec = self
                .commands
                .iter()
                .find(|s| s.path == path)
                .ok_or_else(|| Error::Invalid("unknown command path".into()))?;
            workflow = spec.workflow.clone();
            for arg in &spec.arguments {
                match arg.kind {
                    ArgumentKind::Text => {
                        if let Some(value) = leaf.get_one::<String>(&arg.name) {
                            object.insert(arg.name.clone(), Value::String(value.clone()));
                        }
                    }
                    ArgumentKind::List => {
                        if let Some(values) = leaf.get_many::<String>(&arg.name) {
                            object.insert(
                                arg.name.clone(),
                                Value::Array(
                                    values.map(|value| Value::String(value.clone())).collect(),
                                ),
                            );
                        }
                    }
                    ArgumentKind::Flag => {
                        if leaf.get_flag(&arg.name) {
                            object.insert(arg.name.clone(), Value::Bool(true));
                        }
                    }
                }
            }
        }
        let answers = if let Some(path) = matches.get_one::<String>("answers") {
            serde_json::from_slice(&std::fs::read(path)?).map_err(|error| Diagnostic::new(DiagnosticCode::InvalidInput)
                .with_details(serde_json::json!({"field":"answers","line":error.line(),"column":error.column()})))?
        } else {
            BTreeMap::new()
        };
        let mut session =
            self.engine
                .session(&workflow, &Self::project_from_args(&args)?, input, answers)?;
        let interactive = options.interactive();
        let (sink, mut events) = EventSink::channel();
        session.observe(sink);
        let yes = matches.get_flag("yes");
        let mut history: Vec<Question> = Vec::new();
        loop {
            let report = {
                let advancing = session.advance(&self.engine);
                tokio::pin!(advancing);
                loop {
                    tokio::select! {
                        Some(event) = events.next() => terminal.event(event)?,
                        result = &mut advancing => {
                            while let Some(event) = events.try_next() { terminal.event(event)?; }
                            break result;
                        }
                    }
                }
            };
            let report = match report {
                Ok(report) => report,
                Err(error) => {
                    if let Some((path, file)) = &mut report_file {
                        let mut diagnostic =
                            error.diagnostic().with_report(path.display().to_string());
                        diagnostic.exit_code = error.exit_code();
                        let mut full = serde_json::to_value(&diagnostic)?;
                        full["outputs"] = serde_json::to_value(session.outputs())?;
                        save_report(file, &full, &session.summary())?;
                        return Err(diagnostic.into());
                    }
                    return Err(error);
                }
            };
            terminal.stop();
            terminal.review(&report);
            let Some(question) = &report.question else {
                if json {
                    print_report(
                        &report,
                        &session.summary(),
                        matches.get_flag("summary"),
                        &mut report_file,
                    )?;
                } else {
                    terminal.complete(&report)?;
                }
                return Ok(0);
            };
            if yes && question.kind == QuestionKind::Confirm {
                session.answer(&question.id, Value::Bool(true))?;
                continue;
            }
            if !interactive {
                if json {
                    print_report(
                        &report,
                        &session.summary(),
                        matches.get_flag("summary"),
                        &mut report_file,
                    )?;
                } else {
                    terminal.needs_input(question);
                }
                return Ok(2);
            }
            let mut current = question.clone();
            loop {
                match terminal.ask(&current)? {
                    Prompt::Back => {
                        current = history.pop().ok_or(Error::Cancelled)?;
                    }
                    Prompt::Answer(value) => {
                        session.answer(&current.id, value)?;
                        history.push(current);
                        break;
                    }
                }
            }
        }
    }
}

fn save_report(
    file: &mut std::fs::File,
    full: &Value,
    execution: &crate::ExecutionSummary,
) -> Result<(), Error> {
    let result = serde_json::to_writer(&mut *file, full)
        .map_err(Error::from)
        .and_then(|()| file.sync_all().map_err(Error::from));
    result.map_err(|error| Error::Execution {
        source: Box::new(Diagnostic::new(DiagnosticCode::IoError)
            .with_details(serde_json::json!({"report_saved":false,"operation_may_have_completed":true,"failure":error.diagnostic(),
                "outcome":if full["type"] == "diagnostic" { serde_json::json!({"code":full["code"],"details":full["details"]}) } else { serde_json::json!({"status":full["status"]}) }}))
            .with_retryable(false)
            .with_recovery("inspect_execution", "Inspect the execution receipt before retrying; saving the report failed after execution.").into()),
        summary: Box::new(execution.clone()),
    })
}

fn print_report(
    report: &crate::Report,
    execution: &crate::ExecutionSummary,
    summary: bool,
    report_file: &mut Option<(PathBuf, std::fs::File)>,
) -> Result<(), Error> {
    let mut full = serde_json::to_value(report)?;
    if report.status == crate::Status::NeedsInput {
        full["code"] = serde_json::json!("missing_input");
    }
    full["execution"] = serde_json::to_value(execution)?;
    if let Some((path, file)) = report_file {
        full["report"] = serde_json::json!(path);
        save_report(file, &full, execution)?;
    }
    if summary {
        full.as_object_mut()
            .expect("report serializes as an object")
            .remove("outputs");
        full["mode"] = serde_json::json!("summary");
        full["schema_version"] = serde_json::json!(1);
    }
    println!("{}", serde_json::to_string(&full)?);
    Ok(())
}

fn command_path<'a>(name: &str, matches: &'a ArgMatches) -> (Vec<String>, &'a ArgMatches) {
    let mut path = vec![name.to_owned()];
    let mut leaf = matches;
    while let Some((name, sub)) = leaf.subcommand() {
        path.push(name.to_owned());
        leaf = sub;
    }
    (path, leaf)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn report_write_failure_keeps_the_original_process_failure_and_receipt() {
        let mut directory = std::fs::File::open(".").unwrap();
        let execution = crate::ExecutionSummary {
            id: "execution".into(),
            workflow: "test".into(),
            status: "failed".into(),
            completed_steps: vec!["first".into()],
            calls: vec![],
            verification: vec![],
            verification_status: "not_declared".into(),
            application_verification: "not_reported",
        };
        let mut full = serde_json::to_value(Error::ProcessExit(7).diagnostic()).unwrap();
        full["outputs"] = serde_json::json!({"private":"secret-output"});
        let error = save_report(&mut directory, &full, &execution).unwrap_err();
        let value = serde_json::to_value(error.diagnostic()).unwrap();
        assert_eq!(value["code"], "io_error");
        assert_eq!(value["details"]["report_saved"], false);
        assert_eq!(value["details"]["outcome"]["code"], "process_failed");
        assert_eq!(value["details"]["outcome"]["details"]["exit_code"], 7);
        assert_eq!(
            value["execution"]["completed_steps"],
            serde_json::json!(["first"])
        );
        assert!(!value.to_string().contains("secret-output"));
    }
}
