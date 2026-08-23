use std::{path::PathBuf, time::Duration};

use anyhow::Context;
use clap::Args;
use lenso_app_plan::ExecutionClassId;
use lenso_authoring::{
    AddModule, CheckOptions, Module, PackageInput, PackageSource, ProjectAuthoring, ProjectPath,
    ResolutionOptions, ResolvedProject, run_project,
};
use lenso_bun_adapter::{BunAdapter, BunAdapterConfig, BunWire};
use lenso_kernel::ExecutionAdapterCatalog;
use lenso_runner::TokioDriver;

#[derive(Debug, Args, Clone)]
#[command(disable_version_flag = true)]
pub(crate) struct AddArgs {
    #[arg(long, default_value = "lenso.json")]
    project: PathBuf,
    #[arg(long)]
    key: String,
    #[arg(long = "package")]
    runtime_id: String,
    #[arg(long = "package-name")]
    package_name: Option<String>,
    #[arg(long = "source", value_parser = ["cargo", "bun", "npm", "oci"])]
    package_source: String,
    #[arg(long)]
    version: String,
    #[arg(long)]
    locked_revision: Option<String>,
    #[arg(long)]
    entrypoint: Option<String>,
    #[arg(long)]
    manifest: Option<String>,
    #[arg(long)]
    lockfile: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct CheckArgs {
    #[arg(long, default_value = "lenso.json")]
    project: PathBuf,
    #[arg(long = "execution-class")]
    execution_classes: Vec<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ResolveArgs {
    #[arg(long, default_value = "lenso.json")]
    project: PathBuf,
    #[arg(long)]
    profile: Option<String>,
    #[arg(long = "execution-class")]
    execution_classes: Vec<String>,
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct RunArgs {
    #[arg(long, default_value = ".lenso/resolved-plan.json")]
    plan: PathBuf,
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[arg(long, default_value = "bun")]
    bun: String,
}

pub(crate) fn add(args: &AddArgs) -> anyhow::Result<()> {
    let package_source = match args.package_source.as_str() {
        "cargo" => PackageSource::Cargo,
        "bun" => PackageSource::Bun,
        "npm" => PackageSource::Npm,
        "oci" => PackageSource::Oci,
        source => anyhow::bail!("unknown package source {source}"),
    };
    let mut package = PackageInput::new(&args.runtime_id, package_source, args.version.clone());
    if let Some(package_name) = &args.package_name {
        package = package.with_package_name(package_name);
    }
    if let Some(locked_revision) = &args.locked_revision {
        package = package.with_locked_revision(locked_revision);
    }
    if let Some(manifest) = &args.manifest {
        package = package.with_manifest(manifest);
    }
    if let Some(lockfile) = &args.lockfile {
        package = package.with_lockfile(lockfile);
    }
    let mut module = Module::new(&args.key, &args.runtime_id);
    if let Some(entrypoint) = &args.entrypoint {
        module = module.with_entrypoint(entrypoint);
    }
    let result = ProjectPath::new(&args.project).add(&AddModule::new(module, package))?;
    for changed in result.changed_files() {
        println!("updated {}", changed.display());
    }
    Ok(())
}

pub(crate) fn check(args: &CheckArgs) -> anyhow::Result<()> {
    let project = ProjectPath::load(&args.project)?;
    let root = args
        .project
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let options = check_options(&args.execution_classes);
    let report = project.check(root, &options)?;
    println!(
        "checked {} Module Instances, {} bindings, {} contracts",
        report.modules, report.bindings, report.contracts
    );
    Ok(())
}

pub(crate) fn resolve(args: &ResolveArgs) -> anyhow::Result<()> {
    let project = ProjectPath::load(&args.project)?;
    let root = args
        .project
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut options =
        ResolutionOptions::default().with_check_options(check_options(&args.execution_classes));
    if let Some(profile) = &args.profile {
        options = options.with_profile(profile);
    }
    let resolved = project.resolve(root, &options)?;
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| root.join(".lenso/resolved-plan.json"));
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, resolved.canonical_bytes())?;
    println!("resolved {} ({})", output.display(), resolved.fingerprint());
    Ok(())
}

pub(crate) async fn run(args: &RunArgs) -> anyhow::Result<()> {
    let plan = std::fs::read(&args.plan)
        .with_context(|| format!("failed to read {}", args.plan.display()))?;
    let resolved = ResolvedProject::from_canonical_bytes(&plan)?;
    let mut adapters = ExecutionAdapterCatalog::new();
    let needs_bun = resolved
        .plan()
        .module_instances()
        .iter()
        .any(|instance| instance.execution_class() == &ExecutionClassId::bun_child_process());
    if needs_bun {
        let config = BunAdapterConfig::new(&args.bun, BunWire::JsonRpcHttp)
            .with_working_directory(&args.root);
        adapters =
            adapters.with_adapter(BunAdapter::production(args.bun.clone()).with_config(config))?;
    }
    let driver = TokioDriver::new();
    let local = tokio::task::LocalSet::new();
    let shutdown_driver = driver.clone();
    local.spawn_local(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            shutdown_driver.request_shutdown();
        }
    });
    let outcome = local
        .run_until(run_project(
            &resolved,
            driver,
            adapters,
            Duration::from_secs(10),
        ))
        .await?;
    println!("{outcome:?}");
    Ok(())
}

fn check_options(execution_classes: &[String]) -> CheckOptions {
    if execution_classes.is_empty() {
        CheckOptions::default()
    } else {
        CheckOptions::new(execution_classes.to_vec())
    }
}
