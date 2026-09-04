use std::{fs, path::PathBuf};

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use lenso_app_plan::authoring::HostCatalog;

use crate::plugins::{load_resolved_app, project_root};

mod build;
mod prepare;

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum AppCommand {
    /// Build Host authoring artifacts from static TypeScript and verified bundles.
    Build(build::HostBuildArgs),
    /// Prepare one immutable, offline Host distribution for an exact target.
    Prepare(prepare::PrepareArgs),
    /// Create an App workspace from one exact Host executable and Host Catalog.
    Init(AppInitArgs),
    /// Validate the App derived from this Host and its `plugins/` directory.
    Check(ProjectArgs),
    /// Explain the derived Plugin Instances, provenance, and bindings.
    Show(ShowArgs),
}

#[derive(Args, Clone, Debug)]
pub(crate) struct AppInitArgs {
    /// Host executable to copy into the App workspace.
    #[arg(long)]
    host: PathBuf,
    /// Host Catalog JSON emitted by the same Host Build.
    #[arg(long)]
    host_catalog: PathBuf,
    /// New App workspace directory. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ProjectArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Clone, Debug)]
pub(crate) struct ShowArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
    /// Distribution Host authority used by the private runtime resolver.
    #[arg(long, hide = true, requires = "runtime_json")]
    host_build: Option<PathBuf>,
    /// Emit the exact private runtime input rather than the management projection.
    #[arg(long, hide = true, requires = "host_build")]
    runtime_json: bool,
}

pub(crate) fn app(command: AppCommand) -> anyhow::Result<()> {
    match command {
        AppCommand::Build(args) => build::build(&args),
        AppCommand::Prepare(args) => prepare::prepare(args),
        AppCommand::Init(args) => init(args),
        AppCommand::Check(args) => check(args),
        AppCommand::Show(args) => show(args),
    }
}

fn init(args: AppInitArgs) -> anyhow::Result<()> {
    let root = args.root.unwrap_or(std::env::current_dir()?);
    let host = fs::canonicalize(&args.host)
        .with_context(|| format!("locate Host executable {}", args.host.display()))?;
    let host_metadata = fs::symlink_metadata(&host)?;
    if !host_metadata.file_type().is_file() {
        bail!("Host executable must be a regular file: {}", host.display());
    }
    let catalog_bytes = fs::read(&args.host_catalog)
        .with_context(|| format!("read Host Catalog {}", args.host_catalog.display()))?;
    let _: HostCatalog =
        serde_json::from_slice(&catalog_bytes).context("Host Catalog is invalid")?;
    let control = root.join(".lenso");
    let plugins = root.join("plugins");
    if control.exists() || plugins.exists() {
        bail!(
            "App workspace already contains `.lenso/` or `plugins/`: {}",
            root.display()
        );
    }
    fs::create_dir_all(&root)
        .with_context(|| format!("create App workspace {}", root.display()))?;
    let staging = tempfile::Builder::new()
        .prefix(".lenso-init-")
        .tempdir_in(&root)
        .context("stage App workspace")?;
    let staged_control = staging.path().join(".lenso");
    fs::create_dir(&staged_control)?;
    fs::copy(&host, staged_control.join("host"))?;
    fs::write(staged_control.join("host-catalog.json"), &catalog_bytes)?;
    fs::create_dir(staging.path().join("plugins"))?;
    fs::rename(&staged_control, &control)?;
    if let Err(error) = fs::rename(staging.path().join("plugins"), &plugins) {
        let _ = fs::remove_dir_all(&control);
        return Err(error).context("publish Plugin Root");
    }
    let resolved = load_resolved_app(&root).inspect_err(|_| {
        let _ = fs::remove_dir_all(&plugins);
        let _ = fs::remove_dir_all(&control);
    })?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.app-init",
                "status": "created",
                "root": root,
                "plugin_instances": resolved.instances().len(),
                "capability_bindings": resolved.plan().capability_bindings().len(),
            }))?
        );
    } else {
        println!("Created App workspace at {}.", root.display());
    }
    Ok(())
}

fn check(args: ProjectArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let resolved = load_resolved_app(&root)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.app-check",
                "status": "passed",
                "plugin_instances": resolved.instances().len(),
                "capability_bindings": resolved.plan().capability_bindings().len(),
            }))?
        );
    } else {
        println!(
            "App is valid: {} Plugin Instance(s), {} Capability binding(s).",
            resolved.instances().len(),
            resolved.plan().capability_bindings().len()
        );
    }
    Ok(())
}

fn show(args: ShowArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    if args.runtime_json {
        let host_build = args
            .host_build
            .context("runtime resolution needs Host authority")?;
        let resolution = lenso_app_authoring::resolve_runtime_app(&root, &host_build)?;
        println!("{}", serde_json::to_string(&resolution)?);
        return Ok(());
    }
    let resolved = load_resolved_app(&root)?;
    if args.json {
        let instances = resolved
            .instances()
            .iter()
            .map(|instance| {
                serde_json::json!({
                    "id": instance.id().to_string(),
                    "source": format!("{:?}", instance.source()),
                    "plan_key": instance.plan_key(),
                })
            })
            .collect::<Vec<_>>();
        let bindings = resolved
            .plan()
            .capability_bindings()
            .iter()
            .map(|binding| {
                serde_json::json!({
                    "consumer_instance": binding.consumer_instance(),
                    "capability_id": binding.capability_id(),
                    "descriptor_version": binding.descriptor_version(),
                    "provider_instance": binding.provider_instance(),
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.app-show",
                "instances": instances,
                "bindings": bindings,
            }))?
        );
        return Ok(());
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use lenso_app_plan::authoring::{
        HostDefaultPlugin, HostPluginRelease, HostSlot, PluginDescriptor,
    };

    #[test]
    fn init_publishes_one_valid_app_workspace() {
        let temporary = tempfile::tempdir().unwrap();
        let host = temporary.path().join("host-source");
        fs::write(&host, b"host").unwrap();
        let catalog = HostCatalog::new(
            [HostSlot::one("agent")],
            [HostPluginRelease::new(PluginDescriptor::new(
                "example.agent",
                "1.0.0",
                "agent",
            ))],
            [HostDefaultPlugin::new("example.agent", "default")],
        );
        let catalog_path = temporary.path().join("catalog.json");
        fs::write(&catalog_path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        let root = temporary.path().join("app");

        init(AppInitArgs {
            host,
            host_catalog: catalog_path,
            root: Some(root.clone()),
            json: false,
        })
        .unwrap();

        assert!(root.join(".lenso/host").is_file());
        assert!(root.join("plugins").is_dir());
        assert_eq!(load_resolved_app(&root).unwrap().instances().len(), 1);
    }
}
