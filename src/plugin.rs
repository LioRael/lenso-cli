use std::{
    any::Any,
    collections::BTreeMap,
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime},
};

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use lenso_app_plan::{
    CapabilityEndpointPlan, ExecutionClassId, PluginInstancePlan, ResolvedAppPlan,
    authoring::PluginContract,
};
use lenso_kernel::{CancellationToken, ExecutionAdapter, InvocationContext, RuntimeFailure};
use lenso_plugin_bundle::{
    ImplementationPolicy, PluginManifest, SourcePluginBuild, SourcePluginImplementation,
    SourcePluginReleaseBuild, SourceProcessPluginBuild, VerifiedBundle, build_source_plugin_bundle,
    build_source_plugin_release_bundle, build_source_process_plugin_bundle,
    extract_plugin_descriptor, read_bundle_manifest, resolve_implementation,
    verify_bundle_directory,
};
use lenso_runtime_codec::{ArtifactCatalog, ArtifactHandle, JsonCapabilityCodec};
use lenso_wasm_component_adapter::{
    EXECUTION_CLASS as WASM_EXECUTION_CLASS, WasmComponentAdapter, WasmComponentLimits,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::archive::{archive_bundle, with_bundle_directory};

const PLUGIN_SDK_REVISION: &str = "7c54f4065012d41769fefbb41098a4657f1f4825";
const AGENT_TOOL_SDK_REVISION: &str = "fd944a4ee56026be708b50c710635d1b17a59758";
const WASM_TARGET: &str = "wasm32-unknown-unknown";

#[derive(Clone, Debug, Subcommand)]
pub enum PluginCommand {
    /// Create one ordinary Rust Plugin with portable Wasm and Process outputs.
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
    /// Plugin id, such as uppercase or issue-summary.
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
    capabilities: Vec<PluginCapability>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    }
}

fn create(args: PluginNewArgs) -> anyhow::Result<()> {
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
    let files = match args.runtime {
        PluginRuntimeArg::Multi => multi_plugin_scaffold(&args.plugin_id),
        PluginRuntimeArg::Wasm => plugin_scaffold(&args.plugin_id),
        PluginRuntimeArg::Process => process_plugin_scaffold(&args.plugin_id),
        PluginRuntimeArg::Bun => bun_plugin_scaffold(&args.plugin_id),
    };
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
        if args.runtime == PluginRuntimeArg::Bun {
            run_bun(&target, &["install"], "install Bun Plugin dependencies")?;
            run_bun(&target, &["run", "check"], "check generated Bun Plugin")?;
        } else {
            run_cargo(&target, &["generate-lockfile"], "generate Plugin lockfile")?;
        }
        if matches!(
            args.runtime,
            PluginRuntimeArg::Multi | PluginRuntimeArg::Wasm
        ) {
            run_cargo(
                &target,
                &["check", "--locked", "--lib", "--target", WASM_TARGET],
                "check generated Wasm implementation",
            )?;
        }
        if matches!(
            args.runtime,
            PluginRuntimeArg::Multi | PluginRuntimeArg::Process
        ) {
            run_cargo(
                &target,
                &[
                    "check",
                    "--locked",
                    "--bin",
                    &args.plugin_id.replace('.', "-"),
                ],
                "check generated Process implementation",
            )?;
        }
    }
    println!("Created Plugin project at {}.", target.display());
    Ok(())
}

