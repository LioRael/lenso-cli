use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::Context;
use clap::{Args, Subcommand};
use lenso_app_plan::{CapabilityEndpointPlan, ExecutionClassId, ResolvedAppPlan};
use lenso_authoring::{
    AddModule, AppAddRequest, AppRemoveRequest, AuthoringError, Cardinality, CargoAppDefinition,
    CargoModuleSource, CheckOptions, CompositionRecipePath, CompositionRunner, Module,
    PackageInput, PackageSource, ProjectAuthoring, ProjectFile, ProjectPath, ResolutionOptions,
    ResolvedProject, run_project, sha256_bytes,
};
use lenso_bun_adapter::{BunAdapter, BunAdapterConfig, BunCapabilityCodec, BunWire};
use lenso_kernel::{ExecutionAdapterCatalog, RuntimeFailure};
use lenso_runner::TokioDriver;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

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

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum ComposeCommand {
    /// List available variants.
    List(ComposeListArgs),
    /// Check one or every materialized variant.
    Check(ComposeCheckArgs),
    /// Resolve one or every variant to its declared canonical Plan output.
    Resolve(ComposeResolveArgs),
    /// Resolve and run one variant through its product-owned Runner.
    Run(ComposeRunArgs),
    /// Watch, resolve, and restart one variant through its product-owned Runner.
    Dev(ComposeDevArgs),
}

#[derive(Debug, Subcommand, Clone)]
pub(crate) enum AppCommand {
    /// Install a Cargo Module and select one App-local Instance.
    Add(Box<AppAddArgs>),
    /// Remove one App-local Instance and optionally uninstall its Cargo package.
    Remove(AppRemoveArgs),
    /// Check a source-derived App Definition and its package artifacts.
    Check(AppCheckArgs),
    /// Resolve a source-derived App Definition into an immutable Plan.
    Resolve(AppResolveArgs),
}

