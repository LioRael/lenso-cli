use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, bail};
use clap::{Args, Subcommand, ValueEnum};
use lenso_app_plan::{
    CapabilityEndpointPlan, CapabilityRequirementPlan, ExecutionClassId, authoring::PluginContract,
};
use lenso_plugin_bundle::{
    PluginManifest, SourcePluginBuild, SourcePluginImplementation, SourcePluginReleaseBuild,
    SourceProcessPluginBuild, VerifiedBundle, build_source_plugin_bundle,
    build_source_plugin_release_bundle, build_source_process_plugin_bundle,
    extract_plugin_descriptor, read_bundle_manifest, verify_bundle_directory,
};
use lenso_wasm_component_adapter::EXECUTION_CLASS as WASM_EXECUTION_CLASS;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::archive::{archive_bundle, with_bundle_directory};
use lenso_app_authoring::identity::{
    PluginIdVersion, classify_existing_plugin_id, validate_release_version,
};

mod dev;
mod scaffold;
mod web_dev;

const WASM_TARGET: &str = "wasm32-unknown-unknown";

#[derive(Clone, Debug, Subcommand)]
pub enum PluginCommand {
    /// Create an Agent Tool Plugin, or a linked Web Plugin with `--web`.
    New(PluginNewArgs),
    /// Build and run the Plugin through its SDK-selected execution adapter.
    Dev(PluginDevArgs),
    /// Validate Plugin source and generated descriptor evidence.
    Check(PluginCheckArgs),
    /// Build and verify one immutable `.lenso-plugin` archive.
    Pack(PluginPackArgs),
}

#[derive(Args, Clone, Debug)]
pub struct PluginNewArgs {
    /// Namespaced Plugin id, such as company.uppercase.
    plugin_id: String,
    /// Base directory for the new Plugin project.
    #[arg(long)]
    repo_root: Option<PathBuf>,
    /// New Plugin project directory. Defaults to the Plugin id.
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Implementation outputs. Multi builds Wasm and a native Process from the same source.
    #[arg(long, value_enum, default_value_t = PluginRuntimeArg::Multi)]
    runtime: PluginRuntimeArg,
    /// Create a linked native Rust HTTP Endpoint Plugin instead of an Agent Tool Plugin.
    #[arg(long, conflicts_with = "runtime")]
    pub(crate) web: bool,
    /// Skip lockfile generation and the initial compile check.
    #[arg(long)]
    no_install: bool,
    /// Print generated paths without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum PluginRuntimeArg {
    Multi,
    #[value(alias = "rust")]
    Wasm,
    Process,
    /// TypeScript Plugin executed through the Bun child-process Adapter.
    Bun,
}

