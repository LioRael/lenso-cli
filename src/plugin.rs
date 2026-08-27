use std::{
    any::Any,
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use lenso_app_plan::{
    CapabilityEndpointPlan, ExecutionClassId, ModuleInstancePlan, ResolvedAppPlan,
};
use lenso_kernel::{CancellationToken, ExecutionAdapter, InvocationContext, RuntimeFailure};
use lenso_plugin_bundle::{
    ArtifactSource, BundleBuild, SourcePluginBuild, VerifiedBundle, build_bundle,
    build_source_plugin_bundle, extract_plugin_descriptor, verify_bundle_directory,
};
use lenso_runtime_codec::{ArtifactCatalog, ArtifactHandle, JsonCapabilityCodec};
use lenso_wasm_component_adapter::{EXECUTION_CLASS, WasmComponentAdapter};
use serde::Deserialize;
use serde_json::Value;

const GUEST_SDK_VERSION: &str = "0.1.3";
const WASM_TARGET: &str = "wasm32-unknown-unknown";

#[derive(Clone, Debug, Subcommand)]
pub enum PluginCommand {
    /// Create a Rust/Wasm Plugin project.
    New(PluginNewArgs),
    /// Build and run the Plugin through the Wasm Component Adapter.
    Dev(PluginDevArgs),
    /// Validate Plugin source and generated descriptor evidence.
    Check(PluginCheckArgs),
    /// Build and verify one immutable `.lenso-plugin` directory.
    Pack(PluginPackArgs),
    /// Deprecated compatibility alias for template-based V1 Bundle creation.
    #[command(hide = true)]
    Build(LegacyPluginBuildArgs),
    /// Deprecated compatibility alias for internal Bundle verification.
    #[command(hide = true)]
    Verify(LegacyPluginVerifyArgs),
}

#[derive(Args, Clone, Debug)]
pub struct PluginNewArgs {
    /// Plugin id, such as uppercase or issue-summary.
    plugin_id: String,
    /// Base directory for the new Plugin project.
    #[arg(long)]
    repo_root: Option<PathBuf>,
    /// New Plugin project directory. Defaults to the Plugin id.
    #[arg(long)]
    dir: Option<PathBuf>,
    /// Plugin runtime. The first public Plugin shape supports Rust/Wasm only.
    #[arg(long, value_enum, default_value_t = PluginRuntimeArg::Rust)]
    runtime: PluginRuntimeArg,
    /// Skip lockfile generation and the initial compile check.
    #[arg(long)]
    no_install: bool,
    /// Print generated paths without writing them.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum PluginRuntimeArg {
    Rust,
    Bun,
    QuickJs,
    Process,
    NativeDylib,
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
}

#[derive(Args, Clone, Debug)]
pub struct PluginPackArgs {
    /// Plugin project root. Defaults to the current directory.
    #[arg(long)]
    repo_root: Option<PathBuf>,
    /// Output `.lenso-plugin` directory. Defaults under `dist/`.
    #[arg(long)]
    output: Option<PathBuf>,
    /// Emit a stable JSON result.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Clone, Debug)]
pub struct LegacyPluginBuildArgs {
    /// Publisher Manifest template containing stable Plugin metadata.
    #[arg(long, default_value = "lenso-plugin.template.json")]
    manifest: PathBuf,
    /// New Bundle directory. Existing paths are never overwritten.
    #[arg(long)]
    output: PathBuf,
    /// Source override in `ARTIFACT_ID=PATH` form.
    #[arg(long = "artifact", value_name = "ARTIFACT_ID=PATH")]
    artifacts: Vec<String>,
    /// Emit the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Clone, Debug)]
pub struct LegacyPluginVerifyArgs {
    /// Materialized Bundle directory containing `lenso-plugin.json`.
    #[arg(long)]
    bundle: PathBuf,
    /// Emit the result as JSON.
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct LensoMetadata {
    plugin_id: String,
}

#[derive(Debug, Deserialize)]
struct PluginDescriptor {
    abi: String,
    capabilities: Vec<PluginCapability>,
}

#[derive(Debug, Deserialize)]
struct PluginCapability {
    capability_id: String,
    descriptor_version: String,
    request_operations: Vec<String>,
}

pub async fn plugin(command: PluginCommand) -> anyhow::Result<()> {
    match command {
        PluginCommand::New(args) => create(args),
        PluginCommand::Dev(args) => dev(args).await,
        PluginCommand::Check(args) => check(args),
        PluginCommand::Pack(args) => pack(args),
        PluginCommand::Build(args) => {
            eprintln!("warning: `lenso plugin build` is deprecated; use `lenso plugin pack`");
            legacy_build(args)
        }
        PluginCommand::Verify(args) => {
            eprintln!(
                "warning: `lenso plugin verify` is deprecated; `pack` and Harness `plugins add` already verify exact bytes"
            );
            let verified = verify_bundle_directory(&args.bundle).with_context(|| {
                format!("failed to verify Plugin Bundle `{}`", args.bundle.display())
            })?;
            print_legacy_result(&verified, args.json)
        }
    }
}

fn create(args: PluginNewArgs) -> anyhow::Result<()> {
    if args.runtime != PluginRuntimeArg::Rust {
        bail!(
            "Plugin runtime `{}` is not supported yet; the first public Plugin shape is Rust/Wasm (`--runtime rust`)",
            args.runtime
                .to_possible_value()
                .expect("ValueEnum variant")
                .get_name()
        );
    }
    validate_plugin_id(&args.plugin_id)?;
    let base = args.repo_root.unwrap_or(env::current_dir()?);
    let target = args
        .dir
        .map_or_else(|| base.join(&args.plugin_id), |dir| base.join(dir));
    if target.exists() {
        bail!(
            "Plugin project directory already exists: {}",
            target.display()
        );
    }
    let files = plugin_scaffold(&args.plugin_id);
    if args.dry_run {
        println!("Plugin dry run for {}:", target.display());
        for path in files.keys() {
            println!("  {}", path.display());
        }
        return Ok(());
    }
    fs::create_dir_all(&base)
        .with_context(|| format!("create Plugin project parent {}", base.display()))?;
    let staging = tempfile::Builder::new()
        .prefix(".lenso-plugin-new-")
        .tempdir_in(&base)
        .context("create Plugin scaffold staging directory")?;
    for (path, contents) in files {
        let destination = staging.path().join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, contents)?;
    }
    fs::rename(staging.path(), &target)
        .with_context(|| format!("publish Plugin scaffold {}", target.display()))?;
    if !args.no_install {
        run_cargo(&target, &["generate-lockfile"], "generate Plugin lockfile")?;
        run_cargo(
            &target,
            &["check", "--locked", "--target", WASM_TARGET],
            "check generated Plugin",
        )?;
    }
    println!("Created Plugin project at {}.", target.display());
    Ok(())
}

fn plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let package_name = plugin_id.replace('.', "-");
    let capability_id = format!("{plugin_id}.tool@1");
    BTreeMap::from([
        (
            PathBuf::from("Cargo.toml"),
            format!(
                r#"[package]
name = "{package_name}"
version = "0.1.0"
edition = "2024"
publish = false

[package.metadata.lenso]
plugin-id = "{plugin_id}"

[lib]
crate-type = ["cdylib"]

[dependencies]
lenso-guest-sdk = "{GUEST_SDK_VERSION}"
wit-bindgen = "0.60"

[workspace]
"#
            ),
        ),
        (
            PathBuf::from("src/lib.rs"),
            format!(
                r#"wit_bindgen::generate!({{
    path: "wit",
    world: "plugin",
}});

struct PluginComponent;

lenso_guest_sdk::guest_request_plugin! {{
impl Guest for PluginComponent {{
    provides: {{
        capability_id: "{capability_id}",
        descriptor_version: "1.0.0",
        requests: ["run"],
    }}

    fn invoke(
        capability: String,
        operation: String,
        request_json: String,
    ) -> Result<String, String> {{
        if capability != "{capability_id}" || operation != "run" {{
            return Err("\"unsupported request\"".to_owned());
        }}
        Ok(request_json)
    }}
}}
}}

export!(PluginComponent);
"#
            ),
        ),
        (
            PathBuf::from("wit/world.wit"),
            "package lenso:runtime@1.0.0;\n\nworld plugin {\n  export describe: func() -> string;\n  export invoke: func(capability: string, operation: string, request-json: string) -> result<string, string>;\n}\n".to_owned(),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {plugin_id}\n\nRust/Wasm Plugin providing `{capability_id}`. Edit `src/lib.rs`, then use one Plugin workflow:\n\n```sh\nlenso plugin dev\nlenso plugin check\nlenso plugin pack\n```\n\nCreate another project with `lenso plugin new <id>`.\n"
            ),
        ),
    ])
}