#[derive(Debug, Args, Clone)]
#[command(disable_version_flag = true)]
pub(crate) struct AppAddArgs {
    /// Cargo package that owns the Module Descriptor.
    cargo_package: String,
    /// App Definition document.
    #[arg(long, default_value = "lenso.app.json")]
    definition: PathBuf,
    /// App-local Instance key. Defaults to the final segment of the runtime package id.
    #[arg(long)]
    key: Option<String>,
    /// Descriptor entrypoint exposed by the package.
    #[arg(long, default_value = "default")]
    entrypoint: String,
    /// Module configuration as a JSON value.
    #[arg(long, default_value = "{}", value_parser = parse_json_value)]
    configuration: Value,
    /// Execution lane selected for this Instance.
    #[arg(long)]
    execution_lane: Option<String>,
    /// crates.io version requirement.
    #[arg(long, conflicts_with_all = ["git", "path"])]
    version: Option<String>,
    /// Git repository source.
    #[arg(long, conflicts_with = "path")]
    git: Option<String>,
    /// Exact Git revision; requires --git.
    #[arg(long, requires = "git", conflicts_with_all = ["branch", "tag"])]
    rev: Option<String>,
    /// Git branch; requires --git.
    #[arg(long, requires = "git", conflicts_with_all = ["rev", "tag"])]
    branch: Option<String>,
    /// Git tag; requires --git.
    #[arg(long, requires = "git", conflicts_with_all = ["rev", "branch"])]
    tag: Option<String>,
    /// Local Cargo package path.
    #[arg(long, conflicts_with = "git")]
    path: Option<PathBuf>,
    /// Validate and print the change, then restore every touched file.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct AppRemoveArgs {
    /// App-local Module Instance key.
    key: String,
    /// App Definition document.
    #[arg(long, default_value = "lenso.app.json")]
    definition: PathBuf,
    /// Also remove the Cargo dependency when no other Instance uses it.
    #[arg(long)]
    uninstall: bool,
    /// Validate and print the change, then restore every touched file.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct AppCheckArgs {
    /// App Definition document.
    #[arg(long, default_value = "lenso.app.json")]
    definition: PathBuf,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct AppResolveArgs {
    /// App Definition document.
    #[arg(long, default_value = "lenso.app.json")]
    definition: PathBuf,
    /// Canonical immutable Plan output.
    #[arg(long, default_value = ".lenso/resolved-plan.json")]
    output: PathBuf,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ComposeListArgs {
    /// Reusable Composition recipe document.
    #[arg(long, default_value = "composition/recipes.json")]
    recipe: PathBuf,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ComposeCheckArgs {
    /// Reusable Composition recipe document.
    #[arg(long, default_value = "composition/recipes.json")]
    recipe: PathBuf,
    /// Check only one named variant.
    #[arg(long)]
    variant: Option<String>,
    /// Remove one selected fragment before checking; requires --variant.
    #[arg(long = "without")]
    excluded_fragments: Vec<String>,
    #[arg(long = "execution-class")]
    execution_classes: Vec<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ComposeResolveArgs {
    /// Reusable Composition recipe document.
    #[arg(long, default_value = "composition/recipes.json")]
    recipe: PathBuf,
    /// Resolve only one named variant.
    #[arg(long)]
    variant: Option<String>,
    /// Override the declared output; valid only with --variant.
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long = "execution-class")]
    execution_classes: Vec<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ComposeRunArgs {
    /// Reusable Composition recipe document.
    #[arg(long, default_value = "composition/recipes.json")]
    recipe: PathBuf,
    /// Named variant to resolve and run.
    #[arg(long)]
    variant: String,
    #[arg(long = "execution-class")]
    execution_classes: Vec<String>,
    /// Additional arguments forwarded to the product Runner after `--`.
    #[arg(last = true)]
    runner_args: Vec<String>,
}

#[derive(Debug, Args, Clone)]
pub(crate) struct ComposeDevArgs {
    /// Reusable Composition recipe document.
    #[arg(long, default_value = "composition/recipes.json")]
    recipe: PathBuf,
    /// Named variant to watch and run.
    #[arg(long)]
    variant: String,
    #[arg(long = "execution-class")]
    execution_classes: Vec<String>,
    /// Additional arguments forwarded to each product Runner after `--`.
    #[arg(last = true)]
    runner_args: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct ModuleCheckOptions {
    pub(crate) json: bool,
    pub(crate) project: PathBuf,
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub(crate) struct ModuleVerifyOptions {
    pub(crate) json: bool,
    pub(crate) manifest: PathBuf,
    pub(crate) module_key: Option<String>,
    pub(crate) output: PathBuf,
    pub(crate) project: PathBuf,
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleAuthoringReport {
    artifact_version: &'static str,
    status: &'static str,
    project: String,
    execution_classes: Vec<String>,
    checks: Vec<ModuleAuthoringCheck>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleAuthoringCheck {
    id: String,
    layer: &'static str,
    owner: &'static str,
    status: &'static str,
    path: String,
    message: String,
    fix: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct VerificationManifest {
    protocol: String,
    #[serde(default)]
    probes: Vec<VerificationProbe>,
}

#[derive(Clone, Debug, Deserialize)]
struct VerificationProbe {
    id: String,
    purpose: String,
    command: String,
    #[serde(default, rename = "expectFailure")]
    expect_failure: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VerificationEvidence {
    artifact_version: &'static str,
    status: &'static str,
    project: String,
    plan_fingerprint: Option<String>,
    probes: Vec<VerificationProbeEvidence>,
    removal_proofs: Vec<RemovalProof>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VerificationProbeEvidence {
    id: String,
    purpose: String,
    command: String,
    status: &'static str,
    exit_code: Option<i32>,
    detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RemovalProof {
    requested_module: String,
    removed_modules: Vec<String>,
    remaining_modules: usize,
    status: &'static str,
    detail: String,
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

pub(crate) async fn compose(command: ComposeCommand) -> anyhow::Result<()> {
    match command {
        ComposeCommand::List(args) => compose_list(&args),
        ComposeCommand::Check(args) => compose_check(&args),
        ComposeCommand::Resolve(args) => compose_resolve(&args),
        ComposeCommand::Run(args) => compose_run(&args).await,
        ComposeCommand::Dev(args) => compose_dev(&args).await,
    }
}

pub(crate) fn app(command: AppCommand) -> anyhow::Result<()> {
    match command {
        AppCommand::Add(args) => app_add(&args),
        AppCommand::Remove(args) => app_remove(&args),
        AppCommand::Check(args) => app_check(&args),
        AppCommand::Resolve(args) => app_resolve(&args),
    }
}

fn app_add(args: &AppAddArgs) -> anyhow::Result<()> {
    let result = lenso_authoring::add_app_module(
        &args.definition,
        &AppAddRequest {
            cargo_package: args.cargo_package.clone(),
            key: args.key.clone(),
            entrypoint: args.entrypoint.clone(),
            configuration: args.configuration.clone(),
            execution_lane: args.execution_lane.clone(),
            source: CargoModuleSource {
                version: args.version.clone(),
                git: args.git.clone(),
                rev: args.rev.clone(),
                branch: args.branch.clone(),
                tag: args.tag.clone(),
                path: args.path.clone(),
            },
            dry_run: args.dry_run,
        },
    )?;
    print_app_edit("added", &result);
    Ok(())
}

fn app_remove(args: &AppRemoveArgs) -> anyhow::Result<()> {
    let result = lenso_authoring::remove_app_module(
        &args.definition,
        &AppRemoveRequest {
            key: args.key.clone(),
            uninstall: args.uninstall,
            dry_run: args.dry_run,
        },
    )?;
    print_app_edit("removed", &result);
    Ok(())
}

fn print_app_edit(action: &str, result: &lenso_authoring::AppEditResult) {
    let prefix = if result.dry_run { "would have " } else { "" };
    println!(
        "{prefix}{action} {} -> {} ({})",
        result.key, result.runtime_package, result.cargo_package
    );
    for path in &result.changed_files {
        println!("  {}", path.display());
    }
}

fn parse_json_value(value: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|error| error.to_string())
}

fn app_check(args: &AppCheckArgs) -> anyhow::Result<()> {
    let definition = CargoAppDefinition::load(&args.definition)?;
    let root = args.definition.parent().unwrap_or_else(|| Path::new("."));
    let composition = definition.derive(root)?;
    let plan = composition.resolve()?;
    println!(
        "checked {}: {} Module Instances, {} derived bindings",
        definition.app().name(),
        plan.module_instances().len(),
        plan.capability_bindings().len()
    );
    Ok(())
}

fn app_resolve(args: &AppResolveArgs) -> anyhow::Result<()> {
    let definition = CargoAppDefinition::load(&args.definition)?;
    let root = args.definition.parent().unwrap_or_else(|| Path::new("."));
    let bytes = definition.resolve_canonical(root)?;
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&args.output, &bytes)?;
    println!(
        "resolved {} -> {} ({})",
        definition.app().name(),
        args.output.display(),
        sha256_bytes(&bytes)
    );
    Ok(())
}

fn compose_list(args: &ComposeListArgs) -> anyhow::Result<()> {
    let path = CompositionRecipePath::new(&args.recipe);
    let recipe = path.load()?;
    for (name, variant) in recipe.variants() {
        println!("{name}\t{}", variant.output());
    }
    Ok(())
}

fn compose_check(args: &ComposeCheckArgs) -> anyhow::Result<()> {
    if !args.excluded_fragments.is_empty() && args.variant.is_none() {
        anyhow::bail!("--without requires --variant");
    }
    let path = CompositionRecipePath::new(&args.recipe);
    let recipe = path.load()?;
    let execution_classes = recipe_execution_classes(&recipe, &args.execution_classes);
    for name in selected_recipe_variants(&recipe, args.variant.as_deref())? {
        let materialized = path.materialize_without(&recipe, name, &args.excluded_fragments)?;
        let report = materialized
            .project()
            .check(materialized.root(), &check_options(execution_classes))?;
        println!(
            "checked {name}: {} Module Instances, {} bindings, {} contracts",
            report.modules, report.bindings, report.contracts
        );
    }
    Ok(())
}

fn compose_resolve(args: &ComposeResolveArgs) -> anyhow::Result<()> {
    if args.output.is_some() && args.variant.is_none() {
        anyhow::bail!("--output requires --variant");
    }
    let path = CompositionRecipePath::new(&args.recipe);
    let recipe = path.load()?;
    let execution_classes = recipe_execution_classes(&recipe, &args.execution_classes);
    for name in selected_recipe_variants(&recipe, args.variant.as_deref())? {
        let materialized = path.materialize(&recipe, name)?;
        let mut options =
            ResolutionOptions::default().with_check_options(check_options(execution_classes));
        if let Some(profile) = materialized.profile() {
            options = options.with_profile(profile);
        }
        let resolved = materialized
            .project()
            .resolve(materialized.root(), &options)?;
        let output = args
            .output
            .as_deref()
            .unwrap_or_else(|| materialized.output());
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output, resolved.canonical_bytes())?;
        println!(
            "resolved {name} -> {} ({})",
            output.display(),
            resolved.fingerprint()
        );
    }
    Ok(())
}

fn recipe_execution_classes<'a>(
    recipe: &'a lenso_authoring::CompositionRecipe,
    requested: &'a [String],
) -> &'a [String] {
    if requested.is_empty() {
        recipe
            .runner()
            .map_or(requested, CompositionRunner::execution_classes)
    } else {
        requested
    }
}

async fn compose_run(args: &ComposeRunArgs) -> anyhow::Result<()> {
    let execution =
        resolve_variant_for_execution(&args.recipe, &args.variant, &args.execution_classes)?;
    println!("running {} ({})", execution.variant, execution.fingerprint);
    let status = tokio::process::Command::new(execution.runner.program())
        .args(execution.runner.args())
        .args(&args.runner_args)
        .current_dir(&execution.root)
        .env("LENSO_RESOLVED_PLAN", &execution.plan)
        .env("LENSO_COMPOSITION_VARIANT", &execution.variant)
        .status()
        .await
        .with_context(|| {
            format!(
                "start product Runner `{}` for Composition variant `{}`",
                execution.runner.program(),
                execution.variant
            )
        })?;
    if !status.success() {
        anyhow::bail!(
            "product Runner for Composition variant `{}` exited with {status}",
            execution.variant
        );
    }
    Ok(())
}

async fn compose_dev(args: &ComposeDevArgs) -> anyhow::Result<()> {
    let path = CompositionRecipePath::new(&args.recipe);
    let recipe = path.load()?;
    let root = path.root(&recipe)?;
    if recipe.runner().is_none() {
        anyhow::bail!(
            "Composition recipe {} defines no product Runner",
            args.recipe.display()
        );
    }
    println!(
        "watching Composition variant `{}` at {}",
        args.variant,
        root.display()
    );
    loop {
        let fingerprint = project_fingerprint(&root)?;
        let execution = match resolve_variant_for_execution(
            &args.recipe,
            &args.variant,
            &args.execution_classes,
        ) {
            Ok(execution) => execution,
            Err(error) => {
                eprintln!("Composition variant resolution failed: {error:#}");
                if wait_for_project_change(&root, fingerprint).await? == DevDecision::Stop {
                    return Ok(());
                }
                continue;
            }
        };
        println!("running {} ({})", execution.variant, execution.fingerprint);
        match run_product_runner_until_change(&execution, &args.runner_args, fingerprint).await? {
            DevDecision::Restart => {
                println!("source changed; resolving a fresh App Plan");
            }
            DevDecision::AwaitChange => {
                eprintln!("product Runner stopped; waiting for a source change");
                if wait_for_project_change(&root, fingerprint).await? == DevDecision::Stop {
                    return Ok(());
                }
            }
            DevDecision::Stop => return Ok(()),
        }
    }
}

async fn run_product_runner_until_change(
    execution: &ResolvedVariantExecution,
    runner_args: &[String],
    fingerprint: [u8; 32],
) -> anyhow::Result<DevDecision> {
    let mut command = tokio::process::Command::new(execution.runner.program());
    command
        .args(execution.runner.args())
        .args(runner_args)
        .current_dir(&execution.root)
        .env("LENSO_RESOLVED_PLAN", &execution.plan)
        .env("LENSO_COMPOSITION_VARIANT", &execution.variant)
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command.spawn().with_context(|| {
        format!(
            "start product Runner `{}` for Composition variant `{}`",
            execution.runner.program(),
            execution.variant
        )
    })?;
    let mut interval = tokio::time::interval(Duration::from_millis(350));
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.context("wait for product Runner")?;
                if !status.success() {
                    eprintln!("product Runner exited with {status}");
                }
                return Ok(DevDecision::AwaitChange);
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("wait for Ctrl-C")?;
                stop_process_group_child(&mut child).await;
                return Ok(DevDecision::Stop);
            }
            _ = interval.tick() => {
                match project_fingerprint(&execution.root) {
                    Ok(next) if next != fingerprint => {
                        stop_process_group_child(&mut child).await;
                        return Ok(DevDecision::Restart);
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("could not inspect App sources: {error:#}"),
                }
            }
        }
    }
}

#[derive(Debug)]
struct ResolvedVariantExecution {
    fingerprint: String,
    plan: PathBuf,
    root: PathBuf,
    runner: CompositionRunner,
    variant: String,
}

fn resolve_variant_for_execution(
    recipe_path: &Path,
    variant: &str,
    execution_classes: &[String],
) -> anyhow::Result<ResolvedVariantExecution> {
    let path = CompositionRecipePath::new(recipe_path);
    let recipe = path.load()?;
    let runner = recipe.runner().cloned().ok_or_else(|| {
        anyhow::anyhow!(
            "Composition recipe {} defines no product Runner",
            recipe_path.display()
        )
    })?;
    let materialized = path.materialize(&recipe, variant)?;
    let execution_classes = recipe_execution_classes(&recipe, execution_classes);
    let mut options =
        ResolutionOptions::default().with_check_options(check_options(execution_classes));
    if let Some(profile) = materialized.profile() {
        options = options.with_profile(profile);
    }
    let resolved = materialized
        .project()
        .resolve(materialized.root(), &options)?;
    let plan = materialized
        .root()
        .join(".lenso/compose")
        .join(variant)
        .join("resolved-plan.json");
    if let Some(parent) = plan.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&plan, resolved.canonical_bytes())?;
    Ok(ResolvedVariantExecution {
        fingerprint: resolved.fingerprint().clone(),
        plan,
        root: materialized.root().to_owned(),
        runner,
        variant: variant.to_owned(),
    })
}

fn selected_recipe_variants<'a>(
    recipe: &'a lenso_authoring::CompositionRecipe,
    selected: Option<&'a str>,
) -> anyhow::Result<Vec<&'a str>> {
    if let Some(name) = selected {
        if recipe.variant(name).is_none() {
            anyhow::bail!("Composition variant `{name}` is not defined");
        }
        Ok(vec![name])
    } else {
        Ok(recipe.variants().keys().map(String::as_str).collect())
    }
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
        let mut codec_cache = DevelopmentCodecCache::default();
        let bun = bun_adapter_with_development_codecs(
            BunAdapter::production(args.bun.clone()).with_config(config),
            resolved.plan(),
            &mut codec_cache,
        );
        adapters = adapters.with_adapter(bun)?;
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

pub(crate) async fn dev_module(
    repo_root: Option<&Path>,
    project: &Path,
    bun: &str,
) -> anyhow::Result<()> {
    let root = module_root(repo_root)?;
    let project_path = root.join(project);
    let loaded = ProjectPath::load(&project_path)
        .with_context(|| format!("load Module authoring project {}", project_path.display()))?;
    let execution_classes = inferred_execution_classes(&loaded)?;
    match execution_classes.as_slice() {
        [class] if class == "lenso.bun-process@1" => dev_bun(Some(&root), project, bun).await,
        [class] if class == "lenso.native-rust@1" => dev_native(&root, &project_path).await,
        [] => anyhow::bail!("Module project contains no Module Instances"),
        classes => anyhow::bail!(
            "`lenso module dev` supports one inferred execution class; found {}. Use an App Runner for mixed execution classes",
            classes.join(", ")
        ),
    }
}

pub(crate) fn check_module(options: &ModuleCheckOptions) -> anyhow::Result<()> {
    let root = module_root(options.repo_root.as_deref())?;
    let project_path = root.join(&options.project);
    let mut checks = Vec::new();
    let loaded = match ProjectPath::load(&project_path) {
        Ok(project) => {
            checks.push(ok_check(
                "project",
                "composition",
                "App Composition",
                &project_path,
                "authoring project parsed",
            ));
            project
        }
        Err(error) => {
            checks.push(authoring_failure_check(&error, &project_path));
            let report = ModuleAuthoringReport {
                artifact_version: "lenso.module-authoring-report.v1",
                status: "failed",
                project: project_path.display().to_string(),
                execution_classes: Vec::new(),
                checks,
            };
            print_authoring_report(&report, options.json)?;
            anyhow::bail!("Module authoring check failed");
        }
    };
    let execution_classes = inferred_execution_classes(&loaded)?;
    let check_options = CheckOptions::new(execution_classes.clone());
    match loaded.check(&root, &check_options) {
        Ok(report) => {
            checks.push(ok_check(
                "contracts",
                "capability",
                "Capability authoring",
                &project_path,
                &format!("{} generated contract input(s) are fresh", report.contracts),
            ));
            checks.push(ok_check(
                "packages",
                "package",
                "Package manager",
                &project_path,
                &format!("{} Module package lock(s) agree", report.modules),
            ));
            checks.push(ok_check(
                "composition",
                "composition",
                "App Composition",
                &project_path,
                &format!("{} binding(s) resolve", report.bindings),
            ));
        }
        Err(error) => checks.push(authoring_failure_check(&error, &project_path)),
    }
    let failed = checks.iter().any(|check| check.status == "failed");
    let report = ModuleAuthoringReport {
        artifact_version: "lenso.module-authoring-report.v1",
        status: if failed { "failed" } else { "ready" },
        project: project_path.display().to_string(),
        execution_classes,
        checks,
    };
    print_authoring_report(&report, options.json)?;
    if failed {
        anyhow::bail!("Module authoring check failed");
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(crate) fn verify_module(options: ModuleVerifyOptions) -> anyhow::Result<()> {
    let root = module_root(options.repo_root.as_deref())?;
    let project_path = root.join(&options.project);
    let manifest_path = root.join(&options.manifest);
    let output_path = root.join(&options.output);
    let project = ProjectPath::load(&project_path)?;
    let execution_classes = inferred_execution_classes(&project)?;
    let resolution_options =
        ResolutionOptions::default().with_check_options(CheckOptions::new(execution_classes));
    let (plan_fingerprint, mut failed) = match project.resolve(&root, &resolution_options) {
        Ok(resolved) => (Some(resolved.fingerprint()), false),
        Err(error) => {
            eprintln!("composition verification failed: {error}");
            (None, true)
        }
    };

    let mut probes = Vec::new();
    match read_verification_manifest(&manifest_path) {
        Ok(manifest) => {
            let required = [
                "package",
                "success",
                "domain_error",
                "runtime_failure",
                "lifecycle_cleanup",
            ];
            for purpose in required {
                if !manifest.probes.iter().any(|probe| probe.purpose == purpose) {
                    failed = true;
                    probes.push(VerificationProbeEvidence {
                        id: format!("missing-{purpose}"),
                        purpose: purpose.to_owned(),
                        command: String::new(),
                        status: "failed",
                        exit_code: None,
                        detail: Some(format!(
                            "verification manifest is missing `{purpose}` proof"
                        )),
                    });
                }
            }
            for probe in manifest.probes {
                let status = verification_command(&probe.command)
                    .current_dir(&root)
                    .status()
                    .with_context(|| format!("run verification probe {}", probe.id))?;
                let passed = status.success() != probe.expect_failure;
                failed |= !passed;
                probes.push(VerificationProbeEvidence {
                    id: probe.id,
                    purpose: probe.purpose,
                    command: probe.command,
                    status: if passed { "passed" } else { "failed" },
                    exit_code: status.code(),
                    detail: probe
                        .expect_failure
                        .then(|| "expected the command to fail".to_owned()),
                });
            }
        }
        Err(error) => {
            failed = true;
            probes.push(VerificationProbeEvidence {
                id: "verification-manifest".to_owned(),
                purpose: "behavior".to_owned(),
                command: String::new(),
                status: "failed",
                exit_code: None,
                detail: Some(error.to_string()),
            });
        }
    }

    let targets = if let Some(module_key) = options.module_key {
        if !project
            .composition()
            .modules()
            .iter()
            .any(|module| module.key() == module_key)
        {
            anyhow::bail!("Module Instance `{module_key}` is not in the App Composition");
        }
        vec![module_key]
    } else {
        project
            .composition()
            .modules()
            .iter()
            .map(|module| module.key().to_owned())
            .collect()
    };
    let mut removal_proofs = Vec::new();
    for target in targets {
        let proof = removal_proof(&project, &root, &resolution_options, &target);
        failed |= proof.status != "passed";
        removal_proofs.push(proof);
    }
    let evidence = VerificationEvidence {
        artifact_version: "lenso.module-verification.v1",
        status: if failed { "failed" } else { "passed" },
        project: project_path.display().to_string(),
        plan_fingerprint,
        probes,
        removal_proofs,
    };
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &output_path,
        format!("{}\n", serde_json::to_string_pretty(&evidence)?),
    )?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&evidence)?);
    } else {
        println!("Module verification: {}", evidence.status);
        println!("- evidence: {}", output_path.display());
        for proof in &evidence.removal_proofs {
            println!(
                "- removal {}: {} ({})",
                proof.requested_module, proof.status, proof.detail
            );
        }
    }
    if failed {
        anyhow::bail!(
            "Module verification failed; inspect {}",
            output_path.display()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn verification_command(command: &str) -> Command {
    let mut process = Command::new("cmd");
    process.args(["/C", command]);
    process
}

#[cfg(not(windows))]
fn verification_command(command: &str) -> Command {
    let mut process = Command::new("sh");
    process.args(["-c", command]);
    process
}

fn module_root(repo_root: Option<&Path>) -> anyhow::Result<PathBuf> {
    let current = std::env::current_dir().context("resolve current directory")?;
    Ok(match repo_root {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => current.join(path),
        None => current,
    })
}

fn inferred_execution_classes(project: &ProjectFile) -> anyhow::Result<Vec<String>> {
    let mut classes = BTreeSet::new();
    for module in project.composition().modules() {
        let class = module
            .execution_class()
            .map(ToOwned::to_owned)
            .or_else(|| {
                project
                    .packages()
                    .get(module.package())
                    .and_then(|package| package.source().default_execution_class())
                    .map(ToOwned::to_owned)
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Module Instance `{}` has no inferable execution class",
                    module.key()
                )
            })?;
        classes.insert(class);
    }
    Ok(classes.into_iter().collect())
}

fn ok_check(
    id: &str,
    layer: &'static str,
    owner: &'static str,
    path: &Path,
    message: &str,
) -> ModuleAuthoringCheck {
    ModuleAuthoringCheck {
        id: id.to_owned(),
        layer,
        owner,
        status: "passed",
        path: path.display().to_string(),
        message: message.to_owned(),
        fix: None,
    }
}

fn authoring_failure_check(error: &AuthoringError, fallback: &Path) -> ModuleAuthoringCheck {
    let (id, layer, owner, path, fix) = match error {
        AuthoringError::Contract { path, .. } => (
            "contracts",
            "capability",
            "Capability authoring",
            path.as_path(),
            "Regenerate the package-local bindings from this Descriptor, then rerun `lenso module check`.",
        ),
        AuthoringError::LockMismatch { .. } | AuthoringError::PackageManager { .. } => (
            "packages",
            "package",
            "Package manager",
            fallback,
            "Refresh the ordinary package lock without editing its resolved identity by hand.",
        ),
        AuthoringError::UnavailableExecutionClass { .. }
        | AuthoringError::MissingEntrypoint { .. } => (
            "execution",
            "adapter",
            "Execution Adapter",
            fallback,
            "Select an installed execution class and an exact package entrypoint.",
        ),
        AuthoringError::InvalidConfiguration { .. } | AuthoringError::SecretValue { .. } => (
            "configuration",
            "module",
            "Module",
            fallback,
            "Make configuration match its Schema and replace secret values with secret references.",
        ),
        AuthoringError::Plan { .. } | AuthoringError::InvalidProfile { .. } => (
            "composition",
            "composition",
            "App Composition",
            fallback,
            "Declare every requirement and binding explicitly, then resolve a fresh Plan.",
        ),
        _ => (
            "project",
            "authoring",
            "Authoring CLI",
            fallback,
            "Correct the reported authoring input and rerun `lenso module check`.",
        ),
    };
    ModuleAuthoringCheck {
        id: id.to_owned(),
        layer,
        owner,
        status: "failed",
        path: path.display().to_string(),
        message: error.to_string(),
        fix: Some(fix.to_owned()),
    }
}

fn print_authoring_report(report: &ModuleAuthoringReport, json: bool) -> anyhow::Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(report)?);
    } else {
        println!("Module authoring: {}", report.status);
        println!("- project: {}", report.project);
        println!("- execution: {}", report.execution_classes.join(", "));
        for check in &report.checks {
            println!("- {}: {} — {}", check.id, check.status, check.message);
            if let Some(fix) = &check.fix {
                println!("  fix: {fix}");
            }
        }
    }
    Ok(())
}

