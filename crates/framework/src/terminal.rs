use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

use console::Style;
use indicatif::ProgressBar;
use inquire::{Confirm, InquireError, MultiSelect, Select, Text, ui::RenderConfig};
use serde_json::Value;

use crate::{Error, ExecutionEvent, Question, QuestionKind, Report};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UiOptions {
    pub json: bool,
    pub color: ColorMode,
    pub debug: bool,
    pub no_interactive: bool,
}

impl UiOptions {
    pub fn from_args(args: &[OsString]) -> Self {
        let mut options = Self::default();
        let mut args = args.iter().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            if ["--project", "--input", "--answers", "--report"]
                .iter()
                .any(|flag| arg == *flag)
            {
                args.next();
                continue;
            }
            if arg == "--json" {
                options.json = true;
            }
            if arg == "--debug" {
                options.debug = true;
            }
            if arg == "--no-interactive" {
                options.no_interactive = true;
            }
            let value = if arg == "--color" {
                args.next().and_then(|s| s.to_str())
            } else {
                arg.to_str().and_then(|s| s.strip_prefix("--color="))
            };
            if let Some(value) = value {
                options.color = match value {
                    "always" => ColorMode::Always,
                    "never" => ColorMode::Never,
                    _ => ColorMode::Auto,
                };
            }
        }
        options
    }

    pub fn interactive(self) -> bool {
        let ci = std::env::var("CI").is_ok_and(|s| !s.is_empty() && s != "false" && s != "0");
        !self.json
            && !self.no_interactive
            && !ci
            && io::stdin().is_terminal()
            && io::stderr().is_terminal()
            && std::env::var("TERM").as_deref() != Ok("dumb")
    }

    fn colors(self, tty: bool) -> bool {
        if self.json {
            return false;
        }
        match self.color {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => {
                tty && std::env::var_os("NO_COLOR").is_none()
                    && std::env::var("TERM").as_deref() != Ok("dumb")
            }
        }
    }
}

pub(crate) enum Prompt {
    Back,
    Answer(Value),
}

struct PromptColors(bool);

impl PromptColors {
    fn new(enabled: bool) -> Self {
        // Inquire's Crossterm backend separately caches NO_COLOR. Restore the
        // caller's setting after this synchronous prompt, including early errors.
        let previous = crossterm::style::Colored::ansi_color_disabled_memoized();
        crossterm::style::force_color_output(enabled);
        Self(previous)
    }
}

impl Drop for PromptColors {
    fn drop(&mut self) {
        crossterm::style::Colored::set_ansi_color_disabled(self.0);
    }
}

#[derive(Debug, Clone)]
struct Review {
    id: String,
    diff: String,
    files: Vec<(String, bool)>,
    commands: Vec<String>,
}

impl Review {
    fn from_output(output: &Value) -> Option<Self> {
        let plan = output.get("plan")?;
        let files = plan
            .get("files")?
            .as_array()?
            .iter()
            .filter_map(|file| {
                let path = file.get("path")?.as_str()?;
                let exists =
                    plan.get("snapshots")
                        .and_then(Value::as_array)
                        .is_some_and(|snapshots| {
                            snapshots.iter().any(|before| {
                                before["path"] == path && !before["content"].is_null()
                            })
                        });
                Some((path.to_owned(), exists))
            })
            .collect();
        let commands = plan
            .get("commands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|command| {
                let program = command.get("program")?.as_str()?;
                let args = command.get("args")?.as_array()?;
                Some(
                    std::iter::once(program)
                        .chain(args.iter().filter_map(Value::as_str))
                        .map(quote_arg)
                        .collect::<Vec<_>>()
                        .join(" "),
                )
            })
            .collect();
        Some(Self {
            id: output.get("id")?.as_str()?.to_owned(),
            diff: output.get("diff")?.as_str()?.to_owned(),
            files,
            commands,
        })
    }
}

/// Terminal presentation is isolated from engine execution and JSON serialization.
pub(crate) struct Terminal {
    options: UiOptions,
    progress: Option<ProgressBar>,
    started: Instant,
    review: Option<Review>,
}

impl Terminal {
    pub fn new(options: UiOptions) -> Self {
        Self {
            options,
            progress: None,
            started: Instant::now(),
            review: None,
        }
    }

    fn stderr_style(&self, style: Style) -> Style {
        style
            .for_stderr()
            .force_styling(self.options.colors(io::stderr().is_terminal()))
    }
    fn stdout_style(&self, style: Style) -> Style {
        style
            .for_stdout()
            .force_styling(self.options.colors(io::stdout().is_terminal()))
    }