fn check(args: PluginCheckArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root)?;
    let temporary = tempfile::tempdir().context("create Plugin check directory")?;
    let output = temporary.path().join("checked.lenso-plugin");
    let verified = materialize(&root, &output)?;
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

async fn dev(args: PluginDevArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root)?;
    let temporary = tempfile::tempdir().context("create Plugin dev directory")?;
    let output = temporary.path().join("dev.lenso-plugin");
    let verified = materialize(&root, &output)?;
    let component_path = output.join("plugin.wasm");
    let component = fs::read(&component_path)?;
    let descriptor = parse_descriptor(&component)?;
    let capability = one_capability(&descriptor)?;
    let operation = args.operation.unwrap_or_else(|| {
        capability
            .request_operations
            .first()
            .expect("validated operation")
            .clone()
    });
    if !capability.request_operations.contains(&operation) {
        bail!(
            "Plugin Capability `{}` does not declare operation `{operation}`",
            capability.capability_id
        );
    }
    let request: Value = serde_json::from_str(&args.request_json)
        .context("Plugin development request is not valid JSON")?;
    let digest = verified
        .artifact_digests
        .first()
        .ok_or_else(|| anyhow!("Plugin Bundle contains no executable artifact"))?;
    let artifact = ArtifactHandle::open(&component_path, digest, component.len() as u64)
        .map_err(|error| runtime_error("open Plugin artifact", &error))?;
    let artifacts = ArtifactCatalog::new()
        .with_artifact("plugin", artifact)
        .map_err(|error| runtime_error("register Plugin artifact", &error))?;
    let codec = DynamicJsonCodec::new(capability);
    let adapter = WasmComponentAdapter::new(artifacts).with_codec(codec);
    let plan = ResolvedAppPlan::new(
        vec![
            ModuleInstancePlan::new("plugin", &verified.plugin_id)
                .with_entrypoint("plugin")
                .with_execution_class(ExecutionClassId::new(EXECUTION_CLASS))
                .with_capability(CapabilityEndpointPlan::new(
                    &capability.capability_id,
                    &capability.descriptor_version,
                    capability.request_operations.clone(),
                )),
        ],
        Vec::new(),
    );
    let generation = adapter
        .recreate(&plan, "plugin")
        .map_err(|error| runtime_error("prepare Plugin generation", &error))?;
    let endpoint = generation
        .endpoints()
        .first()
        .ok_or_else(|| anyhow!("Plugin produced no request endpoint"))?;
    let outcome = endpoint
        .invoke(
            &operation,
            Box::new(request),
            InvocationContext::new(1, None, CancellationToken::new()),
        )
        .await
        .map_err(|error| runtime_error("invoke Plugin", &error))?;
    let response = outcome
        .map_err(|error| {
            error.downcast::<Value>().map_or_else(
                |_| anyhow!("Plugin returned an unknown Domain Error"),
                |value| anyhow!("Plugin returned Domain Error: {value}"),
            )
        })?
        .downcast::<Value>()
        .map_err(|_| anyhow!("Plugin returned a response with an unexpected type"))?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.plugin-dev",
                "plugin_id": verified.plugin_id,
                "capability_id": capability.capability_id,
                "operation": operation,
                "response": *response,
            }))?
        );
    } else {
        println!("Plugin {} returned {}", verified.plugin_id, response);
    }
    Ok(())
}

