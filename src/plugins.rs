use std::{env, fs, path::PathBuf};

use anyhow::Context;
use clap::{Args, Subcommand};
pub(crate) use lenso_app_authoring::load_resolved_app;
use lenso_app_authoring::{
    add_bundle, configure_instance, remove_instance_difference, remove_plugin,
    set_instance_disabled,
};
use lenso_app_plan::authoring::PluginInstanceId;

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum PluginsCommand {
    /// List the Plugin Instances in the derived App.
    List(ProjectArgs),
    /// Add one exact Plugin Bundle after candidate validation.
    Add(AddArgs),
    /// Write direct configuration for one Plugin Instance.
    Configure(ConfigureArgs),
    /// Disable one Plugin Instance without deleting its configuration.
    Disable(InstanceArgs),
    /// Re-enable one disabled Plugin Instance.
    Enable(InstanceArgs),
    /// Remove one Instance difference or an entire root-supplied Plugin.
    Remove(RemoveArgs),
}

#[derive(Clone, Debug, Args)]
pub(crate) struct ProjectArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct AddArgs {
    /// Exact `.lenso-plugin` Bundle directory.
    bundle: PathBuf,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct ConfigureArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// App-local Instance key.
    #[arg(default_value = "default")]
    instance: String,
    /// TOML file to use. Omit to create an empty configuration.
    #[arg(long)]
    file: Option<PathBuf>,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct InstanceArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// App-local Instance key.
    #[arg(default_value = "default")]
    instance: String,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct RemoveArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// Remove only this Instance difference; omit to remove the whole Plugin directory.
    instance: Option<String>,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

pub(crate) fn plugins(command: PluginsCommand) -> anyhow::Result<()> {
    match command {
        PluginsCommand::List(args) => list(args),
        PluginsCommand::Add(args) => add(args),
        PluginsCommand::Configure(args) => configure(args),
        PluginsCommand::Disable(args) => set_disabled(args, true),
        PluginsCommand::Enable(args) => set_disabled(args, false),
        PluginsCommand::Remove(args) => remove(args),
    }
}

pub(crate) fn project_root(root: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    root.map_or_else(
        || env::current_dir().context("read current directory"),
        |root| {
            Ok(if root.is_absolute() {
                root
            } else {
                env::current_dir()?.join(root)
            })
        },
    )
}

fn list(args: ProjectArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let resolved = load_resolved_app(&root)?;
    for instance in resolved.instances() {
        println!("{}\t{:?}", instance.id(), instance.source());
    }
    Ok(())
}

fn add(args: AddArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let (plugin_id, release_version, _) = add_bundle(&root, &args.bundle)?;
    println!("Added Plugin `{plugin_id}` {release_version}.");
    Ok(())
}

fn configure(args: ConfigureArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let bytes = args.file.map_or_else(
        || Ok(Vec::new()),
        |path| fs::read(&path).with_context(|| format!("read {}", path.display())),
    )?;
    configure_instance(&root, &args.plugin_id, &args.instance, &bytes)?;
    let id = PluginInstanceId::new(&args.plugin_id, &args.instance);
    println!("Configured Plugin Instance `{id}`.");
    Ok(())
}

fn set_disabled(args: InstanceArgs, disabled: bool) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    set_instance_disabled(&root, &args.plugin_id, &args.instance, disabled)?;
    let id = PluginInstanceId::new(&args.plugin_id, &args.instance);
    let action = if disabled { "Disabled" } else { "Enabled" };
    println!("{action} Plugin Instance `{id}`.");
    Ok(())
}

fn remove(args: RemoveArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    if let Some(instance) = args.instance {
        remove_instance_difference(&root, &args.plugin_id, &instance)?;
        println!(
            "Removed Plugin Instance difference `{}/{instance}`.",
            args.plugin_id
        );
    } else {
        let (_, trash) = remove_plugin(&root, &args.plugin_id)?;
        println!(
            "Removed Plugin `{}`; recoverable at {}.",
            args.plugin_id,
            trash.display()
        );
    }
    Ok(())
}
