mod app;
mod plugin;
mod plugins;

use std::{env, path::PathBuf, process::Command};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

/// Create and run Lenso Apps made only from Plugins.
#[derive(Debug, Parser)]
#[command(
    name = "lenso",
    version,
    about = "Create and run Lenso Apps made only from Plugins",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: RootCommand,
}

#[derive(Debug, Subcommand)]
enum RootCommand {
    /// Create, develop, check, and package one Plugin.
    Plugin {
        #[command(subcommand)]
        command: plugin::PluginCommand,
    },
    /// Inspect or change the current App's `plugins/` directory.
    Plugins {
        #[command(subcommand)]
        command: plugins::PluginsCommand,
    },
    /// Check or inspect the App derived by the current Host.
    App {
        #[command(subcommand)]
        command: app::AppCommand,
    },
    /// Start the current Host with its derived App.
    Run(RunArgs),
}

#[derive(Clone, Debug, Args)]
struct RunArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Arguments forwarded to the Host after `run`.
    #[arg(last = true)]
    host_args: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    reject_retired_invocation()?;
    match Cli::parse().command {
        RootCommand::Plugin { command } => plugin::plugin(command).await,
        RootCommand::Plugins { command } => plugins::plugins(command),
        RootCommand::App { command } => app::app(command),
        RootCommand::Run(args) => run(args),
    }
}

fn reject_retired_invocation() -> anyhow::Result<()> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.iter().any(|argument| argument == "--definition") {
        anyhow::bail!(
            "`--definition` is retired: the current Host plus `plugins/` derive the App; use `lenso app check` or `lenso app show`"
        );
    }
    match arguments.as_slice() {
        [command, ..]
            if ["module", "new", "dev", "check", "verify"].contains(&command.as_str()) =>
        {
            anyhow::bail!(
                "`lenso {command}` is retired: use `lenso plugin new|dev|check|pack`; Module is not an application behavior concept"
            );
        }
        [app, command, ..] if app == "app" && ["add", "remove"].contains(&command.as_str()) => {
            anyhow::bail!(
                "`lenso app {command}` is retired: change one Plugin with `lenso plugins add|configure|disable|enable|remove`"
            );
        }
        [plugin, verify, ..] if plugin == "plugin" && verify == "verify" => {
            anyhow::bail!(
                "`lenso plugin verify` is retired: `plugin pack` validates what it creates and `plugins add` independently verifies incoming bytes"
            );
        }
        _ => Ok(()),
    }
}

fn run(args: RunArgs) -> anyhow::Result<()> {
    let root = plugins::project_root(args.root)?;
    plugins::load_resolved_app(&root)?;
    let host = root.join(".lenso/host");
    let metadata = std::fs::symlink_metadata(&host)
        .with_context(|| format!("current Host is unavailable at {}", host.display()))?;
    if !metadata.file_type().is_file() {
        anyhow::bail!(
            "current Host must be a regular executable file: {}",
            host.display()
        );
    }
    let status = Command::new(&host)
        .arg("run")
        .args(args.host_args)
        .current_dir(&root)
        .status()
        .with_context(|| format!("start current Host {}", host.display()))?;
    if !status.success() {
        anyhow::bail!("Host exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn public_command_tree_contains_only_plugin_app_owner_and_run_workflows() {
        let command = Cli::command();
        let names = command
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["plugin", "plugins", "app", "run"]);

        let plugin = command
            .get_subcommands()
            .find(|command| command.get_name() == "plugin")
            .unwrap();
        assert_eq!(
            plugin
                .get_subcommands()
                .map(clap::Command::get_name)
                .collect::<Vec<_>>(),
            ["new", "dev", "check", "pack"]
        );

        let app = command
            .get_subcommands()
            .find(|command| command.get_name() == "app")
            .unwrap();
        assert_eq!(
            app.get_subcommands()
                .map(clap::Command::get_name)
                .collect::<Vec<_>>(),
            ["check", "show", "resolve"]
        );
    }
}
