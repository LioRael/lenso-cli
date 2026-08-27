mod authoring;
mod module;
mod plugin;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Create, develop, check, and package Lenso Plugins.
#[derive(Debug, Parser)]
#[command(
    name = "lenso",
    version,
    about = "Create, develop, check, and package Lenso Plugins",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create, develop, check, and package an installable Plugin.
    Plugin {
        #[command(subcommand)]
        command: plugin::PluginCommand,
    },
    /// Temporary compatibility workflow for legacy built-in behavior.
    #[command(hide = true)]
    Module {
        #[command(subcommand)]
        command: ModuleCommand,
    },
    /// Check and resolve source-derived App Definitions.
    App {
        #[command(subcommand)]
        command: authoring::AppCommand,
    },
    /// Create a new standalone Module project.
    #[command(hide = true)]
    New(ModuleCreateArgs),
    /// Start the Module development loop in the current project.
    #[command(hide = true)]
    Dev(ModuleDevArgs),
    /// Diagnose the current Module project with actionable checks.
    #[command(hide = true)]
    Check(ModuleCheckArgs),
    /// Prove Module behavior, lifecycle, composition, and removal.
    #[command(hide = true)]
    Verify(ModuleVerifyArgs),
}

#[derive(Debug, Subcommand)]
enum ModuleCommand {
    /// Create a new standalone built-in Module project.
    New(ModuleCreateArgs),
    /// Start the Module development loop in the current project.
    Dev(ModuleDevArgs),
    /// Diagnose the current Module project with actionable checks.
    Check(ModuleCheckArgs),
    /// Prove Module behavior, lifecycle, composition, and removal.
    Verify(ModuleVerifyArgs),
}

#[derive(Debug, Args, Clone)]
struct ModuleCreateArgs {
    /// Module id, such as billing or support.
    module_id: String,

    /// Base directory for the new standalone Module project.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Module implementation runtime.
    #[arg(long, value_enum, default_value_t = ModuleRuntimeArg::Rust)]
    runtime: ModuleRuntimeArg,

    /// High-value authoring recipe used to seed the Module card.
    #[arg(long, value_enum, default_value_t = ModuleRecipeArg::Stateless)]
    recipe: ModuleRecipeArg,

    /// New standalone Module project directory. Defaults to the Module id.
    #[arg(long)]
    dir: Option<std::path::PathBuf>,

    /// Skip dependency installation and compile checks after generation.
    #[arg(long)]
    no_install: bool,

    /// Capability id provided by the generated Module.
    #[arg(long)]
    capability: Option<String>,

    /// Print files without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ModuleRuntimeArg {
    Rust,
    Bun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum ModuleRecipeArg {
    Stateless,
    Stateful,
    WebConsole,
    ManagedWork,
}

#[derive(Debug, Args, Clone)]
struct ModuleDevArgs {
    /// Module repository root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Authoring project document, relative to the Module repository root.
    #[arg(long, default_value = "lenso.json")]
    project: std::path::PathBuf,

    /// Bun executable used by the Execution Adapter when the project requires Bun.
    #[arg(long = "bun-bin", default_value = "bun")]
    bun_bin: String,
}

#[derive(Debug, Args, Clone)]
struct ModuleCheckArgs {
    /// Module repository root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Authoring project document, relative to the Module repository root.
    #[arg(long, default_value = "lenso.json")]
    project: std::path::PathBuf,

    /// Emit the versioned authoring report as JSON.
    #[arg(long)]
    json: bool,

    /// Regenerate source-derived Capability snapshots before checking them.
    #[arg(long)]
    update_contracts: bool,
}

#[derive(Debug, Args, Clone)]
struct ModuleVerifyArgs {
    /// Module repository root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<std::path::PathBuf>,

    /// Authoring project document, relative to the Module repository root.
    #[arg(long, default_value = "lenso.json")]
    project: std::path::PathBuf,

    /// Verify removal of this App-local Module Instance. Defaults to every Instance.
    #[arg(long = "module")]
    module_key: Option<String>,

    /// Behavior verification manifest, relative to the Module repository root.
    #[arg(long, default_value = "lenso.module.verify.json")]
    manifest: std::path::PathBuf,

    /// Write the versioned verification evidence to this path.
    #[arg(long, default_value = ".lenso/module-verification.json")]
    output: std::path::PathBuf,