fn bun_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let package_name = plugin_id.replace('.', "-");
    BTreeMap::from([
        (
            PathBuf::from("package.json"),
            format!(
                r#"{{
  "name": "{package_name}",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {{
    "check": "tsc --noEmit"
  }},
  "dependencies": {{
    "@lenso/bun": "0.2.0"
  }},
  "devDependencies": {{
    "@types/bun": "1.4.0",
    "typescript": "7.0.2"
  }},
  "lenso": {{
    "pluginId": "{plugin_id}",
    "rootSlot": "tool-providers",
    "runtime": "bun"
  }}
}}
"#
            ),
        ),
        (
            PathBuf::from("tsconfig.json"),
            r#"{
  "compilerOptions": {
    "lib": ["ES2023"],
    "module": "Preserve",
    "moduleResolution": "bundler",
    "noEmit": true,
    "strict": true,
    "allowImportingTsExtensions": true,
    "types": ["bun"]
  },
  "include": ["src/**/*.ts"]
}
"#
            .to_owned(),
        ),
        (
            PathBuf::from("src/plugin.ts"),
            format!(
                r#"import {{ definePlugin }} from "@lenso/bun";
import {{
  bindToolProviderProvider,
  type ToolProviderProvider,
}} from "@lenso/bun/capabilities/agent-tool-provider";

const tool: ToolProviderProvider = {{
  async catalog() {{
    return {{
      ok: true,
      value: {{
        tools: [{{
          description: "Process one UTF-8 string.",
          input_schema_json: JSON.stringify({{
            type: "object",
            additionalProperties: false,
            properties: {{ text: {{ type: "string", maxLength: 4096 }} }},
            required: ["text"],
          }}),
          name: "{plugin_id}",
        }}],
      }},
    }};
  }},
  async execute(_context, request) {{
    if (request.name !== "{plugin_id}") {{
      return {{ ok: false, error: {{ kind: "domain", error: "not_found" }} }};
    }}
    let arguments_: unknown;
    try {{
      arguments_ = JSON.parse(request.arguments_json);
    }} catch {{
      return {{ ok: false, error: {{ kind: "domain", error: "invalid_arguments" }} }};
    }}
    if (
      typeof arguments_ !== "object" ||
      arguments_ === null ||
      !("text" in arguments_) ||
      typeof arguments_.text !== "string" ||
      arguments_.text.length === 0 ||
      arguments_.text.length > 4096
    ) {{
      return {{ ok: false, error: {{ kind: "domain", error: "invalid_arguments" }} }};
    }}
    return {{
      ok: true,
      value: {{ content: arguments_.text, content_type: "text", metadata_json: "{{}}" }},
    }};
  }},
}};

export default definePlugin({{ providers: [bindToolProviderProvider(tool)] }});
"#
            ),
        ),
        (
            PathBuf::from("src/lenso.bun.generated.ts"),
            "import { serve } from \"@lenso/bun\";\nimport plugin from \"./plugin.ts\";\n\nserve(plugin);\n"
                .to_owned(),
        ),
        (
            PathBuf::from("src/lenso.describe.generated.ts"),
            "import plugin from \"./plugin.ts\";\n\nconsole.log(JSON.stringify({\n  abi: \"lenso.json-request@1\",\n  capabilities: plugin.providers.map(({ descriptor }) => ({\n    capability_id: descriptor.capability_id,\n    descriptor_version: descriptor.descriptor_version,\n    request_operations: [...descriptor.operations],\n  })),\n}));\n"
                .to_owned(),
        ),
        (
            PathBuf::from("src/lenso.invoke.generated.ts"),
            "import plugin from \"./plugin.ts\";\n\nconst [, , capability, operation, request = \"{}\"] = Bun.argv;\nconst provider = plugin.providers.find(({ descriptor }) => descriptor.capability_id === capability);\nif (!provider) throw new Error(`unknown capability ${capability}`);\nconst outcome = await provider.invokeRequest(operation, {\n  requestId: \"0\" as never,\n  cancelled: false,\n  extensions: Object.freeze({}),\n}, JSON.parse(request));\nswitch (outcome.kind) {\n  case \"success\": console.log(JSON.stringify({ ok: outcome.value })); break;\n  case \"domain\": console.log(JSON.stringify({ error: outcome.value })); break;\n  case \"runtime\": throw new Error(`Plugin runtime failure: ${outcome.failure.kind}`);\n}\n"
                .to_owned(),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {plugin_id}\n\nTyped Bun Plugin using the official `@lenso/bun` Capability projection. The generated files own runtime lowering; edit `src/plugin.ts`.\n\n```sh\nlenso plugin check\nlenso plugin dev --operation execute --request-json '{{\"name\":\"{plugin_id}\",\"arguments_json\":\"{{\\\"text\\\":\\\"hello\\\"}}\"}}'\nlenso plugin dev --watch\nlenso plugin pack\n```\n"
            ),
        ),
    ])
}