    pub fn event(&mut self, event: ExecutionEvent) -> Result<(), Error> {
        match event {
            ExecutionEvent::Preparing { message } if !self.options.json => {
                self.stop();
                if self.options.interactive() {
                    let bar = ProgressBar::new_spinner();
                    bar.set_message(printable(&message));
                    bar.enable_steady_tick(Duration::from_millis(100));
                    self.progress = Some(bar);
                }
            }
            ExecutionEvent::Log { bytes } => {
                let cleaned;
                let bytes = if self.options.json
                    || self.options.colors(io::stderr().is_terminal())
                    || !bytes.contains(&0x1b)
                {
                    bytes.as_slice()
                } else {
                    cleaned =
                        console::strip_ansi_codes(&String::from_utf8_lossy(&bytes)).into_owned();
                    cleaned.as_bytes()
                };
                if let Some(progress) = &self.progress {
                    progress.suspend(|| io::stderr().write_all(bytes))?;
                } else {
                    io::stderr().write_all(bytes)?;
                }
            }
            ExecutionEvent::Started { id } if !self.options.json => {
                self.stop();
                if self.options.interactive() {
                    let bar = ProgressBar::new_spinner();
                    bar.set_message(format!("Running {}", printable(&id)));
                    bar.enable_steady_tick(Duration::from_millis(100));
                    self.progress = Some(bar);
                }
            }
            ExecutionEvent::Completed { id } if !self.options.json => {
                self.stop();
                eprintln!(
                    "{} {}",
                    self.stderr_style(Style::new().green()).apply_to("✓"),
                    printable(&id)
                );
            }
            _ => {}
        }
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(bar) = self.progress.take() {
            bar.finish_and_clear();
        }
    }

    pub fn review(&mut self, report: &Report) {
        if self.options.json {
            return;
        }
        let Some(review) = report.outputs.values().find_map(Review::from_output) else {
            return;
        };
        if self
            .review
            .as_ref()
            .is_some_and(|previous| previous.id == review.id)
        {
            return;
        }
        eprintln!(
            "\n{}",
            self.stderr_style(Style::new().bold()).apply_to("Changes")
        );
        for (path, existed) in &review.files {
            let marker = if *existed { "~" } else { "+" };
            let style = if *existed {
                Style::new().yellow()
            } else {
                Style::new().green()
            };
            eprintln!(
                "  {} {}",
                self.stderr_style(style).apply_to(marker),
                printable(path)
            );
        }
        if review.files.is_empty() {
            eprintln!("  No file changes");
        }
        if !review.commands.is_empty() {
            eprintln!(
                "\n{}",
                self.stderr_style(Style::new().bold())
                    .apply_to("External commands")
            );
            for command in &review.commands {
                eprintln!("  {}", printable(command));
            }
            eprintln!(
                "{}",
                self.stderr_style(Style::new().dim()).apply_to(
                    "  Run after file changes; may update lockfiles, packages, and caches."
                )
            );
        }
        eprintln!();
        self.review = Some(review);
    }