    /// Emit the versioned verification evidence as JSON.
    #[arg(long)]
    json: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Plugin { command } => plugin::plugin(command).await?,
        Command::Module { command } => {
            eprintln!("{}", module_compatibility_warning());
            run_module_command(command).await?;
        }
        Command::App { command } => authoring::app(command)?,
        Command::New(args) => {
            eprintln!("{}", compatibility_warning("new", "module new"));
            create_module(args)?;
        }
        Command::Dev(args) => {
            eprintln!("{}", compatibility_warning("dev", "module dev"));
            dev_module(args).await?;
        }
        Command::Check(args) => {
            eprintln!("{}", compatibility_warning("check", "module check"));
            check_module(args)?;
        }
        Command::Verify(args) => {
            eprintln!("{}", compatibility_warning("verify", "module verify"));
            verify_module(args)?;
        }
    }

    Ok(())
}

async fn run_module_command(command: ModuleCommand) -> anyhow::Result<()> {
    match command {
        ModuleCommand::New(args) => create_module(args),
        ModuleCommand::Dev(args) => dev_module(args).await,
        ModuleCommand::Check(args) => check_module(args),
        ModuleCommand::Verify(args) => verify_module(args),
    }
}

fn compatibility_warning(old: &str, _: &str) -> String {
    format!(
        "warning: `lenso {old}` is retired Module compatibility; use `lenso plugin ...` for application behavior"
    )
}

fn module_compatibility_warning() -> &'static str {
    "warning: `lenso module ...` is hidden compatibility; Module is no longer a public application-behavior model"
}

fn create_module(args: ModuleCreateArgs) -> anyhow::Result<()> {
    module::create_module(&module::ModuleCreateOptions {
        capability: args.capability,
        dir: args.dir,
        dry_run: args.dry_run,
        module_id: args.module_id,
        no_install: args.no_install,
        repo_root: args.repo_root,
        recipe: match args.recipe {
            ModuleRecipeArg::Stateless => module::ModuleRecipe::Stateless,
            ModuleRecipeArg::Stateful => module::ModuleRecipe::Stateful,
            ModuleRecipeArg::WebConsole => module::ModuleRecipe::WebConsole,
            ModuleRecipeArg::ManagedWork => module::ModuleRecipe::ManagedWork,
        },
        runtime: match args.runtime {
            ModuleRuntimeArg::Rust => module::ModuleRuntime::Rust,
            ModuleRuntimeArg::Bun => module::ModuleRuntime::Bun,
        },
    })
}

async fn dev_module(args: ModuleDevArgs) -> anyhow::Result<()> {
    authoring::dev_module(args.repo_root.as_deref(), &args.project, &args.bun_bin).await
}

fn check_module(args: ModuleCheckArgs) -> anyhow::Result<()> {
    authoring::check_module(&authoring::ModuleCheckOptions {
        json: args.json,
        project: args.project,
        repo_root: args.repo_root,
        update_contracts: args.update_contracts,
    })
}

fn verify_module(args: ModuleVerifyArgs) -> anyhow::Result<()> {
    authoring::verify_module(authoring::ModuleVerifyOptions {
        json: args.json,
        manifest: args.manifest,
        module_key: args.module_key,
        output: args.output,
        project: args.project,
        repo_root: args.repo_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_tree_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn normal_help_exposes_only_plugin_and_app_namespaces() {
        let command = Cli::command();
        let names = command
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set())
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["plugin", "app"]);

        let app = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "app")
            .expect("app command");
        let app_names = app
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(app_names, ["add", "remove", "check", "resolve"]);

        let plugin = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "plugin")
            .expect("plugin command");
        let plugin_names = plugin
            .get_subcommands()
            .filter(|subcommand| !subcommand.is_hide_set())
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(plugin_names, ["new", "dev", "check", "pack"]);

        let module = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "module")
            .expect("module command");
        assert!(module.is_hide_set());
        let module_names = module
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(module_names, ["new", "dev", "check", "verify"]);
    }

    #[test]
    fn deprecated_aliases_are_hidden_and_actionable() {
        let command = Cli::command();
        for name in ["new", "dev", "check", "verify"] {
            assert!(
                command
                    .get_subcommands()
                    .find(|subcommand| subcommand.get_name() == name)
                    .expect("compatibility command")
                    .is_hide_set()
            );
        }
        assert_eq!(
            compatibility_warning("check", "module check"),
            "warning: `lenso check` is retired Module compatibility; use `lenso plugin ...` for application behavior"
        );
        assert!(module_compatibility_warning().contains("hidden compatibility"));
    }
}