fn pack(args: PluginPackArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root)?;
    let package = read_package(&root.join("Cargo.toml"))?;
    let output = args.output.unwrap_or_else(|| {
        root.join("dist").join(format!(
            "{}-{}.lenso-plugin",
            package.metadata.lenso.plugin_id, package.version
        ))
    });
    let verified = materialize(&root, &output)?;
    let reopened = verify_bundle_directory(&output)
        .with_context(|| format!("reopen packed Plugin `{}`", output.display()))?;
    if verified != reopened {
        bail!("packed Plugin verification result changed after publication");
    }
    print_verified(&reopened, Some(&output), args.json)
}

fn materialize(root: &Path, output: &Path) -> anyhow::Result<VerifiedBundle> {
    let manifest = root.join("Cargo.toml");
    let package = read_package(&manifest)?;
    let target_directory = cargo_target_directory(root)?;
    run_cargo(
        root,
        &["build", "--locked", "--release", "--target", WASM_TARGET],
        "build Plugin Wasm",
    )?;
    let artifact = target_directory
        .join(WASM_TARGET)
        .join("release")
        .join(format!("{}.wasm", package.name.replace('-', "_")));
    build_source_plugin_bundle(&SourcePluginBuild {
        package_manifest: manifest,
        wasm_module: artifact,
        output: output.to_path_buf(),
    })
    .with_context(|| format!("package Plugin `{}`", package.metadata.lenso.plugin_id))
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
    validate_plugin_id(&document.package.metadata.lenso.plugin_id)?;
    Ok(document.package)
}

