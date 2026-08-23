use std::{
    any::Any,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use clap::Args;
use lenso_app_plan::{CapabilityEndpointPlan, ExecutionClassId, ResolvedAppPlan};
use lenso_authoring::{
    AddModule, CheckOptions, Module, PackageInput, PackageSource, ProjectAuthoring, ProjectPath,
    ResolutionOptions, ResolvedProject, run_project,
};
use lenso_bun_adapter::{BunAdapter, BunAdapterConfig, BunCapabilityCodec, BunWire};
use lenso_kernel::{ExecutionAdapterCatalog, RuntimeFailure};
use lenso_runner::TokioDriver;
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
            .ok_or_else(|| RuntimeFailure::ProtocolViolation {
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
                    Err(error) => eprintln!("could not inspect Bun Module sources: {error:#}"),
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
