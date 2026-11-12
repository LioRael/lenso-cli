use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::future::Future;

use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::terminal::{ColorMode, Prompt, Terminal, UiOptions};
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
}

impl Cli {
    /// Construct a CLI with no project-specific commands.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            engine: Engine::new(),
            commands: Vec::new(),
        }
    }

    /// Access the registry for trusted native or sandboxed runtime adapters.
    pub fn engine_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }

    /// Register a command alias for an existing workflow.
    ///
    /// # Errors
    /// Rejects reserved names, malformed paths, duplicate flags, and alias conflicts.
    pub fn alias(&mut self, spec: CommandSpec) -> Result<(), Error> {
        const RESERVED: &[&str] = &["run", "help", "version"];
        const FLAGS: &[&str] = &[
            "help",
            "version",
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
            return Err(Error::Invalid(format!(
                "command conflict: {} ({}) and {} ({})",
                other.path.join(" "),
                other.provider,
                spec.path.join(" "),
                spec.provider
            )));
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
            if ["--project", "--input", "--answers", "--color"]
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
                match error {
                    Error::Cancelled => 130,
                    Error::ProcessExit(code) => code.clamp(1, 255),
                    _ => 1,
                }
            }
        }
    }

    /// Render discovery errors using the same output/color flags as execution.
    pub fn diagnostic(error: &Error, args: &[OsString]) {
        Terminal::new(UiOptions::from_args(args)).diagnostic(error);
    }

    fn parser(&self, options: UiOptions) -> Command {
        let mut parser = Command::new(self.name.clone())
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
                if json && error.exit_code() != 0 {
                    terminal.diagnostic(&Error::Invalid(error.to_string()));
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
        let mut input: Value = matches
            .get_one::<String>("input")
            .map(|s| serde_json::from_str(s))
            .transpose()?
            .unwrap_or_else(|| serde_json::json!({}));
        let object = input
            .as_object_mut()
            .ok_or_else(|| Error::Invalid("--input must be a JSON object".into()))?;
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
            serde_json::from_slice(&std::fs::read(path)?)?
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
                            break result?;
                        }
                    }
                }
            };
            terminal.stop();
            terminal.review(&report);
            let Some(question) = &report.question else {
                if json {
                    println!("{}", serde_json::to_string(&report)?);
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
                    println!("{}", serde_json::to_string(&report)?);
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

fn command_path<'a>(name: &str, matches: &'a ArgMatches) -> (Vec<String>, &'a ArgMatches) {
    let mut path = vec![name.to_owned()];
    let mut leaf = matches;
    while let Some((name, sub)) = leaf.subcommand() {
        path.push(name.to_owned());
        leaf = sub;
    }
    (path, leaf)
}