fn read_verification_manifest(path: &Path) -> anyhow::Result<VerificationManifest> {
    let manifest: VerificationManifest = serde_json::from_slice(
        &fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))?;
    if manifest.protocol != "lenso.module-verification-manifest.v1" {
        anyhow::bail!(
            "unsupported verification manifest protocol `{}`",
            manifest.protocol
        );
    }
    Ok(manifest)
}

fn removal_proof(
    project: &ProjectFile,
    root: &Path,
    options: &ResolutionOptions,
    target: &str,
) -> RemovalProof {
    let mut candidate = project.clone();
    let mut removed = BTreeSet::from([target.to_owned()]);
    loop {
        let mut next = removed.clone();
        for binding in candidate.composition().bindings() {
            if !removed.contains(binding.provider()) || removed.contains(binding.consumer()) {
                continue;
            }
            let required = candidate
                .composition()
                .modules()
                .iter()
                .find(|module| module.key() == binding.consumer())
                .and_then(|module| {
                    module.requires().iter().find(|requirement| {
                        requirement.capability_id() == binding.capability_id()
                            && requirement.descriptor_version() == binding.descriptor_version()
                    })
                })
                .is_some_and(|requirement| requirement.cardinality() != Cardinality::Optional);
            if required {
                next.insert(binding.consumer().to_owned());
            }
        }
        if next == removed {
            break;
        }
        removed = next;
    }
    candidate
        .composition_mut()
        .modules_mut()
        .retain(|module| !removed.contains(module.key()));
    candidate
        .composition_mut()
        .bindings_mut()
        .retain(|binding| {
            !removed.contains(binding.consumer()) && !removed.contains(binding.provider())
        });
    let remaining_modules = candidate.composition().modules().len();
    match candidate.resolve(root, options) {
        Ok(_) => RemovalProof {
            requested_module: target.to_owned(),
            removed_modules: removed.into_iter().collect(),
            remaining_modules,
            status: "passed",
            detail: "remaining App Composition resolves without hidden runtime mutation".to_owned(),
        },
        Err(error) => RemovalProof {
            requested_module: target.to_owned(),
            removed_modules: removed.into_iter().collect(),
            remaining_modules,
            status: "failed",
            detail: error.to_string(),
        },
    }
}

