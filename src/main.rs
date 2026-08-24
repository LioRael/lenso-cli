mod authoring;
mod module;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// Author, validate, resolve, and run Lenso applications and Modules.
#[derive(Debug, Parser)]
#[command(
    name = "lenso",
    version,
    about = "Author, validate, resolve, and run Lenso applications and Modules",
    propagate_version = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Add a Module package to an authoring project.
    Add(authoring::AddArgs),
    /// Validate an authoring project and its generated contracts.
    Check(authoring::CheckArgs),
    /// Resolve an authoring project into an immutable App Plan.
    Resolve(authoring::ResolveArgs),
    /// Run a resolved App Plan.
    Run(authoring::RunArgs),
    /// Expand reusable Project fragments into exact App variants.
    Compose {
        #[command(subcommand)]
        command: authoring::ComposeCommand,
    },
    /// Create and develop a Lenso Module.
    Module {
        #[command(subcommand)]
        command: ModuleCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ModuleCommand {
    /// Create a standalone Rust or Bun Module authoring project.
    Create(ModuleCreateArgs),
    /// Start the inferred Module development loop.
    Dev(ModuleDevArgs),
    /// Validate a Module authoring project with actionable diagnostics.
    Check(ModuleCheckArgs),
    /// Prove Module behavior declarations, composition, and removal.
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
        Command::Add(args) => authoring::add(&args)?,
        Command::Check(args) => authoring::check(&args)?,
        Command::Resolve(args) => authoring::resolve(&args)?,
        Command::Run(args) => authoring::run(&args).await?,
        Command::Compose { command } => authoring::compose(command).await?,
        Command::Module { command } => match command {
            ModuleCommand::Create(args) => {
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
                })?;
            }
            ModuleCommand::Dev(args) => {
                authoring::dev_module(args.repo_root.as_deref(), &args.project, &args.bun_bin)
                    .await?;
            }
            ModuleCommand::Check(args) => {
                authoring::check_module(&authoring::ModuleCheckOptions {
                    json: args.json,
                    project: args.project,
                    repo_root: args.repo_root,
                })?;
            }
            ModuleCommand::Verify(args) => {
                authoring::verify_module(authoring::ModuleVerifyOptions {
                    json: args.json,
                    manifest: args.manifest,
                    module_key: args.module_key,
                    output: args.output,
                    project: args.project,
                    repo_root: args.repo_root,
                })?;
            }
        },
    }

    Ok(())
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
    fn removed_framework_commands_are_not_public() {
        let command = Cli::command();
        let names = command
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["add", "check", "resolve", "run", "compose", "module"]
        );

        let module = command
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "module")
            .expect("module command");
        let module_names = module
            .get_subcommands()
            .map(clap::Command::get_name)
            .collect::<Vec<_>>();
        assert_eq!(module_names, ["create", "dev", "check", "verify"]);
    }
}