fn multi_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let mut files = plugin_scaffold(plugin_id);
    let manifest = files
        .get_mut(Path::new("Cargo.toml"))
        .expect("Wasm scaffold has a manifest");
    *manifest = manifest.replace("runtime = \"wasm\"", "outputs = [\"wasm\", \"process\"]");
    files.insert(
        PathBuf::from("src/main.rs"),
        "// Cargo Process entrypoint; the SDK supplies main and protocol lowering.\ninclude!(\"lib.rs\");\n"
            .to_owned(),
    );
    files.insert(
        PathBuf::from("README.md"),
        format!(
            "# {plugin_id}\n\nOne ordinary Rust Plugin source with portable Wasm and trusted Process outputs. `lenso plugin pack` builds both implementations into one release.\n"
        ),
    );
    files
}

fn plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let package_name = plugin_id.replace('.', "-");
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
root-slot = "tool-providers"

[package.metadata.lenso-cli]
runtime = "wasm"

[lib]
crate-type = ["cdylib"]

[dependencies]
lenso = {{ package = "lenso-plugin-sdk", version = "0.2.0", git = "https://github.com/LioRael/lenso-runtime-rust", rev = "{PLUGIN_SDK_REVISION}" }}
lenso-agent-tool-sdk = {{ version = "0.2.0", git = "https://github.com/LioRael/lenso-agent-harness", rev = "{AGENT_TOOL_SDK_REVISION}" }}
schemars = "1"
serde = {{ version = "1", features = ["derive"] }}

[workspace]
"#
            ),
        ),
        (
            PathBuf::from("src/lib.rs"),
            format!(
                r#"use lenso_agent_tool_sdk::prelude::*;
use schemars::JsonSchema;

#[derive(Debug, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {{
    #[schemars(length(max = 4096))]
    text: String,
}}

#[lenso::plugin]
#[derive(Clone, Copy, Debug, Default)]
struct Plugin {{}}

#[lenso_agent_tool_sdk::tool_provider]
impl Plugin {{
    #[tool(
        name = "{plugin_id}",
        description = "Process one UTF-8 string.",
        execution = "parallel_safe"
    )]
    fn execute(arguments: Arguments) -> Result<ExecuteResponse, ExecuteError> {{
        if arguments.text.is_empty() {{
            return Err(ExecuteError::InvalidArguments);
        }}
        Ok(ExecuteResponse {{
            content: arguments.text,
            content_type: ContentType::Text,
            metadata_json: "{{}}"
                .try_into()
                .expect("static Tool metadata must be valid JSON"),
        }})
    }}
}}
"#
            ),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {plugin_id}\n\nOrdinary Rust Plugin for the Lenso Agent Harness, packaged as an isolated Wasm Component. The SDK owns the execution bridge.\n\n```sh\nlenso plugin check\nlenso plugin dev --operation execute --request-json '{{\"name\":\"{plugin_id}\",\"arguments_json\":\"{{\\\"text\\\":\\\"hello\\\"}}\"}}'\nlenso plugin pack\n```\n\nCreate another project with `lenso plugin new <id>`.\n"
            ),
        ),
    ])
}

fn process_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let mut files = plugin_scaffold(plugin_id);
    let manifest = files
        .get_mut(Path::new("Cargo.toml"))
        .expect("Plugin scaffold has a manifest");
    *manifest = manifest.replace("runtime = \"wasm\"", "runtime = \"process\"");
    files.insert(
        PathBuf::from("src/main.rs"),
        "// Cargo Process entrypoint; the SDK supplies main and protocol lowering.\ninclude!(\"lib.rs\");\n"
            .to_owned(),
    );
    files.insert(
        PathBuf::from("README.md"),
        format!(
            "# {plugin_id}\n\nOrdinary Rust source compiled as a trusted native Process Plugin. The SDK owns the protocol bridge and runtime descriptor. Process Plugins are not sandboxed, so install only trusted bundles.\n\n```sh\nlenso plugin check\nlenso plugin dev --operation execute --request-json '{{\"name\":\"{plugin_id}\",\"arguments_json\":\"{{\\\"text\\\":\\\"hello\\\"}}\"}}'\nlenso plugin pack\n```\n"
        ),
    );
    files
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