async fn dev_native(root: &Path, project_path: &Path) -> anyhow::Result<()> {
    let runner = root.join("src/bin/lenso-module-dev.rs");
    if !runner.is_file() {
        anyhow::bail!(
            "native Module development needs {}; create a standalone Rust scaffold or add a statically linked development Runner",
            runner.display()
        );
    }
    println!("watching native Rust Module project at {}", root.display());
    loop {
        let fingerprint = project_fingerprint(root)?;
        let project = match ProjectPath::load(project_path) {
            Ok(project) => project,
            Err(error) => {
                eprintln!("Rust Module project load failed: {error}");
                if wait_for_project_change(root, fingerprint).await? == DevDecision::Stop {
                    return Ok(());
                }
                continue;
            }
        };
        let options = ResolutionOptions::default()
            .with_check_options(CheckOptions::new(["lenso.native-rust@1"]));
        let resolved = match project.resolve(root, &options) {
            Ok(resolved) => resolved,
            Err(error) => {
                eprintln!("Rust Module check failed: {error}");
                if wait_for_project_change(root, fingerprint).await? == DevDecision::Stop {
                    return Ok(());
                }
                continue;
            }
        };
        let plan_path = root.join(".lenso/resolved-plan.json");
        if let Some(parent) = plan_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&plan_path, resolved.canonical_bytes())?;
        println!(
            "resolved {} ({})",
            plan_path.display(),
            resolved.fingerprint()
        );
        match run_native_until_change(root, fingerprint).await? {
            DevDecision::Restart => println!("source changed; resolving a fresh App Plan"),
            DevDecision::AwaitChange => {
                eprintln!("native runtime stopped; waiting for a source change");
                if wait_for_project_change(root, fingerprint).await? == DevDecision::Stop {
                    return Ok(());
                }
            }
            DevDecision::Stop => return Ok(()),
        }
    }
}

