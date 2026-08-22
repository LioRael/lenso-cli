use std::{env, path::PathBuf, process::ExitCode, time::Duration};

use lenso_app_plan::ExecutionClassId;
use lenso_authoring::{
    AddModule, CheckOptions, Module, PackageInput, PackageSource, ProjectAuthoring, ProjectPath,
    ResolutionOptions, ResolvedProject, run_project,
};
use lenso_bun_adapter::{BunAdapter, BunAdapterConfig, BunWire};
use lenso_kernel::ExecutionAdapterCatalog;
use lenso_runner::TokioDriver;

fn usage() -> &'static str {
    "usage:
  lenso add --project <lenso.json> --key <key> --package <runtime-id> --source <cargo|bun|npm|oci> --version <requirement> [--package-name <name>] [--locked-revision <revision>] [--entrypoint <path>] [--manifest <path>] [--lockfile <path>]
  lenso check --project <lenso.json> [--execution-class <id>]...
  lenso resolve --project <lenso.json> [--profile <name>] [--execution-class <id>]... [--output <path>]
  lenso run [--plan <resolved-plan.json>] [--root <project-dir>] [--bun <path>]"
}

fn value(arguments: &[String], name: &str) -> Result<String, String> {
    let index = arguments
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("missing {name}\n{}", usage()))?;
    arguments
        .get(index + 1)
        .cloned()
        .ok_or_else(|| format!("missing value for {name}"))
}

fn optional_value(arguments: &[String], name: &str) -> Option<String> {
    arguments
        .iter()
        .position(|argument| argument == name)
        .and_then(|index| arguments.get(index + 1))
        .cloned()
}

fn values(arguments: &[String], name: &str) -> Vec<String> {
    arguments
        .iter()
        .enumerate()
        .filter(|(_, argument)| argument.as_str() == name)
        .filter_map(|(index, _)| arguments.get(index + 1))
        .cloned()
        .collect()
}

fn check_options(arguments: &[String]) -> CheckOptions {
    let classes = values(arguments, "--execution-class");
    if classes.is_empty() {
        CheckOptions::default()
    } else {
        CheckOptions::new(classes)
    }
}

fn source(value: &str) -> Result<PackageSource, String> {
    match value {
        "cargo" => Ok(PackageSource::Cargo),
        "bun" => Ok(PackageSource::Bun),
        "npm" => Ok(PackageSource::Npm),
        "oci" => Ok(PackageSource::Oci),
        _ => Err(format!("unknown package source {value}")),
    }
}

fn project_path(arguments: &[String]) -> PathBuf {
    optional_value(arguments, "--project")
        .map_or_else(|| PathBuf::from("lenso.json"), PathBuf::from)
}

fn add(arguments: &[String]) -> Result<(), String> {
    let path = project_path(arguments);
    let package_name = value(arguments, "--package")?;
    let package_source = source(&value(arguments, "--source")?)?;
    let mut package = PackageInput::new(
        &package_name,
        package_source,
        value(arguments, "--version")?,
    );
    if let Some(package_name) = optional_value(arguments, "--package-name") {
        package = package.with_package_name(package_name);
    }
    if let Some(locked_revision) = optional_value(arguments, "--locked-revision") {
        package = package.with_locked_revision(locked_revision);
    }
    if let Some(manifest) = optional_value(arguments, "--manifest") {
        package = package.with_manifest(manifest);
    }
    if let Some(lockfile) = optional_value(arguments, "--lockfile") {
        package = package.with_lockfile(lockfile);
    }
    let mut module = Module::new(value(arguments, "--key")?, &package_name);
    if let Some(entrypoint) = optional_value(arguments, "--entrypoint") {
        module = module.with_entrypoint(entrypoint);
    }
    let request = AddModule::new(module, package);
    let result = ProjectPath::new(&path)
        .add(&request)
        .map_err(|error| error.to_string())?;
    for changed in result.changed_files() {
        println!("updated {}", changed.display());
    }
    Ok(())
}

fn check(arguments: &[String]) -> Result<(), String> {
    let path = project_path(arguments);
    let project = ProjectPath::load(&path).map_err(|error| error.to_string())?;
    let root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let options = check_options(arguments);
    let report = project
        .check(root, &options)
        .map_err(|error| error.to_string())?;
    println!(
        "checked {} Module Instances, {} bindings, {} contracts",
        report.modules, report.bindings, report.contracts
    );
    Ok(())
}

fn resolve(arguments: &[String]) -> Result<(), String> {
    let path = project_path(arguments);
    let project = ProjectPath::load(&path).map_err(|error| error.to_string())?;
    let root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut options = ResolutionOptions::default().with_check_options(check_options(arguments));
    if let Some(profile) = optional_value(arguments, "--profile") {
        options = options.with_profile(profile);
    }
    let resolved = project
        .resolve(root, &options)
        .map_err(|error| error.to_string())?;
    let output = optional_value(arguments, "--output")
        .map_or_else(|| root.join(".lenso/resolved-plan.json"), PathBuf::from);
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::write(&output, resolved.canonical_bytes()).map_err(|error| error.to_string())?;
    println!("resolved {} ({})", output.display(), resolved.fingerprint());
    Ok(())
}

async fn run(arguments: &[String]) -> Result<(), String> {
    let plan_path = optional_value(arguments, "--plan")
        .map_or_else(|| PathBuf::from(".lenso/resolved-plan.json"), PathBuf::from);
    let resolved = ResolvedProject::from_canonical_bytes(
        &std::fs::read(&plan_path).map_err(|error| format!("{}: {error}", plan_path.display()))?,
    )
    .map_err(|error| error.to_string())?;
    let root = optional_value(arguments, "--root").map_or_else(
        || std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        PathBuf::from,
    );
    let mut adapters = ExecutionAdapterCatalog::new();
    let needs_bun = resolved
        .plan()
        .module_instances()
        .iter()
        .any(|instance| instance.execution_class() == &ExecutionClassId::bun_child_process());
    if needs_bun {
        let bun = optional_value(arguments, "--bun").unwrap_or_else(|| "bun".to_owned());
        let config =
            BunAdapterConfig::new(&bun, BunWire::JsonRpcHttp).with_working_directory(&root);
        adapters = adapters
            .with_adapter(BunAdapter::production(bun).with_config(config))
            .map_err(|error| error.to_string())?;
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
        .await
        .map_err(|error| error.to_string())?;
    println!("{outcome:?}");
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let result = match arguments.first().map(String::as_str) {
        Some("add") => add(&arguments[1..]),
        Some("check") => check(&arguments[1..]),
        Some("resolve") => resolve(&arguments[1..]),
        Some("run") => run(&arguments[1..]).await,
        _ => Err(usage().to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