fn parse_descriptor(component: &[u8]) -> anyhow::Result<PluginDescriptor> {
    let bytes = extract_plugin_descriptor(component)?;
    let descriptor: PluginDescriptor =
        serde_json::from_slice(&bytes).context("parse generated Plugin descriptor")?;
    if descriptor.abi != "lenso.json-request@1" {
        bail!(
            "unsupported Plugin descriptor ABI `{}`; expected request-only V1",
            descriptor.abi
        );
    }
    one_capability(&descriptor)?;
    Ok(descriptor)
}

fn one_capability(descriptor: &PluginDescriptor) -> anyhow::Result<&PluginCapability> {
    let [capability] = descriptor.capabilities.as_slice() else {
        bail!("the first public Plugin shape requires exactly one provided Capability");
    };
    if capability.request_operations.is_empty() {
        bail!("Plugin Capability must declare at least one request operation");
    }
    Ok(capability)
}

fn project_root(root: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    root.map_or_else(
        || env::current_dir().context("resolve Plugin project root"),
        Ok,
    )
}

fn validate_plugin_id(plugin_id: &str) -> anyhow::Result<()> {
    if plugin_id.is_empty()
        || !plugin_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
        || !plugin_id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
    {
        bail!(
            "Plugin id must start with a lowercase letter and contain only lowercase letters, digits, `.` or `-`"
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

fn runtime_error(action: &str, error: &RuntimeFailure) -> anyhow::Error {
    anyhow!("{action}: {error:?}")
}

fn legacy_build(args: LegacyPluginBuildArgs) -> anyhow::Result<()> {
    let artifact_sources = args
        .artifacts
        .iter()
        .map(|value| parse_artifact_source(value))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let verified = build_bundle(&BundleBuild {
        template: args.manifest,
        output: args.output.clone(),
        artifact_sources,
    })
    .with_context(|| format!("failed to build Plugin Bundle `{}`", args.output.display()))?;
    print_legacy_result(&verified, args.json)
}

fn parse_artifact_source(value: &str) -> anyhow::Result<ArtifactSource> {
    let Some((artifact_id, path)) = value.split_once('=') else {
        bail!("Artifact source `{value}` must use ARTIFACT_ID=PATH");
    };
    if artifact_id.is_empty() || path.is_empty() {
        bail!("Artifact source `{value}` must include a non-empty ID and path");
    }
    Ok(ArtifactSource {
        artifact_id: artifact_id.to_owned(),
        path: PathBuf::from(path),
    })
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

fn print_legacy_result(verified: &VerifiedBundle, json: bool) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&legacy_result_json(verified))?
        );
    } else {
        println!(
            "verified {}@{}",
            verified.plugin_id, verified.release_version
        );
        println!("manifest {}", verified.manifest_digest);
        for digest in &verified.artifact_digests {
            println!("artifact {digest}");
        }
        for digest in &verified.product_metadata_digests {
            println!("product-metadata {digest}");
        }
    }
    Ok(())
}

fn legacy_result_json(verified: &VerifiedBundle) -> Value {
    serde_json::json!({
        "plugin_id": verified.plugin_id,
        "release_version": verified.release_version,
        "manifest_digest": verified.manifest_digest,
        "artifact_digests": verified.artifact_digests,
        "product_metadata_digests": verified.product_metadata_digests,
    })
}

#[derive(Debug)]
struct DynamicJsonCodec {
    capability_id: &'static str,
    descriptor_version: &'static str,
    request_operations: &'static [&'static str],
}

impl DynamicJsonCodec {
    fn new(capability: &PluginCapability) -> Self {
        let operations = capability
            .request_operations
            .iter()
            .map(|operation| Box::leak(operation.clone().into_boxed_str()) as &'static str)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            capability_id: Box::leak(capability.capability_id.clone().into_boxed_str()),
            descriptor_version: Box::leak(capability.descriptor_version.clone().into_boxed_str()),
            request_operations: Box::leak(operations),
        }
    }
}