async fn run_native_until_change(
    root: &Path,
    fingerprint: [u8; 32],
) -> anyhow::Result<DevDecision> {
    let mut command = tokio::process::Command::new("cargo");
    command
        .args([
            "run",
            "--quiet",
            "--bin",
            "lenso-module-dev",
            "--",
            ".lenso/resolved-plan.json",
        ])
        .current_dir(root)
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
    let mut child = command
        .spawn()
        .context("start the statically linked native Module development Runner")?;
    let mut interval = tokio::time::interval(Duration::from_millis(350));
    loop {
        tokio::select! {
            status = child.wait() => {
                let status = status.context("wait for native Module development Runner")?;
                if !status.success() {
                    eprintln!("native Module Runner exited with {status}");
                }
                return Ok(DevDecision::AwaitChange);
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("wait for Ctrl-C")?;
                stop_process_group_child(&mut child).await;
                return Ok(DevDecision::Stop);
            }
            _ = interval.tick() => {
                match project_fingerprint(root) {
                    Ok(next) if next != fingerprint => {
                        stop_process_group_child(&mut child).await;
                        return Ok(DevDecision::Restart);
                    }
                    Ok(_) => {}
                    Err(error) => eprintln!("could not inspect Rust Module sources: {error:#}"),
                }
            }
        }
    }
}