#[derive(Args, Clone, Debug)]
pub struct PluginCheckArgs {
    /// Plugin project root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<PathBuf>,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Clone, Debug)]
pub struct PluginDevArgs {
    /// Plugin project root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<PathBuf>,
    /// Request operation to invoke. Defaults to the first declared operation.
    #[arg(long)]
    operation: Option<String>,
    /// JSON request passed to the Plugin.
    #[arg(long, default_value = "{}")]
    request_json: String,
    /// Emit a stable JSON result.
    #[arg(long)]
    json: bool,
    /// Rebuild and rerun after source or manifest changes.
    #[arg(long)]
    watch: bool,
    /// Implementation to build and invoke. Auto chooses the fastest declared local implementation.
    #[arg(long, value_enum, default_value_t = DevImplementationArg::Auto)]
    implementation: DevImplementationArg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DevImplementationArg {
    Auto,
    Wasm,
    Process,
    /// Build every declared implementation, then invoke the fastest local one.
    All,
}

#[derive(Args, Clone, Debug)]
pub struct PluginPackArgs {
    /// Plugin project root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<PathBuf>,
    /// Output `.lenso-plugin` archive. Defaults under `dist/`.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Emit a stable JSON result.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Deserialize)]
struct CargoDocument {
    package: CargoPackage,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    metadata: CargoMetadata,
}

#[derive(Debug, Deserialize)]
struct CargoTargetMetadata {
    target_directory: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    lenso: LensoMetadata,
    #[serde(default, rename = "lenso-cli")]
    lenso_cli: Option<LensoCliMetadata>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct LensoMetadata {
    plugin_id: String,
    #[serde(default)]
    root_slot: String,
}

#[derive(Debug, Deserialize)]
struct LensoCliMetadata {
    #[serde(default)]
    runtime: Option<String>,
    #[serde(default)]
    outputs: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProjectRuntime {
    Multi,
    Wasm,
    Process,
    Bun,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DevBuild {
    Wasm,
    Process,
    All,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DevSelection {
    build: DevBuild,
    invoke: ProjectRuntime,
}

#[derive(Debug, Deserialize)]
struct BunPackageDocument {
    #[serde(rename = "name")]
    _name: String,
    version: String,
    #[serde(default)]
    lenso: Option<BunPackageMetadata>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BunPackageMetadata {
    plugin_id: String,
    root_slot: String,
    runtime: String,
}

#[derive(Clone, Debug)]
struct BunPackage {
    version: String,
    metadata: BunPackageMetadata,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PluginDescriptor {
    abi: String,
    #[serde(default)]
    configuration_schema: Option<Value>,
    capabilities: Vec<PluginCapability>,
    #[serde(default)]
    required_capabilities: Vec<PluginRequirement>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PluginCapability {
    capability_id: String,
    descriptor_version: String,
    request_operations: Vec<String>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
struct PluginRequirement {
    requirement_id: String,
    capability_id: String,
    descriptor_version: String,
    cardinality: String,
}

pub async fn plugin(command: PluginCommand) -> anyhow::Result<()> {
    match command {
        PluginCommand::New(args) => scaffold::create(args),
        PluginCommand::Dev(args) => dev::run(args).await,
        PluginCommand::Check(args) => check(args),
        PluginCommand::Pack(args) => pack(args),
    }
}

fn check(args: PluginCheckArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root)?;
    let temporary = tempfile::tempdir().context("create Plugin check directory")?;
    let output = temporary.path().join("checked.lenso-plugin");
    let verified = materialize(&root, &output, BuildProfile::Development)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.plugin-check",
                "status": "passed",
                "plugin_id": verified.plugin_id,
                "release_version": verified.release_version,
                "manifest_digest": verified.manifest_digest,
            }))?
        );
    } else {
        println!(
            "Plugin check passed: {}@{}",
            verified.plugin_id, verified.release_version
        );
    }
    Ok(())
}

fn pack(args: PluginPackArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root)?;
    let bun_package = read_bun_package(&root)?;
    let (plugin_id, version) = if let Some(package) = bun_package.as_ref() {
        (&package.metadata.plugin_id, &package.version)
    } else {
        let package = read_package(&root.join("Cargo.toml"))?;
        let output = args.output.unwrap_or_else(|| {
            root.join("dist").join(format!(
                "{}-{}.lenso-plugin",
                package.metadata.lenso.plugin_id, package.version
            ))
        });
        return pack_to(&root, &output, args.json);
    };
    let output = args.output.unwrap_or_else(|| {
        root.join("dist")
            .join(format!("{plugin_id}-{version}.lenso-plugin"))
    });
    pack_to(&root, &output, args.json)
}

fn pack_to(root: &Path, output: &Path, json: bool) -> anyhow::Result<()> {
    let staging = tempfile::tempdir().context("stage packed Plugin Bundle")?;
    let directory = staging.path().join("bundle");
    let verified = materialize(root, &directory, BuildProfile::Release)?;
    archive_bundle(&directory, output)?;
    let reopened = with_bundle_directory(output, |directory| {
        verify_bundle_directory(directory)
            .with_context(|| format!("reopen packed Plugin `{}`", output.display()))
    })?;
    if verified != reopened {
        bail!("packed Plugin verification result changed after publication");
    }
    print_verified(&reopened, Some(output), json)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildProfile {
    Development,
    Release,
}

impl BuildProfile {
    const fn directory(self) -> &'static str {
        match self {
            Self::Development => "debug",
            Self::Release => "release",
        }
    }

    fn cargo_args<'a>(self, arguments: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
        let mut output = vec!["build", "--locked"];
        if self == Self::Release {
            output.push("--release");
        }
        output.extend(arguments);
        output
    }
}

fn read_bun_package(root: &Path) -> anyhow::Result<Option<BunPackage>> {
    let path = root.join("package.json");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let document: BunPackageDocument =
        serde_json::from_slice(&bytes).context("parse Bun Plugin package.json")?;
    let Some(metadata) = document.lenso else {
        return Ok(None);
    };
    if metadata.runtime != "bun" {
        bail!(
            "unsupported package.json Lenso runtime `{}`; expected `bun`",
            metadata.runtime
        );
    }
    warn_for_legacy_plugin_id(&metadata.plugin_id)?;
    if metadata.root_slot.trim().is_empty() {
        bail!("Bun Plugin rootSlot must not be empty");
    }
    validate_release_version(&document.version)?;
    Ok(Some(BunPackage {
        version: document.version,
        metadata,
    }))
}

fn materialize_bun(
    root: &Path,
    output: &Path,
    package: &BunPackage,
    profile: BuildProfile,
) -> anyhow::Result<(VerifiedBundle, PluginDescriptor)> {
    run_bun(root, &["run", "check"], "typecheck Bun Plugin")?;
    let staging = tempfile::tempdir().context("stage Bun Plugin implementation")?;
    let artifact = staging.path().join("plugin.js");
    let artifact_text = artifact.to_string_lossy().into_owned();
    let mut arguments = vec![
        "build",
        "src/lenso.bun.generated.ts",
        "--target=bun",
        "--format=esm",
        "--outfile",
        artifact_text.as_str(),
    ];
    if profile == BuildProfile::Release {
        arguments.push("--minify");
    }
    run_bun(root, &arguments, "build Bun Plugin implementation")?;
    let descriptor = describe_bun_plugin(root)?;
    let contract = contract_from_bun_descriptor(package, &descriptor)?;
    let verified = build_source_plugin_release_bundle(&SourcePluginReleaseBuild {
        contract,
        implementations: vec![SourcePluginImplementation {
            id: "bun".to_owned(),
            host_targets: vec!["*".to_owned()],
            artifact,
            bundle_path: "implementations/bun/plugin.js".to_owned(),
            media_type: "application/javascript".to_owned(),
            target: "javascript-bun".to_owned(),
            entrypoint: "plugin.js".to_owned(),
            execution_class: ExecutionClassId::bun_child_process(),
            runtime_profile: lenso_app_plan::PLUGIN_AUTHORING_V2_RUNTIME_PROFILE.to_owned(),
        }],
        output: output.to_path_buf(),
    })?;
    Ok((verified, descriptor))
}

fn contract_from_bun_descriptor(
    package: &BunPackage,
    descriptor: &PluginDescriptor,
) -> anyhow::Result<PluginContract> {
    let mut contract = descriptor.capabilities.iter().fold(
        PluginContract::new(
            &package.metadata.plugin_id,
            &package.version,
            &package.metadata.root_slot,
        )
        .with_authoring_version(2),
        |contract, capability| {
            contract.with_capability(CapabilityEndpointPlan::new(
                &capability.capability_id,
                &capability.descriptor_version,
                capability.request_operations.clone(),
            ))
        },
    );
    if let Some(schema) = &descriptor.configuration_schema {
        contract = contract.with_configuration_schema(schema.clone());
    }
    for requirement in &descriptor.required_capabilities {
        if requirement.cardinality != "one" {
            bail!(
                "Bun Plugin requirement `{}` uses unsupported cardinality `{}`",
                requirement.requirement_id,
                requirement.cardinality
            );
        }
        contract = contract.with_requirement(
            CapabilityRequirementPlan::one(
                &requirement.capability_id,
                &requirement.descriptor_version,
            )
            .with_requirement_id(&requirement.requirement_id),
        );
    }
    Ok(contract)
}

fn describe_bun_plugin(root: &Path) -> anyhow::Result<PluginDescriptor> {
    let output = run_bun_output(
        root,
        &["run", "src/lenso.describe.generated.ts"],
        "describe Bun Plugin",
    )?;
    let descriptor = parse_descriptor_bytes(&output)?;
    Ok(descriptor)
}

fn materialize(
    root: &Path,
    output: &Path,
    profile: BuildProfile,
) -> anyhow::Result<VerifiedBundle> {
    if let Some(package) = read_bun_package(root)? {
        return materialize_bun(root, output, &package, profile).map(|(verified, _)| verified);
    }
    let package = read_package(&root.join("Cargo.toml"))?;
    synchronize_plugin_lock(root, &package)?;
    let target_directory = cargo_target_directory(root)?;
    match project_runtime(&package)? {
        ProjectRuntime::Multi => {
            materialize_multi(root, output, &package, &target_directory, profile)
        }
        ProjectRuntime::Wasm => {
            materialize_wasm(root, output, &package, &target_directory, profile, false)
        }
        ProjectRuntime::Process => {
            materialize_process(root, output, &package, &target_directory, profile)
        }
        ProjectRuntime::Bun => unreachable!("Bun projects do not use Cargo metadata"),
    }
    .with_context(|| format!("package Plugin `{}`", package.metadata.lenso.plugin_id))
}

fn materialize_dev(
    root: &Path,
    output: &Path,
    package: &CargoPackage,
    declared_runtime: ProjectRuntime,
    build: DevBuild,
    profile: BuildProfile,
) -> anyhow::Result<VerifiedBundle> {
    synchronize_plugin_lock(root, package)?;
    let target_directory = cargo_target_directory(root)?;
    match build {
        DevBuild::All => materialize_multi(root, output, package, &target_directory, profile),
        DevBuild::Wasm => materialize_wasm(
            root,
            output,
            package,
            &target_directory,
            profile,
            declared_runtime == ProjectRuntime::Multi,
        ),
        DevBuild::Process => materialize_process(root, output, package, &target_directory, profile),
    }
    .with_context(|| {
        format!(
            "package Plugin `{}` for development",
            package.metadata.lenso.plugin_id
        )
    })
}

fn materialize_wasm(
    root: &Path,
    output: &Path,
    package: &CargoPackage,
    target_directory: &Path,
    profile: BuildProfile,
    explicit_library_target: bool,
) -> anyhow::Result<VerifiedBundle> {
    let arguments = if explicit_library_target {
        profile.cargo_args(["--lib", "--target", WASM_TARGET])
    } else {
        profile.cargo_args(["--target", WASM_TARGET])
    };
    run_cargo(root, &arguments, "build Plugin Wasm implementation")?;
    let artifact = target_directory
        .join(WASM_TARGET)
        .join(profile.directory())
        .join(format!("{}.wasm", package.name.replace('-', "_")));
    Ok(build_source_plugin_bundle(&SourcePluginBuild {
        package_manifest: root.join("Cargo.toml"),
        wasm_module: artifact,
        output: output.to_path_buf(),
    })?)
}

fn materialize_process(
    root: &Path,
    output: &Path,
    package: &CargoPackage,
    target_directory: &Path,
    profile: BuildProfile,
) -> anyhow::Result<VerifiedBundle> {
    let arguments = profile.cargo_args(["--bin", package.name.as_str()]);
    run_cargo(root, &arguments, "build Plugin Process implementation")?;
    let executable = target_directory
        .join(profile.directory())
        .join(&package.name);
    let descriptor = tempfile::NamedTempFile::new().context("stage Process descriptor")?;
    serde_json::to_writer(
        descriptor.as_file(),
        &dev::read_process_descriptor(&executable)?,
    )?;
    Ok(build_source_process_plugin_bundle(
        &SourceProcessPluginBuild {
            package_manifest: root.join("Cargo.toml"),
            executable,
            runtime_descriptor: descriptor.path().to_path_buf(),
            target: format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS),
            output: output.to_path_buf(),
        },
    )?)
}

fn materialize_multi(
    root: &Path,
    output: &Path,
    package: &CargoPackage,
    target_directory: &Path,
    profile: BuildProfile,
) -> anyhow::Result<VerifiedBundle> {
    let wasm_arguments = profile.cargo_args(["--lib", "--target", WASM_TARGET]);
    run_cargo(root, &wasm_arguments, "build Plugin Wasm implementation")?;
    let process_arguments = profile.cargo_args(["--bin", package.name.as_str()]);
    run_cargo(
        root,
        &process_arguments,
        "build Plugin Process implementation",
    )?;
    let staging = tempfile::tempdir().context("stage Plugin implementations")?;
    let wasm_bundle = staging.path().join("wasm");
    build_source_plugin_bundle(&SourcePluginBuild {
        package_manifest: root.join("Cargo.toml"),
        wasm_module: target_directory
            .join(WASM_TARGET)
            .join(profile.directory())
            .join(format!("{}.wasm", package.name.replace('-', "_"))),
        output: wasm_bundle.clone(),
    })?;
    let process_bundle = staging.path().join("process");
    let host_target = format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS);
    let executable = target_directory
        .join(profile.directory())
        .join(&package.name);
    let runtime_descriptor = staging.path().join("process-descriptor.json");
    fs::write(
        &runtime_descriptor,
        serde_json::to_vec(&dev::read_process_descriptor(&executable)?)?,
    )?;
    build_source_process_plugin_bundle(&SourceProcessPluginBuild {
        package_manifest: root.join("Cargo.toml"),
        executable,
        runtime_descriptor,
        target: host_target.clone(),
        output: process_bundle.clone(),
    })?;
    let wasm_descriptor = v2_descriptor(&wasm_bundle)?;
    let process_descriptor = v2_descriptor(&process_bundle)?;
    if wasm_descriptor.contract() != process_descriptor.contract() {
        bail!("Plugin implementations do not expose the same Contract");
    }
    let process_name = if cfg!(windows) {
        "plugin.exe"
    } else {
        "plugin"
    };
    Ok(build_source_plugin_release_bundle(
        &SourcePluginReleaseBuild {
            contract: wasm_descriptor.contract(),
            implementations: vec![
                SourcePluginImplementation {
                    id: "wasm".to_owned(),
                    host_targets: vec!["*".to_owned()],
                    artifact: wasm_bundle.join("plugin.wasm"),
                    bundle_path: "implementations/wasm/plugin.wasm".to_owned(),
                    media_type: "application/wasm".to_owned(),
                    target: WASM_TARGET.to_owned(),
                    entrypoint: "plugin".to_owned(),
                    execution_class: ExecutionClassId::new(WASM_EXECUTION_CLASS),
                    runtime_profile: wasm_descriptor.runtime_profile().to_owned(),
                },
                SourcePluginImplementation {
                    id: "process".to_owned(),
                    host_targets: vec![host_target.clone()],
                    artifact: process_bundle.join(process_name),
                    bundle_path: format!("implementations/process/{process_name}"),
                    media_type: "application/vnd.lenso.process".to_owned(),
                    target: host_target,
                    entrypoint: "plugin".to_owned(),
                    execution_class: ExecutionClassId::new("lenso.process@1"),
                    runtime_profile: process_descriptor.runtime_profile().to_owned(),
                },
            ],
            output: output.to_path_buf(),
        },
    )?)
}

fn v2_descriptor(root: &Path) -> anyhow::Result<lenso_app_plan::authoring::PluginDescriptor> {
    let PluginManifest::V2(manifest) = read_bundle_manifest(root)? else {
        bail!("implementation staging Bundle did not use V2")
    };
    serde_json::from_value(manifest.entry.descriptor)
        .context("parse staged Plugin implementation descriptor")
}

fn synchronize_plugin_lock(root: &Path, package: &CargoPackage) -> anyhow::Result<()> {
    run_cargo(
        root,
        &[
            "update",
            "--offline",
            "--package",
            &package.name,
            "--precise",
            &package.version,
        ],
        "synchronize Plugin version in Cargo.lock",
    )
}

fn cargo_target_directory(root: &Path) -> anyhow::Result<PathBuf> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args(["metadata", "--locked", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("inspect Plugin Cargo target directory")?;
    if !output.status.success() {
        bail!(
            "inspect Plugin Cargo target directory failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let metadata: CargoTargetMetadata =
        serde_json::from_slice(&output.stdout).context("parse Plugin Cargo metadata")?;
    Ok(metadata.target_directory)
}

fn read_package(manifest: &Path) -> anyhow::Result<CargoPackage> {
    let bytes = fs::read(manifest)
        .with_context(|| format!("read Plugin manifest {}", manifest.display()))?;
    let document: CargoDocument =
        toml::from_slice(&bytes).context("parse Plugin Cargo manifest")?;
    warn_for_legacy_plugin_id(&document.package.metadata.lenso.plugin_id)?;
    validate_release_version(&document.package.version)?;
    Ok(document.package)
}

fn project_runtime(package: &CargoPackage) -> anyhow::Result<ProjectRuntime> {
    let Some(metadata) = package.metadata.lenso_cli.as_ref() else {
        return Ok(ProjectRuntime::Wasm);
    };
    if !metadata.outputs.is_empty() {
        return match metadata.outputs.as_slice() {
            [output] if output == "wasm" => Ok(ProjectRuntime::Wasm),
            [output] if output == "process" => Ok(ProjectRuntime::Process),
            [wasm, process] if wasm == "wasm" && process == "process" => Ok(ProjectRuntime::Multi),
            outputs => bail!("unsupported Plugin implementation outputs `{outputs:?}`"),
        };
    }
    match metadata.runtime.as_deref().unwrap_or("wasm") {
        "wasm" => Ok(ProjectRuntime::Wasm),
        "process" => Ok(ProjectRuntime::Process),
        "bun" => Ok(ProjectRuntime::Bun),
        runtime => bail!("unsupported Plugin project runtime `{runtime}`"),
    }
}

fn resolve_dev_selection(
    declared: ProjectRuntime,
    implementation: DevImplementationArg,
) -> anyhow::Result<DevSelection> {
    match (declared, implementation) {
        (ProjectRuntime::Multi, DevImplementationArg::Auto | DevImplementationArg::Process) => {
            Ok(DevSelection {
                build: DevBuild::Process,
                invoke: ProjectRuntime::Process,
            })
        }
        (ProjectRuntime::Multi, DevImplementationArg::Wasm)
        | (
            ProjectRuntime::Wasm,
            DevImplementationArg::Auto | DevImplementationArg::Wasm | DevImplementationArg::All,
        ) => Ok(DevSelection {
            build: DevBuild::Wasm,
            invoke: ProjectRuntime::Wasm,
        }),
        (ProjectRuntime::Multi, DevImplementationArg::All) => Ok(DevSelection {
            build: DevBuild::All,
            invoke: ProjectRuntime::Process,
        }),
        (
            ProjectRuntime::Process,
            DevImplementationArg::Auto | DevImplementationArg::Process | DevImplementationArg::All,
        ) => Ok(DevSelection {
            build: DevBuild::Process,
            invoke: ProjectRuntime::Process,
        }),
        (ProjectRuntime::Wasm, DevImplementationArg::Process) => {
            bail!("Plugin project declares only a Wasm implementation")
        }
        (ProjectRuntime::Process, DevImplementationArg::Wasm) => {
            bail!("Plugin project declares only a Process implementation")
        }
        (ProjectRuntime::Bun, _) => bail!("Bun development is dispatched separately"),
    }
}

fn parse_descriptor(component: &[u8]) -> anyhow::Result<PluginDescriptor> {
    let bytes = extract_plugin_descriptor(component)?;
    parse_descriptor_bytes(&bytes)
}

fn parse_descriptor_bytes(bytes: &[u8]) -> anyhow::Result<PluginDescriptor> {
    let descriptor: PluginDescriptor =
        serde_json::from_slice(bytes).context("parse generated Plugin descriptor")?;
    if descriptor.abi != "lenso.json-request@1" {
        bail!(
            "unsupported Plugin descriptor ABI `{}`; expected request-only V1",
            descriptor.abi
        );
    }
    validate_capabilities(&descriptor)?;
    Ok(descriptor)
}

fn validate_capabilities(descriptor: &PluginDescriptor) -> anyhow::Result<()> {
    if descriptor.capabilities.len() > 256 {
        bail!("Plugin descriptor exceeds 256 provided Capabilities");
    }
    let mut seen = BTreeSet::new();
    for capability in &descriptor.capabilities {
        if !seen.insert(&capability.capability_id) {
            bail!(
                "Plugin descriptor repeats provided Capability `{}`",
                capability.capability_id
            );
        }
        if capability.request_operations.is_empty() {
            bail!(
                "Plugin Capability `{}` must declare at least one request operation",
                capability.capability_id
            );
        }
    }
    Ok(())
}

fn one_capability(descriptor: &PluginDescriptor) -> anyhow::Result<&PluginCapability> {
    let [capability] = descriptor.capabilities.as_slice() else {
        bail!("Plugin dev invocation requires exactly one provided Capability");
    };
    Ok(capability)
}

fn project_root(root: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    root.map_or_else(
        || env::current_dir().context("resolve Plugin project root"),
        Ok,
    )
}

fn warn_for_legacy_plugin_id(plugin_id: &str) -> anyhow::Result<()> {
    if classify_existing_plugin_id(plugin_id)? == PluginIdVersion::Legacy {
        eprintln!(
            "warning: `{plugin_id}` is a legacy unnamespaced Plugin id; migrate to a namespaced v1 id such as `company.{plugin_id}`"
        );
    }
    Ok(())
}

fn run_cargo(root: &Path, args: &[&str], action: &str) -> anyhow::Result<()> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| action.to_owned())?;
    if !status.success() {
        bail!("{action} failed with {status}");
    }
    Ok(())
}

fn run_bun(root: &Path, args: &[&str], action: &str) -> anyhow::Result<()> {
    let bun = env::var_os("BUN_BIN").unwrap_or_else(|| "bun".into());
    let status = Command::new(bun)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| action.to_owned())?;
    if !status.success() {
        bail!("{action} failed with {status}");
    }
    Ok(())
}

fn run_bun_output(root: &Path, args: &[&str], action: &str) -> anyhow::Result<Vec<u8>> {
    let bun = env::var_os("BUN_BIN").unwrap_or_else(|| "bun".into());
    let output = Command::new(bun)
        .args(args)
        .current_dir(root)
        .output()
        .with_context(|| action.to_owned())?;
    if !output.status.success() {
        bail!(
            "{action} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn print_verified(
    verified: &VerifiedBundle,
    output: Option<&Path>,
    json: bool,
) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.plugin-pack",
                "plugin_id": verified.plugin_id,
                "release_version": verified.release_version,
                "manifest_digest": verified.manifest_digest,
                "artifact_digests": verified.artifact_digests,
                "output": output.map(|path| path.display().to_string()),
            }))?
        );
    } else {
        println!("Packed {}@{}", verified.plugin_id, verified.release_version);
        if let Some(output) = output {
            println!("output {}", output.display());
        }
        println!("manifest {}", verified.manifest_digest);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