async fn dev(args: PluginDevArgs) -> anyhow::Result<()> {
    if !args.watch {
        return dev_once(&args).await;
    }
    let root = project_root(args.repo_root.clone())?;
    let mut fingerprint = source_fingerprint(&root)?;
    dev_once(&args).await?;
    if !args.json {
        println!(
            "Watching {} for Plugin changes. Press Ctrl-C to stop.",
            root.display()
        );
    }
    loop {
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result.context("listen for Ctrl-C")?;
                return Ok(());
            }
            () = tokio::time::sleep(Duration::from_millis(250)) => {
                let next = source_fingerprint(&root)?;
                if next != fingerprint {
                    fingerprint = next;
                    if let Err(error) = dev_once(&args).await {
                        eprintln!("Plugin rebuild failed: {error:#}");
                    }
                }
            }
        }
    }
}

async fn dev_once(args: &PluginDevArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root.clone())?;
    if let Some(package) = read_bun_package(&root)? {
        return dev_bun(&root, &package, args);
    }
    let package = read_package(&root.join("Cargo.toml"))?;
    let runtime = project_runtime(&package)?;
    let temporary = tempfile::tempdir().context("create Plugin dev directory")?;
    let output = temporary.path().join("dev.lenso-plugin");
    let verified = materialize(&root, &output, BuildProfile::Development)?;
    let dev_class = if runtime == ProjectRuntime::Process {
        "lenso.process@1"
    } else {
        WASM_EXECUTION_CLASS
    };
    let selected = resolve_implementation(
        &read_bundle_manifest(&output)?,
        &ImplementationPolicy {
            host_target: format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS),
            execution_classes: vec![ExecutionClassId::new(dev_class)],
        },
    )?;
    let artifact_path = output.join(&selected.artifact.path);
    let artifact_bytes = fs::read(&artifact_path)?;
    let descriptor = match runtime {
        ProjectRuntime::Wasm | ProjectRuntime::Multi => parse_descriptor(&artifact_bytes)?,
        ProjectRuntime::Process => read_process_descriptor(&artifact_path)?,
        ProjectRuntime::Bun => unreachable!("Bun development dispatches before Cargo metadata"),
    };
    let capability = one_capability(&descriptor)?;
    let operation = args.operation.clone().unwrap_or_else(|| {
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
    if runtime == ProjectRuntime::Process {
        let response = invoke_dev_process(
            &artifact_path,
            &descriptor,
            capability,
            &operation,
            &request,
        )?;
        return print_dev_response(&verified, capability, &operation, &response, args.json);
    }
    let artifact = ArtifactHandle::open(
        &artifact_path,
        &selected.artifact.digest,
        artifact_bytes.len() as u64,
    )
    .map_err(|error| runtime_error("open Plugin artifact", &error))?;
    let artifacts = ArtifactCatalog::new()
        .with_artifact("plugin", artifact)
        .map_err(|error| runtime_error("register Plugin artifact", &error))?;
    let plan = ResolvedAppPlan::new(
        vec![
            PluginInstancePlan::new("plugin", &verified.plugin_id)
                .with_entrypoint("plugin")
                .with_execution_class(ExecutionClassId::new(WASM_EXECUTION_CLASS))
                .with_capability(CapabilityEndpointPlan::new(
                    &capability.capability_id,
                    &capability.descriptor_version,
                    capability.request_operations.clone(),
                )),
        ],
        Vec::new(),
    );
    let adapter = WasmComponentAdapter::new(artifacts)
        .with_limits(WasmComponentLimits {
            max_component_bytes: artifact_bytes.len(),
            ..WasmComponentLimits::default()
        })
        .with_codec(DynamicJsonCodec::new(capability));
    let response = invoke_dev_adapter(&adapter, &plan, &operation, request).await?;
    print_dev_response(&verified, capability, &operation, &response, args.json)
}

fn invoke_dev_process(
    executable: &Path,
    expected_descriptor: &PluginDescriptor,
    capability: &PluginCapability,
    operation: &str,
    request: &Value,
) -> anyhow::Result<Value> {
    const MAX_FRAME_BYTES: usize = 1024 * 1024;
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("start Process Plugin `{}`", executable.display()))?;
    let mut input = child.stdin.take().context("open Process Plugin stdin")?;
    let output = child.stdout.take().context("open Process Plugin stdout")?;
    let mut output = BufReader::new(output);

    let result = (|| {
        let ready = read_process_frame(&mut output, MAX_FRAME_BYTES)?;
        if ready.get("type").and_then(Value::as_str) != Some("ready")
            || ready.get("protocol").and_then(Value::as_str) != Some("lenso.process-stdio@1")
        {
            bail!("Process Plugin did not complete the expected readiness handshake");
        }
        let actual_descriptor = serde_json::from_value::<PluginDescriptor>(
            ready
                .get("descriptor")
                .cloned()
                .context("Process Plugin readiness omitted its descriptor")?,
        )
        .context("Process Plugin readiness descriptor is invalid")?;
        if &actual_descriptor != expected_descriptor {
            bail!("Process Plugin readiness descriptor differs from packaged source evidence");
        }

        write_process_frame(
            &mut input,
            &serde_json::json!({
                "type": "invoke",
                "id": 1,
                "capability": capability.capability_id,
                "operation": operation,
                "request": request,
            }),
        )?;
        let response = read_process_frame(&mut output, MAX_FRAME_BYTES)?;
        if response.get("type").and_then(Value::as_str) != Some("result")
            || response.get("id").and_then(Value::as_u64) != Some(1)
        {
            bail!("Process Plugin returned an unexpected result frame");
        }
        if let Some(value) = response.get("ok") {
            return Ok(value.clone());
        }
        if let Some(error) = response.get("error") {
            bail!("Plugin returned Domain Error: {error}");
        }
        if let Some(failure) = response.get("failure").and_then(Value::as_str) {
            bail!("Process Plugin failed: {failure}");
        }
        bail!("Process Plugin result contains no terminal outcome")
    })();

    let _ = write_process_frame(&mut input, &serde_json::json!({ "type": "shutdown" }));
    drop(input);
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

fn read_process_descriptor(executable: &Path) -> anyhow::Result<PluginDescriptor> {
    const MAX_FRAME_BYTES: usize = 1024 * 1024;
    let mut child = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("start Process Plugin `{}`", executable.display()))?;
    let mut input = child.stdin.take().context("open Process Plugin stdin")?;
    let output = child.stdout.take().context("open Process Plugin stdout")?;
    let mut output = BufReader::new(output);

    let result = (|| {
        let ready = read_process_frame(&mut output, MAX_FRAME_BYTES)?;
        if ready.get("type").and_then(Value::as_str) != Some("ready")
            || ready.get("protocol").and_then(Value::as_str) != Some("lenso.process-stdio@1")
        {
            bail!("Process Plugin did not complete the expected readiness handshake");
        }
        let descriptor = serde_json::from_value(
            ready
                .get("descriptor")
                .cloned()
                .context("Process Plugin readiness omitted its descriptor")?,
        )
        .context("Process Plugin readiness descriptor is invalid")?;
        Ok(descriptor)
    })();

    let _ = write_process_frame(&mut input, &serde_json::json!({ "type": "shutdown" }));
    drop(input);
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

fn read_process_frame(reader: &mut impl BufRead, limit: usize) -> anyhow::Result<Value> {
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        bail!("Process Plugin closed stdout before returning a frame");
    }
    if line.len() > limit {
        bail!("Process Plugin frame exceeds the development limit");
    }
    serde_json::from_str(&line).context("Process Plugin returned invalid framed JSON")
}