impl JsonCapabilityCodec for DynamicJsonCodec {
    fn capability_id(&self) -> &'static str {
        self.capability_id
    }

    fn descriptor_version(&self) -> &'static str {
        self.descriptor_version
    }

    fn request_operations(&self) -> &'static [&'static str] {
        self.request_operations
    }

    fn encode_request(&self, _: &str, request: &dyn Any) -> Result<Value, RuntimeFailure> {
        request
            .downcast_ref::<Value>()
            .cloned()
            .ok_or(RuntimeFailure::ProtocolViolation {
                capability: self.capability_id,
            })
    }

    fn decode_response(&self, _: &str, value: Value) -> Result<Box<dyn Any>, RuntimeFailure> {
        Ok(Box::new(value))
    }

    fn decode_domain_error(&self, _: &str, value: Value) -> Result<Box<dyn Any>, RuntimeFailure> {
        Ok(Box::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_plugin_scaffold_has_one_plugin_authority_and_four_commands() {
        let files = plugin_scaffold("uppercase");
        let all = files.values().cloned().collect::<String>();

        assert!(all.contains("guest_request_plugin!"));
        assert!(all.contains("plugin-id = \"uppercase\""));
        assert!(all.contains("lenso plugin new"));
        assert!(all.contains("lenso plugin dev"));
        assert!(all.contains("lenso plugin check"));
        assert!(all.contains("lenso plugin pack"));
        for forbidden in [
            "#[module]",
            "defineModule",
            "MODULE.md",
            "module_contributions",
            "template.json",
            "fn describe",
        ] {
            assert!(!all.contains(forbidden), "unexpected `{forbidden}`");
        }
    }

    #[test]
    fn unsupported_plugin_runtime_fails_with_the_supported_shape() {
        let root = tempfile::tempdir().unwrap();
        let error = create(PluginNewArgs {
            plugin_id: "uppercase".to_owned(),
            repo_root: Some(root.path().to_path_buf()),
            dir: None,
            runtime: PluginRuntimeArg::Bun,
            no_install: true,
            dry_run: false,
        })
        .unwrap_err();

        assert!(error.to_string().contains("Rust/Wasm"));
    }

    #[test]
    fn artifact_source_requires_an_id_and_path() {
        assert_eq!(
            parse_artifact_source("guest=target/guest.wasm").unwrap(),
            ArtifactSource {
                artifact_id: "guest".to_owned(),
                path: PathBuf::from("target/guest.wasm"),
            }
        );
        assert!(parse_artifact_source("guest").is_err());
        assert!(parse_artifact_source("=guest.wasm").is_err());
        assert!(parse_artifact_source("guest=").is_err());
    }

    #[test]
    fn duplicate_plugin_identity_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join("Cargo.toml");
        fs::write(
            &manifest,
            r#"[package]
name = "duplicate"
version = "0.1.0"
[package.metadata.lenso]
plugin-id = "first"
plugin-id = "second"
"#,
        )
        .unwrap();

        assert!(read_package(&manifest).is_err());
    }

    #[test]
    fn malformed_plugin_package_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("lenso-plugin.json"), b"{}\n").unwrap();

        assert!(verify_bundle_directory(root.path()).is_err());
    }

    #[test]
    fn legacy_json_result_keeps_the_v1_field_set() {
        let verified = VerifiedBundle {
            plugin_id: "example.plugin".to_owned(),
            release_version: "1.0.0".to_owned(),
            manifest_digest: "sha256:manifest".to_owned(),
            artifact_digests: vec!["sha256:artifact".to_owned()],
            product_metadata_digests: vec!["sha256:metadata".to_owned()],
        };
        let value = legacy_result_json(&verified);
        let keys = value
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(
            keys,
            [
                "plugin_id",
                "release_version",
                "manifest_digest",
                "artifact_digests",
                "product_metadata_digests",
            ]
        );
    }

    #[tokio::test]
    #[ignore = "clean-room test downloads released crates and compiles wasm32"]
    async fn clean_room_plugin_runs_new_check_dev_and_pack() {
        let root = tempfile::tempdir().unwrap();
        create(PluginNewArgs {
            plugin_id: "uppercase".to_owned(),
            repo_root: Some(root.path().to_path_buf()),
            dir: None,
            runtime: PluginRuntimeArg::Rust,
            no_install: false,
            dry_run: false,
        })
        .unwrap();
        let project = root.path().join("uppercase");
        check(PluginCheckArgs {
            repo_root: Some(project.clone()),
            json: true,
        })
        .unwrap();
        dev(PluginDevArgs {
            repo_root: Some(project.clone()),
            operation: None,
            request_json: r#"{"text":"hello"}"#.to_owned(),
            json: true,
        })
        .await
        .unwrap();
        let output = project.join("dist/uppercase.lenso-plugin");
        pack(PluginPackArgs {
            repo_root: Some(project.clone()),
            output: Some(output.clone()),
            json: true,
        })
        .unwrap();
        verify_bundle_directory(&output).unwrap();
        fs::write(output.join("plugin.wasm"), b"changed after pack").unwrap();
        assert!(verify_bundle_directory(&output).is_err());
        assert!(
            pack(PluginPackArgs {
                repo_root: Some(project),
                output: Some(output),
                json: false,
            })
            .is_err()
        );
    }
}