async fn stop_process_group_child(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(id) = child.id() {
        use nix::{sys::signal::Signal, unistd::Pid};

        if nix::sys::signal::killpg(Pid::from_raw(id.cast_signed()), Signal::SIGINT).is_ok()
            && tokio::time::timeout(Duration::from_secs(10), child.wait())
                .await
                .is_ok()
        {
            return;
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

pub(crate) async fn dev_bun(
    repo_root: Option<&Path>,
    project: &Path,
    bun: &str,
) -> anyhow::Result<()> {
    let root = repo_root.map_or_else(
        || std::env::current_dir().context("resolve current directory"),
        |path| {
            if path.is_absolute() {
                Ok(path.to_path_buf())
            } else {
                Ok(std::env::current_dir()
                    .context("resolve current directory")?
                    .join(path))
            }
        },
    )?;
    let project_path = if project.is_absolute() {
        project.to_path_buf()
    } else {
        root.join(project)
    };
    println!("watching Bun Module project at {}", root.display());
    let mut codec_cache = DevelopmentCodecCache::default();

    loop {
        let fingerprint = project_fingerprint(&root)?;
        let resolved = match resolve_bun_project(&root, &project_path) {
            Ok(resolved) => resolved,
            Err(error) => {
                eprintln!("Bun Module check failed: {error:#}");
                if wait_for_project_change(&root, fingerprint).await? == DevDecision::Stop {
                    return Ok(());
                }
                continue;
            }
        };
        let plan_path = root.join(".lenso/resolved-plan.json");
        if let Some(parent) = plan_path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(&plan_path, resolved.canonical_bytes())
            .with_context(|| format!("write {}", plan_path.display()))?;
        println!(
            "resolved {} ({})",
            plan_path.display(),
            resolved.fingerprint()
        );

        let codecs = codec_cache.for_plan(resolved.plan());
        match run_bun_until_change(&root, bun, resolved, codecs, fingerprint).await? {
            DevDecision::Restart => println!("source changed; resolving a fresh App Plan"),
            DevDecision::AwaitChange => {
                eprintln!("Bun runtime stopped; waiting for a source change");
                if wait_for_project_change(&root, fingerprint).await? == DevDecision::Stop {
                    return Ok(());
                }
                println!("source changed; resolving a fresh App Plan");
            }
            DevDecision::Stop => return Ok(()),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DevelopmentCodecKey {
    capability_id: String,
    descriptor_version: String,
    operations: Vec<String>,
    request_operations: Vec<String>,
    stream_operations: Vec<String>,
    event_operations: Vec<String>,
}

impl DevelopmentCodecKey {
    fn from_endpoint(endpoint: &CapabilityEndpointPlan) -> Self {
        Self {
            capability_id: endpoint.capability_id().to_owned(),
            descriptor_version: endpoint.descriptor_version().to_owned(),
            operations: endpoint.operations().to_vec(),
            request_operations: endpoint
                .request_operations()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            stream_operations: endpoint
                .stream_operations()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
            event_operations: endpoint
                .event_operations()
                .into_iter()
                .map(ToOwned::to_owned)
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
struct DevelopmentJsonCodec {
    capability_id: &'static str,
    descriptor_version: &'static str,
    operations: &'static [&'static str],
    request_operations: &'static [&'static str],
    stream_operations: &'static [&'static str],
    event_operations: &'static [&'static str],
}

impl DevelopmentJsonCodec {
    fn new(key: &DevelopmentCodecKey) -> Self {
        let operations = key
            .operations
            .iter()
            .map(|operation| (operation.clone(), intern_for_process(operation)))
            .collect::<BTreeMap<_, _>>();
        Self {
            capability_id: intern_for_process(&key.capability_id),
            descriptor_version: intern_for_process(&key.descriptor_version),
            operations: intern_operations(&key.operations, &operations),
            request_operations: intern_operations(&key.request_operations, &operations),
            stream_operations: intern_operations(&key.stream_operations, &operations),
            event_operations: intern_operations(&key.event_operations, &operations),
        }
    }

    fn encode_json(&self, value: &dyn Any) -> Result<Value, RuntimeFailure> {
        value
            .downcast_ref::<Value>()
            .cloned()
            .ok_or(RuntimeFailure::ProtocolViolation {
                capability: self.capability_id,
            })
    }
}

impl BunCapabilityCodec for DevelopmentJsonCodec {
    fn capability_id(&self) -> &'static str {
        self.capability_id
    }

    fn descriptor_version(&self) -> &'static str {
        self.descriptor_version
    }

    fn operations(&self) -> &'static [&'static str] {
        self.operations
    }

    fn request_operations(&self) -> &'static [&'static str] {
        self.request_operations
    }

    fn stream_operations(&self) -> &'static [&'static str] {
        self.stream_operations
    }

    fn event_operations(&self) -> &'static [&'static str] {
        self.event_operations
    }

    fn encode_request(&self, _operation: &str, request: &dyn Any) -> Result<Value, RuntimeFailure> {
        self.encode_json(request)
    }

    fn decode_response(
        &self,
        _operation: &str,
        value: Value,
    ) -> Result<Box<dyn Any>, RuntimeFailure> {
        Ok(Box::new(value))
    }

    fn decode_domain_error(
        &self,
        _operation: &str,
        value: Value,
    ) -> Result<Box<dyn Any>, RuntimeFailure> {
        Ok(Box::new(value))
    }

    fn encode_stream_message(
        &self,
        _operation: &str,
        message: &dyn Any,
    ) -> Result<Value, RuntimeFailure> {
        self.encode_json(message)
    }
}

