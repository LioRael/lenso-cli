use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::plugins::{load_resolved_app, project_root};

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum AppCommand {
    /// Validate the App derived from this Host and its `plugins/` directory.
    Check(ProjectArgs),
    /// Explain the derived Plugin Instances, provenance, and bindings.
    Show(ProjectArgs),
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ProjectArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

pub(crate) fn app(command: AppCommand) -> anyhow::Result<()> {
    match command {
        AppCommand::Check(args) => check(args),
        AppCommand::Show(args) => show(args),
    }
}

fn check(args: ProjectArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let resolved = load_resolved_app(&root)?;
    println!(
        "App is valid: {} Plugin Instance(s), {} Capability binding(s).",
        resolved.instances().len(),
        resolved.plan().capability_bindings().len()
    );
    Ok(())
}

fn show(args: ProjectArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let resolved = load_resolved_app(&root)?;
    println!("Plugin Instances:");
    for instance in resolved.instances() {
        println!(
            "  {}  source={:?}  plan-key={}",
            instance.id(),
            instance.source(),
            instance.plan_key()
        );
    }
    println!("Capability bindings:");
    for binding in resolved.plan().capability_bindings() {
        println!(
            "  {} --{}@{}--> {}",
            binding.consumer_instance(),
            binding.capability_id(),
            binding.descriptor_version(),
            binding.provider_instance()
        );
    }
    Ok(())
}