fn write_process_frame(writer: &mut impl Write, frame: &Value) -> anyhow::Result<()> {
    serde_json::to_writer(&mut *writer, frame)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

async fn invoke_dev_adapter(
    adapter: &impl ExecutionAdapter,
    plan: &ResolvedAppPlan,
    operation: &str,
    request: Value,
) -> anyhow::Result<Value> {
    let generation = adapter
        .recreate(plan, "plugin")
        .map_err(|error| runtime_error("prepare Plugin generation", &error))?;
    let endpoint = generation
        .endpoints()
        .first()
        .ok_or_else(|| anyhow!("Plugin produced no request endpoint"))?;
    let outcome = endpoint
        .invoke(
            operation,
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
    Ok(*response)
}

fn print_dev_response(
    verified: &VerifiedBundle,
    capability: &PluginCapability,
    operation: &str,
    response: &Value,
    json: bool,
) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.plugin-dev",
                "plugin_id": verified.plugin_id,
                "capability_id": capability.capability_id,
                "operation": operation,
                "response": response,
            }))?
        );
    } else {
        println!("Plugin {} returned {}", verified.plugin_id, response);
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
    let reopened = with_bundle_directory(&output, |directory| {
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
    validate_plugin_id(&metadata.plugin_id)?;
    if metadata.root_slot.trim().is_empty() {
        bail!("Bun Plugin rootSlot must not be empty");
    }
    if document.version.trim().is_empty() {
        bail!("Bun Plugin version must not be empty");
    }
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
) -> anyhow::Result<VerifiedBundle> {
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
    let contract = descriptor.capabilities.iter().fold(
        PluginContract::new(
            &package.metadata.plugin_id,
            &package.version,
            &package.metadata.root_slot,
        ),
        |contract, capability| {
            contract.with_capability(CapabilityEndpointPlan::new(
                &capability.capability_id,
                &capability.descriptor_version,
                capability.request_operations.clone(),
            ))
        },
    );
    Ok(build_source_plugin_release_bundle(
        &SourcePluginReleaseBuild {
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
            }],
            output: output.to_path_buf(),
        },
    )?)
}