#[derive(Debug, Default)]
struct DevelopmentCodecCache {
    codecs: BTreeMap<DevelopmentCodecKey, DevelopmentJsonCodec>,
}

impl DevelopmentCodecCache {
    fn for_plan(&mut self, plan: &ResolvedAppPlan) -> Vec<DevelopmentJsonCodec> {
        let mut codecs = Vec::new();
        for instance in plan
            .module_instances()
            .iter()
            .filter(|instance| instance.execution_class() == &ExecutionClassId::bun_child_process())
        {
            for endpoint in instance.provided_capabilities() {
                let key = DevelopmentCodecKey::from_endpoint(endpoint);
                let codec = self
                    .codecs
                    .entry(key.clone())
                    .or_insert_with(|| DevelopmentJsonCodec::new(&key));
                codecs.push(codec.clone());
            }
        }
        codecs
    }
}

fn bun_adapter_with_development_codecs(
    mut adapter: BunAdapter,
    plan: &ResolvedAppPlan,
    cache: &mut DevelopmentCodecCache,
) -> BunAdapter {
    for codec in cache.for_plan(plan) {
        adapter = adapter.with_codec(codec);
    }
    adapter
}

fn intern_for_process(value: &str) -> &'static str {
    // Native endpoint identities are process-static. The cache above interns each
    // distinct Descriptor shape once, even when ordinary source edits restart it.
    Box::leak(value.to_owned().into_boxed_str())
}

