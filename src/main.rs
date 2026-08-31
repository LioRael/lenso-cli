mod app;
mod archive;
mod catalog;
mod doctor;
mod plugin;
mod plugins;
mod watch;

use std::{env, path::PathBuf, process::Command};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

/// Create and run Lenso Apps made only from Plugins.
#[derive(Debug, Parser)]
#[command(
    name = "lenso",
    version,
    about = "Create and run Lenso Apps made only from Plugins",
    after_help = "Other root commands are delegated to the current Host's terminal Plugins.",
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
    /// Diagnose the current Host, Plugin Root, toolchain, and App resolution.
    Doctor(doctor::DoctorArgs),
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
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    reject_retired_invocation(&arguments)?;
    if should_delegate_to_host(&arguments) {
        return run_host_command(arguments);
    }
    match Cli::parse().command {
        RootCommand::Plugin { command } => plugin::plugin(command).await,
        RootCommand::Plugins { command } => plugins::plugins(command),
        RootCommand::App { command } => app::app(command),
        RootCommand::Run(args) => run(args),
        RootCommand::Doctor(args) => doctor::doctor(args),
    }
}

fn reject_retired_invocation(arguments: &[String]) -> anyhow::Result<()> {
    if arguments.iter().any(|argument| argument == "--definition") {
        anyhow::bail!(
            "`--definition` is retired: the current Host plus `plugins/` derive the App; use `lenso app check` or `lenso app show`"
        );
    }
    match arguments {
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
        [app, resolve, ..] if app == "app" && resolve == "resolve" => {
            anyhow::bail!(
                "`lenso app resolve` is retired: `app check`, `app show`, and `run` derive the App directly from the current Host and `plugins/`"
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

fn should_delegate_to_host(arguments: &[String]) -> bool {
    let Some(first) = arguments.first().map(String::as_str) else {
        return false;
    };
    !first.starts_with('-') && !matches!(first, "plugin" | "plugins" | "app" | "run" | "doctor")
}

fn run_host_command(arguments: Vec<String>) -> anyhow::Result<()> {
    run_current_host(arguments)
}

fn run(args: RunArgs) -> anyhow::Result<()> {
    let mut arguments = vec!["run".to_owned()];
    arguments.extend(args.host_args);
    run_current_host_at(args.root, arguments)
}

fn run_current_host(arguments: Vec<String>) -> anyhow::Result<()> {
    run_current_host_at(None, arguments)
}

fn run_current_host_at(root: Option<PathBuf>, arguments: Vec<String>) -> anyhow::Result<()> {
    let root = plugins::project_root(root)?;
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
        .args(arguments)
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
    use clap::{CommandFactory, Parser};

    #[test]
    fn public_command_tree_contains_only_plugin_app_owner_and_run_workflows() {
        let command = Cli::command();
        let names = command
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["plugin", "plugins", "app", "run", "doctor"]);

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
            ["init", "check", "show"]
        );
    }

    #[test]
    fn root_help_explains_dynamic_app_command_delegation() {
        let help = Cli::command().render_long_help().to_string();
        assert!(help.contains("delegated to the current Host's terminal Plugins"));
    }

    #[test]
    fn static_maintenance_roots_stay_local_and_app_roots_delegate() {
        for command in ["plugin", "plugins", "app", "run", "doctor"] {
            assert!(!should_delegate_to_host(&[command.to_owned()]));
        }
        for argument in ["--help", "-h", "--version", "-V"] {
            assert!(!should_delegate_to_host(&[argument.to_owned()]));
        }
        assert!(should_delegate_to_host(&["sessions".to_owned()]));
        assert!(should_delegate_to_host(&[
            "project".to_owned(),
            "status".to_owned(),
            "--json".to_owned(),
        ]));
    }

    #[test]
    fn retired_roots_fail_before_host_delegation() {
        for command in ["module", "new", "dev", "check", "verify"] {
            let error = reject_retired_invocation(&[command.to_owned()]).unwrap_err();
            assert!(error.to_string().contains("retired"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn app_commands_are_forwarded_unchanged_to_the_validated_current_host() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        use lenso_app_plan::authoring::HostCatalog;

        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        fs::create_dir(root.join(".lenso")).unwrap();
        fs::create_dir(root.join("plugins")).unwrap();
        fs::write(
            root.join(".lenso/host-catalog.json"),
            serde_json::to_vec(&HostCatalog::new([], [], [])).unwrap(),
        )
        .unwrap();
        let host = root.join(".lenso/host");
        fs::write(
            &host,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > .lenso/forwarded-args\npwd > .lenso/forwarded-cwd\n",
        )
        .unwrap();
        fs::set_permissions(&host, fs::Permissions::from_mode(0o755)).unwrap();

        run_current_host_at(
            Some(root.to_path_buf()),
            ["sessions", "show", "s-123", "--json"]
                .map(str::to_owned)
                .to_vec(),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(root.join(".lenso/forwarded-args")).unwrap(),
            "sessions\nshow\ns-123\n--json\n"
        );
        assert_eq!(
            fs::canonicalize(root).unwrap(),
            fs::canonicalize(
                fs::read_to_string(root.join(".lenso/forwarded-cwd"))
                    .unwrap()
                    .trim()
            )
            .unwrap()
        );
    }

    #[test]
    fn plugin_new_accepts_web_as_an_explicit_authoring_path() {
        let parsed =
            Cli::try_parse_from(["lenso", "plugin", "new", "company.greetings-http", "--web"])
                .unwrap();
        let RootCommand::Plugin {
            command: plugin::PluginCommand::New(arguments),
        } = parsed.command
        else {
            panic!("expected plugin new");
        };
        assert!(arguments.web);

        assert!(
            Cli::try_parse_from([
                "lenso",
                "plugin",
                "new",
                "company.greetings-http",
                "--web",
                "--runtime",
                "wasm",
            ])
            .is_err()
        );
    }
}
