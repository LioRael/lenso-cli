use std::{fs, path::PathBuf};

use anyhow::Context;
use clap::{Args, Subcommand};

use crate::plugins::{load_resolved_app, project_root};

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum AppCommand {
    /// Validate the App derived from this Host and its `plugins/` directory.
    Check(ProjectArgs),
    /// Explain the derived Plugin Instances, provenance, and bindings.
    Show(ProjectArgs),
    /// Export the derived immutable Plan as advanced diagnostic evidence.
    Resolve(ResolveArgs),
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ProjectArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ResolveArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Derived Plan output. This file is never App authoring input.
    #[arg(long, default_value = ".lenso/resolved-plan.json")]
    output: PathBuf,
}

pub(crate) fn app(command: AppCommand) -> anyhow::Result<()> {
    match command {
        AppCommand::Check(args) => check(args),
        AppCommand::Show(args) => show(args),
        AppCommand::Resolve(args) => resolve(args),
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

fn resolve(args: ResolveArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let resolved = load_resolved_app(&root)?;
    let output = root.join(args.output);
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create derived output directory {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(resolved.plan())?;
    fs::write(&output, bytes)
        .with_context(|| format!("write derived Plan {}", output.display()))?;
    println!("Wrote derived Plan evidence to {}.", output.display());
    Ok(())
}