fn intern_operations(
    names: &[String],
    operations: &BTreeMap<String, &'static str>,
) -> &'static [&'static str] {
    let values = names
        .iter()
        .map(|name| {
            operations
                .get(name)
                .copied()
                .expect("operation kind must reference a declared operation")
        })
        .collect::<Vec<_>>();
    Box::leak(values.into_boxed_slice())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DevDecision {
    AwaitChange,
    Restart,
    Stop,
}

fn resolve_bun_project(root: &Path, project_path: &Path) -> anyhow::Result<ResolvedProject> {
    let project = ProjectPath::load(project_path)?;
    let options =
        ResolutionOptions::default().with_check_options(CheckOptions::new(["lenso.bun-process@1"]));
    project.resolve(root, &options).map_err(Into::into)
}

async fn run_bun_until_change(
    root: &Path,
    bun: &str,
    resolved: ResolvedProject,
    codecs: Vec<DevelopmentJsonCodec>,
    fingerprint: [u8; 32],
) -> anyhow::Result<DevDecision> {
    let config = BunAdapterConfig::new(bun, BunWire::JsonRpcHttp).with_working_directory(root);
    let mut bun_adapter = BunAdapter::production(bun.to_owned()).with_config(config);
    for codec in codecs {
        bun_adapter = bun_adapter.with_codec(codec);
    }
    let adapters = ExecutionAdapterCatalog::new().with_adapter(bun_adapter)?;
    let driver = TokioDriver::new();
    let shutdown_driver = driver.clone();
    let root = root.to_path_buf();
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            let runtime = run_project(
                &resolved,
                driver,
                adapters,
                Duration::from_secs(10),
            );
            tokio::pin!(runtime);
            let mut interval = tokio::time::interval(Duration::from_millis(350));
            loop {
                tokio::select! {
                    outcome = &mut runtime => {
                        match outcome {
                            Ok(outcome) => println!("{outcome:?}"),
                            Err(error) => eprintln!("Bun runtime failed: {error}"),
                        }
                        return Ok(DevDecision::AwaitChange);
                    }
                    signal = tokio::signal::ctrl_c() => {
                        signal.context("wait for Ctrl-C")?;
                        shutdown_driver.request_shutdown();
                        match runtime.await {
                            Ok(outcome) => println!("{outcome:?}"),
                            Err(error) => eprintln!("Bun runtime failed during shutdown: {error}"),
                        }
                        return Ok(DevDecision::Stop);
                    }
                    _ = interval.tick() => {
                        match project_fingerprint(&root) {
                            Ok(next) if next != fingerprint => {
                                shutdown_driver.request_shutdown();
                                match runtime.await {
                                    Ok(outcome) => println!("{outcome:?}"),
                                    Err(error) => eprintln!("Bun runtime failed during restart: {error}"),
                                }
                                return Ok(DevDecision::Restart);
                            }
                            Ok(_) => {}
                            Err(error) => eprintln!("could not inspect Bun Module sources: {error:#}"),
                        }
                    }
                }
            }
        })
        .await
}

async fn wait_for_project_change(
    root: &Path,
    fingerprint: [u8; 32],
) -> anyhow::Result<DevDecision> {
    let mut interval = tokio::time::interval(Duration::from_millis(350));
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("wait for Ctrl-C")?;
                return Ok(DevDecision::Stop);
            }
            _ = interval.tick() => {
                match project_fingerprint(root) {
                    Ok(next) if next != fingerprint => return Ok(DevDecision::Restart),
                    Ok(_) => {}
                    Err(error) => eprintln!("could not inspect project sources: {error:#}"),
                }
            }
        }
    }
}

fn project_fingerprint(root: &Path) -> anyhow::Result<[u8; 32]> {
    let mut files = Vec::new();
    collect_project_files(root, &mut files)?;
    files.sort();
    let mut digest = Sha256::new();
    for path in files {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        digest.update(relative.to_string_lossy().as_bytes());
        digest.update([0]);
        digest.update(
            fs::read(&path).with_context(|| format!("read watched file {}", path.display()))?,
        );
        digest.update([0]);
    }
    Ok(digest.finalize().into())
}

fn collect_project_files(directory: &Path, files: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("read watched directory {}", directory.display()))?
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(
            name.as_ref(),
            ".git" | ".lenso" | "node_modules" | "target" | ".DS_Store"
        ) {
            continue;
        }
        let file_type = entry
            .file_type()
            .with_context(|| format!("inspect watched path {}", entry.path().display()))?;
        if file_type.is_dir() {
            collect_project_files(&entry.path(), files)?;
        } else if file_type.is_file() {
            files.push(entry.path());
        }
    }
    Ok(())
}

fn check_options(execution_classes: &[String]) -> CheckOptions {
    if execution_classes.is_empty() {
        CheckOptions::default()
    } else {
        CheckOptions::new(execution_classes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_json_codec_preserves_the_resolved_endpoint_shape() {
        let endpoint = CapabilityEndpointPlan::new(
            "example.activity@1",
            "1.2.0",
            ["request", "subscribe", "publish"],
        )
        .with_stream_operation("subscribe")
        .with_event_operation("publish");
        let codec = DevelopmentJsonCodec::new(&DevelopmentCodecKey::from_endpoint(&endpoint));
        let request = serde_json::json!({ "message": "hello" });

        assert_eq!(codec.capability_id(), "example.activity@1");
        assert_eq!(codec.descriptor_version(), "1.2.0");
        assert_eq!(codec.operations(), ["request", "subscribe", "publish"]);
        assert_eq!(codec.request_operations(), ["request"]);
        assert_eq!(codec.stream_operations(), ["subscribe"]);
        assert_eq!(codec.event_operations(), ["publish"]);
        assert_eq!(codec.encode_request("request", &request).unwrap(), request);

        let response = serde_json::json!({ "accepted": true });
        let decoded = codec
            .decode_response("request", response.clone())
            .unwrap()
            .downcast::<Value>()
            .unwrap();
        assert_eq!(*decoded, response);
    }

    #[test]
    fn project_fingerprint_changes_when_module_source_changes() {
        let root = std::env::temp_dir().join(format!(
            "lenso-cli-dev-watch-source-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(root.join("modules/example/src")).unwrap();
        let source = root.join("modules/example/src/index.ts");
        fs::write(&source, "export const value = 1;\n").unwrap();
        let before = project_fingerprint(&root).unwrap();

        fs::write(&source, "export const value = 2;\n").unwrap();
        let after = project_fingerprint(&root).unwrap();

        assert_ne!(before, after);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_fingerprint_ignores_runtime_output_directories() {
        let root = std::env::temp_dir().join(format!(
            "lenso-cli-dev-watch-output-{}",
            uuid::Uuid::now_v7()
        ));
        fs::create_dir_all(root.join("modules/example/src")).unwrap();
        fs::create_dir_all(root.join("node_modules/example")).unwrap();
        fs::write(
            root.join("modules/example/src/index.ts"),
            "export const value = 1;\n",
        )
        .unwrap();
        let ignored = root.join("node_modules/example/index.js");
        fs::write(&ignored, "export const generated = 1;\n").unwrap();
        let before = project_fingerprint(&root).unwrap();

        fs::write(&ignored, "export const generated = 2;\n").unwrap();
        let after = project_fingerprint(&root).unwrap();

        assert_eq!(before, after);
        fs::remove_dir_all(root).unwrap();
    }
}