    fn render_config(&self) -> RenderConfig<'static> {
        if self.options.colors(io::stderr().is_terminal()) {
            RenderConfig::default_colored()
        } else {
            RenderConfig::empty()
        }
    }

    pub fn ask(&mut self, question: &Question) -> Result<Prompt, Error> {
        self.stop();
        let _colors = PromptColors::new(self.options.colors(io::stderr().is_terminal()));
        let help = "↑/↓ move · type to filter · Enter select · Esc back · Ctrl-C cancel";
        let config = self.render_config();
        if let Some(review) = self.review.clone()
            && question.id == format!("apply:{}", review.id)
            && question.kind == QuestionKind::Confirm
        {
            loop {
                match Select::new(
                    "Review file changes",
                    vec!["Apply file changes", "View full diff", "Cancel"],
                )
                .with_render_config(config)
                .with_help_message(help)
                .prompt()
                {
                    Ok("Apply file changes") => return Ok(Prompt::Answer(Value::Bool(true))),
                    Ok("Cancel") => return Ok(Prompt::Answer(Value::Bool(false))),
                    Ok(_) => self.print_diff(
                        &review.diff,
                        &mut io::stderr(),
                        self.stderr_style(Style::new()),
                    )?,
                    Err(error) => return prompt_error(error),
                }
            }
        }
        let message = if self
            .review
            .as_ref()
            .is_some_and(|review| question.id == format!("commands:{}", review.id))
        {
            "Authorize the listed external commands?".to_owned()
        } else {
            printable(&question.message)
        };
        let value = match &question.kind {
            QuestionKind::Text => Text::new(&message)
                .with_render_config(config)
                .with_help_message("Enter submit · Esc back · Ctrl-C cancel")
                .with_validator(|value: &str| {
                    if value.trim().is_empty() {
                        Ok(inquire::validator::Validation::Invalid(
                            "Enter a nonempty value".into(),
                        ))
                    } else {
                        Ok(inquire::validator::Validation::Valid)
                    }
                })
                .prompt()
                .map(Value::String),
            QuestionKind::Confirm => Confirm::new(&message)
                .with_default(false)
                .with_render_config(config)
                .with_help_message("y/n · Enter declines · Esc back · Ctrl-C cancel")
                .prompt()
                .map(Value::Bool),
            QuestionKind::Select { options } => {
                let choices = choices(options);
                Select::new(&message, choices)
                    .with_render_config(config)
                    .with_help_message(help)
                    .prompt()
                    .map(|choice| Value::String(options[choice.index].clone()))
            }
            QuestionKind::MultiSelect { options } => MultiSelect::new(&message, choices(options))
                .with_render_config(config)
                .with_help_message(
                    "↑/↓ move · Space toggle · type to filter · Enter submit · Esc back",
                )
                .prompt()
                .map(|selected| {
                    Value::Array(
                        selected
                            .into_iter()
                            .map(|choice| Value::String(options[choice.index].clone()))
                            .collect(),
                    )
                }),
        };
        match value {
            Ok(Value::String(value)) if value == ":back" => Ok(Prompt::Back),
            Ok(value) => {
                question.validate(&value)?;
                Ok(Prompt::Answer(value))
            }
            Err(error) => prompt_error(error),
        }
    }

    pub fn needs_input(&self, question: &Question) {
        eprintln!(
            "{}\nSupply --answers FILE or explicit command arguments.",
            printable(&question.message)
        );
    }

    pub fn complete(&mut self, report: &Report) -> Result<(), Error> {
        self.stop();
        let mut stdout = io::stdout().lock();
        let applied: Vec<_> = report
            .outputs
            .values()
            .filter_map(|output| output.get("applied").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let has_apply = report
            .outputs
            .values()
            .any(|output| output.get("applied").is_some());
        if !has_apply && let Some(review) = &self.review {
            self.print_diff(&review.diff, &mut stdout, self.stdout_style(Style::new()))?;
            writeln!(
                stdout,
                "{}",
                self.stdout_style(Style::new().cyan())
                    .apply_to("Preview only · no changes applied")
            )?;
        } else {
            writeln!(
                stdout,
                "{} Completed {} in {:.1}s",
                self.stdout_style(Style::new().green()).apply_to("✓"),
                self.stdout_style(Style::new().bold())
                    .apply_to(printable(&report.workflow)),
                self.started.elapsed().as_secs_f32()
            )?;
            if has_apply {
                let commands = report
                    .outputs
                    .values()
                    .filter(|output| output.get("applied").is_some())
                    .filter_map(|output| output.get("commands").and_then(Value::as_array))
                    .map(Vec::len)
                    .sum::<usize>();
                writeln!(
                    stdout,
                    "  {} {} changed · {} {} completed",
                    applied.len(),
                    if applied.len() == 1 { "file" } else { "files" },
                    commands,
                    if commands == 1 { "command" } else { "commands" }
                )?;
                for path in applied {
                    writeln!(stdout, "  {}", printable(path))?;
                }
                if let Some(verified) = report
                    .outputs
                    .values()
                    .find_map(|output| output.get("verified").and_then(Value::as_str))
                {
                    writeln!(stdout, "  Verified {}", printable(verified))?;
                }
            } else {
                for output in report.outputs.values() {
                    if let Some(rows) = output.get("results").and_then(Value::as_array) {
                        for row in rows {
                            writeln!(
                                stdout,
                                "  {}  {}",
                                self.stdout_style(Style::new().cyan().bold())
                                    .apply_to(printable(row["add"].as_str().unwrap_or(""))),
                                printable(row["type"].as_str().unwrap_or(""))
                            )?;
                            if let Some(description) =
                                row["description"].as_str().filter(|s| !s.is_empty())
                            {
                                writeln!(stdout, "    {}", printable(description))?;
                            }
                        }
                        let page = &output["pagination"];
                        writeln!(
                            stdout,
                            "  {} results · {} total · offset {}{}",
                            rows.len(),
                            page["total"],
                            page["offset"],
                            if page["hasMore"] == true {
                                " · more available"
                            } else {
                                ""
                            }
                        )?;
                        continue;
                    }
                    if let Some(message) = output
                        .get("message")
                        .or_else(|| output.get("greeting"))
                        .and_then(Value::as_str)
                    {
                        writeln!(stdout, "  {}", printable(message))?;
                        continue;
                    }
                    if let Some(object) = output.as_object() {
                        for (key, value) in object
                            .iter()
                            .filter(|(key, value)| {
                                !key.starts_with('_')
                                    && (value.is_string()
                                        || value.is_number()
                                        || value.is_boolean())
                            })
                            .take(8)
                        {
                            writeln!(
                                stdout,
                                "  {}: {}",
                                printable(key),
                                printable(
                                    value
                                        .as_str()
                                        .map_or_else(|| value.to_string(), str::to_owned)
                                        .as_str()
                                )
                            )?;
                        }
                    } else if !output.is_null() {
                        writeln!(stdout, "  {}", printable(&output.to_string()))?;
                    }
                }
            }
        }
        if let Some(summary) = report
            .outputs
            .values()
            .find_map(|output| output.get("summary"))
        {
            writeln!(
                stdout,
                "  {} checked · {} changed · {} unchanged · {} kept",
                summary["checked"], summary["changed"], summary["unchanged"], summary["kept"]
            )?;
        }
        let notes: std::collections::BTreeSet<_> = report
            .outputs
            .values()
            .filter_map(|output| output.get("docs").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .filter(|note| !note.trim().is_empty())
            .collect();
        if !notes.is_empty() {
            writeln!(
                stdout,
                "\n{}",
                self.stdout_style(Style::new().bold()).apply_to("Notes")
            )?;
            for note in notes {
                writeln!(stdout, "{}", printable(note))?;
            }
        }
        if self.options.debug {
            eprintln!("Debug report:\n{}", serde_json::to_string_pretty(report)?);
        }
        Ok(())
    }

    fn print_diff(&self, diff: &str, out: &mut impl Write, base: Style) -> Result<(), Error> {
        // Command requests have their own summary; the diff view contains file changes only.
        let diff = diff
            .split("\nExternal commands (run after file changes):")
            .next()
            .unwrap_or(diff);
        for line in diff.lines() {
            let style =
                if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") {
                    base.clone().cyan()
                } else if line.starts_with('+') {
                    base.clone().green()
                } else if line.starts_with('-') {
                    base.clone().red()
                } else {
                    base.clone()
                };
            writeln!(out, "{}", style.apply_to(printable(line)))?;
        }
        Ok(())
    }

    pub fn diagnostic(&mut self, error: &Error) {
        self.stop();
        let diagnostic = error.diagnostic();
        if self.options.json {
            println!("{}", serde_json::json!(diagnostic));
        } else if diagnostic.code() == crate::DiagnosticCode::Cancelled {
            eprintln!(
                "{} Cancelled",
                self.stderr_style(Style::new().yellow()).apply_to("!")
            );
        } else {
            eprintln!(
                "{} {}",
                self.stderr_style(Style::new().red().bold())
                    .apply_to("Error:"),
                printable(&diagnostic.to_string())
            );
            if diagnostic
                .details()
                .as_object()
                .is_some_and(|details| !details.is_empty())
            {
                eprintln!("  {}", printable(&diagnostic.details().to_string()));
            }
            if let Some(recovery) = diagnostic.recovery() {
                eprintln!("  {}", printable(&recovery.message));
            }
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
struct Choice {
    index: usize,
    label: String,
}
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
fn choices(options: &[String]) -> Vec<Choice> {
    options
        .iter()
        .enumerate()
        .map(|(index, value)| Choice {
            index,
            label: printable(value),
        })
        .collect()
}
fn prompt_error(error: InquireError) -> Result<Prompt, Error> {
    match error {
        InquireError::OperationCanceled => Ok(Prompt::Back),
        InquireError::OperationInterrupted => Err(Error::Cancelled),
        other => Err(Error::Operation(other.to_string())),
    }
}
fn printable(value: &str) -> String {
    console::strip_ansi_codes(value)
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}
fn quote_arg(value: &str) -> String {
    if !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/=@%+,".contains(&b))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