fn describe_bun_plugin(root: &Path) -> anyhow::Result<PluginDescriptor> {
    let output = run_bun_output(
        root,
        &["run", "src/lenso.describe.generated.ts"],
        "describe Bun Plugin",
    )?;
    let descriptor = parse_descriptor_bytes(&output)?;
    one_capability(&descriptor)?;
    Ok(descriptor)
}

fn dev_bun(root: &Path, package: &BunPackage, args: &PluginDevArgs) -> anyhow::Result<()> {
    let temporary = tempfile::tempdir().context("create Bun Plugin dev directory")?;
    let verified = materialize_bun(
        root,
        &temporary.path().join("dev.lenso-plugin"),
        package,
        BuildProfile::Development,
    )?;
    let descriptor = describe_bun_plugin(root)?;
    let capability = one_capability(&descriptor)?;
    let operation = args.operation.clone().unwrap_or_else(|| {
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
    let request_json = serde_json::to_string(&request)?;
    let output = run_bun_output(
        root,
        &[
            "run",
            "src/lenso.invoke.generated.ts",
            "--",
            &capability.capability_id,
            &operation,
            &request_json,
        ],
        "invoke Bun Plugin",
    )?;
    let outcome: Value =
        serde_json::from_slice(&output).context("Bun Plugin returned invalid JSON")?;
    if let Some(response) = outcome.get("ok") {
        return print_dev_response(&verified, capability, &operation, response, args.json);
    }
    if let Some(error) = outcome.get("error") {
        bail!("Plugin returned Domain Error: {error}");
    }
    bail!("Bun Plugin returned no terminal outcome")
}

fn materialize(
    root: &Path,
    output: &Path,
    profile: BuildProfile,
) -> anyhow::Result<VerifiedBundle> {
    if let Some(package) = read_bun_package(root)? {
        return materialize_bun(root, output, &package, profile);
    }
    let manifest = root.join("Cargo.toml");
    let package = read_package(&manifest)?;
    synchronize_plugin_lock(root, &package)?;
    let target_directory = cargo_target_directory(root)?;
    match project_runtime(&package)? {
        ProjectRuntime::Multi => {
            materialize_multi(root, output, &package, &target_directory, profile)
        }
        ProjectRuntime::Wasm => {
            let arguments = profile.cargo_args(["--target", WASM_TARGET]);
            run_cargo(root, &arguments, "build Plugin Wasm")?;
            let artifact = target_directory
                .join(WASM_TARGET)
                .join(profile.directory())
                .join(format!("{}.wasm", package.name.replace('-', "_")));
            Ok(build_source_plugin_bundle(&SourcePluginBuild {
                package_manifest: manifest,
                wasm_module: artifact,
                output: output.to_path_buf(),
            })?)
        }
        ProjectRuntime::Process => {
            let arguments = profile.cargo_args(["--bin", package.name.as_str()]);
            run_cargo(root, &arguments, "build Process Plugin")?;
            let executable = target_directory
                .join(profile.directory())
                .join(&package.name);
            let descriptor = tempfile::NamedTempFile::new().context("stage Process descriptor")?;
            serde_json::to_writer(descriptor.as_file(), &read_process_descriptor(&executable)?)?;
            Ok(build_source_process_plugin_bundle(
                &SourceProcessPluginBuild {
                    package_manifest: manifest,
                    executable,
                    runtime_descriptor: descriptor.path().to_path_buf(),
                    target: format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS),
                    output: output.to_path_buf(),
                },
            )?)
        }
        ProjectRuntime::Bun => unreachable!("Bun projects do not use Cargo metadata"),
    }
    .with_context(|| format!("package Plugin `{}`", package.metadata.lenso.plugin_id))
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
        serde_json::to_vec(&read_process_descriptor(&executable)?)?,
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
    validate_plugin_id(&document.package.metadata.lenso.plugin_id)?;
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

fn source_fingerprint(root: &Path) -> anyhow::Result<Vec<(PathBuf, u64, SystemTime)>> {
    let mut pending = vec![root.join("src")];
    let mut files = [
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
        root.join("package.json"),
        root.join("bun.lock"),
        root.join("tsconfig.json"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect::<Vec<_>>();
    while let Some(directory) = pending.pop() {
        if !directory.exists() {
            continue;
        }
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let metadata = fs::metadata(&path)
                .with_context(|| format!("inspect watched Plugin source {}", path.display()))?;
            Ok((path, metadata.len(), metadata.modified()?))
        })
        .collect()
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

fn runtime_error(action: &str, error: &RuntimeFailure) -> anyhow::Error {
    anyhow!("{action}: {error:?}")
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
    fn rust_plugin_scaffold_exposes_only_portable_authoring() {
        let files = plugin_scaffold("uppercase");
        let author_source = files.get(Path::new("src/lib.rs")).unwrap();
        let all = files.values().cloned().collect::<String>();

        assert!(author_source.contains("#[lenso::plugin]"));
        assert!(author_source.contains("#[lenso_agent_tool_sdk::tool_provider]"));
        assert!(author_source.contains("#[tool("));
        assert!(author_source.contains("fn execute(arguments: Arguments)"));
        assert!(all.contains("plugin-id = \"uppercase\""));
        assert!(all.contains("root-slot = \"tool-providers\""));
        assert!(all.contains("lenso plugin new"));
        assert!(all.contains("lenso plugin dev"));
        assert!(all.contains("lenso plugin check"));
        assert!(all.contains("lenso plugin pack"));
        for internal in [
            "wit_bindgen",
            "guest_request_plugin",
            "ProcessPlugin",
            "ProcessOutcome",
            "request_json",
            "arguments_json",
            "lenso.agent.tool-provider",
            "lenso.generated",
        ] {
            assert!(
                !author_source.contains(internal),
                "author source leaked `{internal}`"
            );
        }
        for removed in [
            "src/plugin.rs",
            "src/lenso.generated.rs",
            "src/lenso.wasm.generated.rs",
            "src/lenso.process.generated.rs",
            "lenso.generated.descriptor.json",
            "wit/world.wit",
        ] {
            assert!(
                !files.contains_key(Path::new(removed)),
                "unexpected `{removed}`"
            );
        }
    }

    #[test]
    fn bun_plugin_scaffold_uses_generated_runtime_lowering() {
        let files = bun_plugin_scaffold("example.echo");
        let package = files.get(Path::new("package.json")).unwrap();
        let author = files.get(Path::new("src/plugin.ts")).unwrap();
        let runtime = files.get(Path::new("src/lenso.bun.generated.ts")).unwrap();

        assert!(package.contains("\"runtime\": \"bun\""));
        assert!(package.contains("\"@lenso/bun\": \"0.2.0\""));
        assert!(author.contains("bindToolProviderProvider"));
        assert!(author.contains("definePlugin"));
        assert!(!author.contains("serve("));
        assert!(runtime.contains("serve(plugin)"));
    }

    #[test]
    fn process_plugin_scaffold_uses_the_sdk_owned_lowering() {
        let files = process_plugin_scaffold("uppercase");
        let manifest = files.get(Path::new("Cargo.toml")).unwrap();
        let entrypoint = files.get(Path::new("src/main.rs")).unwrap();

        assert!(manifest.contains("runtime = \"process\""));
        assert!(manifest.contains("package = \"lenso-plugin-sdk\""));
        assert!(manifest.contains("lenso-agent-tool-sdk"));
        assert_eq!(
            entrypoint,
            "// Cargo Process entrypoint; the SDK supplies main and protocol lowering.\ninclude!(\"lib.rs\");\n"
        );
        assert!(!files.contains_key(Path::new("lenso.generated.descriptor.json")));
    }

    #[test]
    fn multi_scaffold_keeps_one_business_source_for_two_outputs() {
        let files = multi_plugin_scaffold("uppercase");
        let manifest = files.get(Path::new("Cargo.toml")).unwrap();

        assert!(manifest.contains("outputs = [\"wasm\", \"process\"]"));
        assert!(files.contains_key(Path::new("src/lib.rs")));
        assert!(files.contains_key(Path::new("src/main.rs")));
        assert_eq!(
            files
                .keys()
                .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
                .count(),
            2
        );
        let author_source = files.get(Path::new("src/lib.rs")).unwrap();
        for runtime_detail in ["wit_bindgen", "ProcessPlugin", "ProcessOutcome", "Guest"] {
            assert!(
                !author_source.contains(runtime_detail),
                "author source leaked runtime detail `{runtime_detail}`"
            );
        }
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

    #[tokio::test]
    #[ignore = "clean-room test downloads released crates and compiles wasm32"]
    async fn clean_room_plugin_runs_new_check_dev_and_pack() {
        let root = tempfile::tempdir().unwrap();
        create(PluginNewArgs {
            plugin_id: "uppercase".to_owned(),
            repo_root: Some(root.path().to_path_buf()),
            dir: None,
            runtime: PluginRuntimeArg::Wasm,
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
            operation: Some("execute".to_owned()),
            request_json: r#"{"name":"uppercase","arguments_json":"{\"text\":\"hello\"}"}"#
                .to_owned(),
            json: true,
            watch: false,
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
        with_bundle_directory(&output, |directory| {
            verify_bundle_directory(directory)
                .map(|_| ())
                .map_err(Into::into)
        })
        .unwrap();
        assert!(
            pack(PluginPackArgs {
                repo_root: Some(project),
                output: Some(output),
                json: false,
            })
            .is_err()
        );
    }

    #[tokio::test]
    #[ignore = "clean-room test downloads git dependencies and compiles a native executable"]
    async fn clean_room_process_plugin_runs_new_check_dev_and_pack() {
        let root = tempfile::tempdir().unwrap();
        create(PluginNewArgs {
            plugin_id: "uppercase".to_owned(),
            repo_root: Some(root.path().to_path_buf()),
            dir: None,
            runtime: PluginRuntimeArg::Process,
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
            operation: Some("execute".to_owned()),
            request_json: r#"{"name":"uppercase","arguments_json":"{\"text\":\"hello\"}"}"#
                .to_owned(),
            json: true,
            watch: false,
        })
        .await
        .unwrap();
        let output = project.join("dist/uppercase.lenso-plugin");
        pack(PluginPackArgs {
            repo_root: Some(project),
            output: Some(output.clone()),
            json: true,
        })
        .unwrap();
        with_bundle_directory(&output, |directory| {
            verify_bundle_directory(directory)
                .map(|_| ())
                .map_err(Into::into)
        })
        .unwrap();
    }
}
