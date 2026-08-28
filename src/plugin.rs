use std::{
    any::Any,
    collections::BTreeMap,
    env, fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, anyhow, bail};
use clap::{Args, Subcommand, ValueEnum};
use lenso_app_plan::{
    CapabilityEndpointPlan, ExecutionClassId, PluginInstancePlan, ResolvedAppPlan,
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
use lenso_wasm_component_adapter::{EXECUTION_CLASS as WASM_EXECUTION_CLASS, WasmComponentAdapter};
use serde::Deserialize;
use serde_json::Value;

const GUEST_SDK_VERSION: &str = "0.2.0";
const PROCESS_RUNTIME_REVISION: &str = "7f5acd577374157cb57c72c9dd0d4b0e054c53ce";
const WASM_TARGET: &str = "wasm32-unknown-unknown";

#[derive(Clone, Debug, Subcommand)]
pub enum PluginCommand {
    /// Create one ordinary Rust Plugin with portable Wasm and Process outputs.
    New(PluginNewArgs),
    /// Build and run the Plugin through its generated execution adapter.
    Dev(PluginDevArgs),
    /// Validate Plugin source and generated descriptor evidence.
    Check(PluginCheckArgs),
    /// Build and verify one immutable `.lenso-plugin` directory.
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
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct PluginDescriptor {
    abi: String,
    capabilities: Vec<PluginCapability>,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
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
        run_cargo(&target, &["generate-lockfile"], "generate Plugin lockfile")?;
        if args.runtime != PluginRuntimeArg::Process {
            run_cargo(
                &target,
                &["check", "--locked", "--lib", "--target", WASM_TARGET],
                "check generated Wasm implementation",
            )?;
        }
        if args.runtime != PluginRuntimeArg::Wasm {
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

fn multi_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let mut files = plugin_scaffold(plugin_id);
    let process = process_plugin_scaffold(plugin_id);
    let manifest = files
        .get_mut(Path::new("Cargo.toml"))
        .expect("Wasm scaffold has a manifest");
    *manifest = manifest
        .replace("runtime = \"wasm\"", "outputs = [\"wasm\", \"process\"]")
        .replace(
            &format!("lenso-guest-sdk = \"{GUEST_SDK_VERSION}\""),
            &format!(
                "lenso-guest-sdk = \"{GUEST_SDK_VERSION}\"\nlenso-process-sdk = {{ version = \"0.1.0\", git = \"https://github.com/LioRael/lenso-runtime-rust\", rev = \"{PROCESS_RUNTIME_REVISION}\" }}"
            ),
        );
    let wasm_generated = files
        .remove(Path::new("src/lenso.generated.rs"))
        .expect("Wasm scaffold has generated source");
    files.insert(PathBuf::from("src/lenso.wasm.generated.rs"), wasm_generated);
    files.insert(
        PathBuf::from("src/lib.rs"),
        "mod plugin;\n\n// Generated execution lowering. Edit `plugin.rs`, not this file.\ninclude!(\"lenso.wasm.generated.rs\");\n"
            .to_owned(),
    );
    files.insert(
        PathBuf::from("src/main.rs"),
        "mod plugin;\n\n// Generated execution lowering. Edit `plugin.rs`, not this file.\ninclude!(\"lenso.process.generated.rs\");\n"
            .to_owned(),
    );
    files.insert(
        PathBuf::from("src/lenso.process.generated.rs"),
        process
            .get(Path::new("src/lenso.generated.rs"))
            .expect("Process scaffold has generated source")
            .clone(),
    );
    files.insert(
        PathBuf::from("lenso.generated.descriptor.json"),
        process
            .get(Path::new("lenso.generated.descriptor.json"))
            .expect("Process scaffold has descriptor")
            .clone(),
    );
    files.insert(
        PathBuf::from("README.md"),
        format!(
            "# {plugin_id}\n\nOne Rust Plugin source with portable Wasm and trusted Process outputs. Edit only `src/plugin.rs`; `lenso plugin pack` builds both implementations into one release.\n"
        ),
    );
    files
}

// Keep the complete scaffold together so its single public identity
// and forbidden author vocabulary remain reviewable in one place.
#[allow(clippy::too_many_lines)]
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
lenso-guest-sdk = "{GUEST_SDK_VERSION}"
schemars = "1"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
wit-bindgen = "0.60"

[workspace]
"#
            ),
        ),
        (
            PathBuf::from("src/lib.rs"),
            "mod plugin;\n\n// Generated execution lowering. Edit `plugin.rs`, not this file.\ninclude!(\"lenso.generated.rs\");\n".to_owned(),
        ),
        (
            PathBuf::from("src/plugin.rs"),
            r#"use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Arguments {
    #[schemars(length(max = 4096))]
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolError {
    InvalidArguments,
}

pub fn execute(arguments: Arguments) -> Result<String, ToolError> {
    if arguments.text.is_empty() {
        Err(ToolError::InvalidArguments)
    } else {
        Ok(arguments.text)
    }
}
"#
            .to_owned(),
        ),
        (
            PathBuf::from("src/lenso.generated.rs"),
            format!(
                r##"// Generated by `lenso plugin new`. Do not edit.
use serde::{{Deserialize, Serialize}};

wit_bindgen::generate!({{
    path: "wit",
    world: "plugin",
}});

const CAPABILITY: &str = "lenso.agent.tool-provider@2";
const TOOL: &str = "{plugin_id}";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteRequest {{
    name: String,
    arguments_json: String,
}}

#[derive(Serialize)]
struct ExecuteResponse {{
    content_type: &'static str,
    content: String,
    metadata_json: &'static str,
}}

fn catalog() -> Result<String, String> {{
    let schema = schemars::schema_for!(crate::plugin::Arguments);
    let input_schema_json = serde_json::to_string(&schema)
        .map_err(|_| "\"execution_failed\"".to_owned())?;
    serde_json::to_string(&serde_json::json!({{
        "tools": [{{
            "name": TOOL,
            "description": "Process one UTF-8 string.",
            "input_schema_json": input_schema_json,
            "execution": "parallel_safe",
        }}],
    }}))
    .map_err(|_| "\"execution_failed\"".to_owned())
}}

struct PluginComponent;

lenso_guest_sdk::guest_request_plugin! {{
impl Guest for PluginComponent {{
    provides: {{
        capability_id: "lenso.agent.tool-provider@2",
        descriptor_version: "2.0.0",
        requests: ["catalog", "execute"],
    }}

    fn invoke(
        capability: String,
        operation: String,
        request_json: String,
    ) -> Result<String, String> {{
        if capability != CAPABILITY {{
            return Err("\"not_found\"".to_owned());
        }}
        match operation.as_str() {{
            "catalog" => catalog(),
            "execute" => {{
                let request = serde_json::from_str::<ExecuteRequest>(&request_json)
                    .map_err(|_| "\"invalid_arguments\"".to_owned())?;
                if request.name != TOOL {{
                    return Err("\"not_found\"".to_owned());
                }}
                let arguments = serde_json::from_str::<crate::plugin::Arguments>(&request.arguments_json)
                    .map_err(|_| "\"invalid_arguments\"".to_owned())?;
                let content = crate::plugin::execute(arguments)
                    .map_err(|error| serde_json::to_string(&error)
                        .unwrap_or_else(|_| "\"execution_failed\"".to_owned()))?;
                serde_json::to_string(&ExecuteResponse {{
                    content_type: "text",
                    content,
                    metadata_json: r#"{{"provider":"external-wasm"}}"#,
                }})
                .map_err(|_| "\"execution_failed\"".to_owned())
            }}
            _ => Err("\"not_found\"".to_owned()),
        }}
    }}
}}
}}

export!(PluginComponent);
"##
            ),
        ),
        (
            PathBuf::from("wit/world.wit"),
            "package lenso:runtime@1.0.0;\n\nworld plugin {\n  export describe: func() -> string;\n  export invoke: func(capability: string, operation: string, request-json: string) -> result<string, string>;\n}\n".to_owned(),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {plugin_id}\n\nRust Plugin for the Lenso Agent Harness, packaged as an isolated Wasm Component. Edit only `src/plugin.rs`; Lenso owns the generated execution bridge.\n\n```sh\nlenso plugin check\nlenso plugin dev --operation execute --request-json '{{\"name\":\"{plugin_id}\",\"arguments_json\":\"{{\\\"text\\\":\\\"hello\\\"}}\"}}'\nlenso plugin pack\n```\n\nCreate another project with `lenso plugin new <id>`.\n"
            ),
        ),
    ])
}

#[allow(clippy::too_many_lines)]
fn process_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let package_name = plugin_id.replace('.', "-");
    let descriptor = r#"{"abi":"lenso.json-request@1","capabilities":[{"capability_id":"lenso.agent.tool-provider@2","descriptor_version":"2.0.0","request_operations":["catalog","execute"]}]}"#;
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
runtime = "process"

[dependencies]
lenso-process-sdk = {{ version = "0.1.0", git = "https://github.com/LioRael/lenso-runtime-rust", rev = "{PROCESS_RUNTIME_REVISION}" }}
schemars = "1"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"

[workspace]
"#
            ),
        ),
        (
            PathBuf::from("src/main.rs"),
            "mod plugin;\n\n// Generated execution lowering. Edit `plugin.rs`, not this file.\ninclude!(\"lenso.generated.rs\");\n"
                .to_owned(),
        ),
        (
            PathBuf::from("src/plugin.rs"),
            r#"use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Arguments {
    #[schemars(length(max = 4096))]
    pub text: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolError {
    InvalidArguments,
}

pub fn execute(arguments: Arguments) -> Result<String, ToolError> {
    if arguments.text.is_empty() {
        Err(ToolError::InvalidArguments)
    } else {
        Ok(arguments.text)
    }
}
"#
            .to_owned(),
        ),
        (
            PathBuf::from("src/lenso.generated.rs"),
            format!(
                r##"// Generated by `lenso plugin new`. Do not edit.
use lenso_process_sdk::{{ProcessOutcome, ProcessPlugin}};
use serde::{{Deserialize, Serialize}};
use serde_json::{{Value, json}};

const CAPABILITY: &str = "lenso.agent.tool-provider@2";
const TOOL: &str = "{plugin_id}";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExecuteRequest {{
    name: String,
    arguments_json: String,
}}

#[derive(Serialize)]
struct ExecuteResponse {{
    content_type: &'static str,
    content: String,
    metadata_json: &'static str,
}}

#[derive(Debug)]
struct GeneratedPlugin;

impl GeneratedPlugin {{
    fn catalog() -> ProcessOutcome {{
        let schema = schemars::schema_for!(crate::plugin::Arguments);
        let input_schema_json = match serde_json::to_string(&schema) {{
            Ok(schema) => schema,
            Err(error) => return ProcessOutcome::Failure(error.to_string()),
        }};
        ProcessOutcome::Success(json!({{
            "tools": [{{
                "name": TOOL,
                "description": "Process one UTF-8 string.",
                "input_schema_json": input_schema_json,
                "execution": "parallel_safe",
            }}],
        }}))
    }}
}}

impl ProcessPlugin for GeneratedPlugin {{
    fn descriptor(&self) -> Value {{
        serde_json::from_str(include_str!("../lenso.generated.descriptor.json"))
            .expect("generated descriptor is valid JSON")
    }}

    fn invoke(&self, capability: &str, operation: &str, request: Value) -> ProcessOutcome {{
        if capability != CAPABILITY {{
            return ProcessOutcome::DomainError(json!("not_found"));
        }}
        match operation {{
            "catalog" => Self::catalog(),
            "execute" => {{
                let request = match serde_json::from_value::<ExecuteRequest>(request) {{
                    Ok(request) => request,
                    Err(_) => return ProcessOutcome::DomainError(json!("invalid_arguments")),
                }};
                if request.name != TOOL {{
                    return ProcessOutcome::DomainError(json!("not_found"));
                }}
                let arguments = match serde_json::from_str::<crate::plugin::Arguments>(&request.arguments_json) {{
                    Ok(arguments) => arguments,
                    Err(_) => return ProcessOutcome::DomainError(json!("invalid_arguments")),
                }};
                match crate::plugin::execute(arguments) {{
                    Ok(content) => match serde_json::to_value(ExecuteResponse {{
                        content_type: "text",
                        content,
                        metadata_json: r#"{{"provider":"external-process"}}"#,
                    }}) {{
                        Ok(value) => ProcessOutcome::Success(value),
                        Err(error) => ProcessOutcome::Failure(error.to_string()),
                    }},
                    Err(error) => ProcessOutcome::DomainError(
                        serde_json::to_value(error).unwrap_or_else(|_| json!("execution_failed")),
                    ),
                }}
            }}
            _ => ProcessOutcome::DomainError(json!("not_found")),
        }}
    }}
}}

fn main() {{
    lenso_process_sdk::serve(&GeneratedPlugin).expect("serve Lenso Process Plugin");
}}
"##
            ),
        ),
        (
            PathBuf::from("lenso.generated.descriptor.json"),
            format!("{descriptor}\n"),
        ),
        (
            PathBuf::from("README.md"),
            format!(
                "# {plugin_id}\n\nTrusted native Process Plugin for the Lenso Agent Harness. Edit only `src/plugin.rs`; Lenso owns the generated protocol bridge and descriptor. Process Plugins are not sandboxed, so install only trusted bundles.\n\n```sh\nlenso plugin check\nlenso plugin dev --operation execute --request-json '{{\"name\":\"{plugin_id}\",\"arguments_json\":\"{{\\\"text\\\":\\\"hello\\\"}}\"}}'\nlenso plugin pack\n```\n"
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
    let package = read_package(&root.join("Cargo.toml"))?;
    let runtime = project_runtime(&package)?;
    let temporary = tempfile::tempdir().context("create Plugin dev directory")?;
    let output = temporary.path().join("dev.lenso-plugin");
    let verified = materialize(&root, &output)?;
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
        ProjectRuntime::Process => {
            parse_descriptor_bytes(&fs::read(root.join("lenso.generated.descriptor.json"))?)?
        }
    };
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
    let adapter =
        WasmComponentAdapter::new(artifacts).with_codec(DynamicJsonCodec::new(capability));
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
    synchronize_plugin_lock(root, &package)?;
    let target_directory = cargo_target_directory(root)?;
    match project_runtime(&package)? {
        ProjectRuntime::Multi => materialize_multi(root, output, &package, &target_directory),
        ProjectRuntime::Wasm => {
            run_cargo(
                root,
                &["build", "--locked", "--release", "--target", WASM_TARGET],
                "build Plugin Wasm",
            )?;
            let artifact = target_directory
                .join(WASM_TARGET)
                .join("release")
                .join(format!("{}.wasm", package.name.replace('-', "_")));
            Ok(build_source_plugin_bundle(&SourcePluginBuild {
                package_manifest: manifest,
                wasm_module: artifact,
                output: output.to_path_buf(),
            })?)
        }
        ProjectRuntime::Process => {
            run_cargo(
                root,
                &["build", "--locked", "--release"],
                "build Process Plugin",
            )?;
            let executable = target_directory.join("release").join(&package.name);
            Ok(build_source_process_plugin_bundle(
                &SourceProcessPluginBuild {
                    package_manifest: manifest,
                    executable,
                    runtime_descriptor: root.join("lenso.generated.descriptor.json"),
                    target: format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS),
                    output: output.to_path_buf(),
                },
            )?)
        }
    }
    .with_context(|| format!("package Plugin `{}`", package.metadata.lenso.plugin_id))
}

fn materialize_multi(
    root: &Path,
    output: &Path,
    package: &CargoPackage,
    target_directory: &Path,
) -> anyhow::Result<VerifiedBundle> {
    run_cargo(
        root,
        &[
            "build",
            "--locked",
            "--release",
            "--lib",
            "--target",
            WASM_TARGET,
        ],
        "build Plugin Wasm implementation",
    )?;
    run_cargo(
        root,
        &["build", "--locked", "--release", "--bin", &package.name],
        "build Plugin Process implementation",
    )?;
    let staging = tempfile::tempdir().context("stage Plugin implementations")?;
    let wasm_bundle = staging.path().join("wasm");
    build_source_plugin_bundle(&SourcePluginBuild {
        package_manifest: root.join("Cargo.toml"),
        wasm_module: target_directory
            .join(WASM_TARGET)
            .join("release")
            .join(format!("{}.wasm", package.name.replace('-', "_"))),
        output: wasm_bundle.clone(),
    })?;
    let process_bundle = staging.path().join("process");
    let host_target = format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS);
    build_source_process_plugin_bundle(&SourceProcessPluginBuild {
        package_manifest: root.join("Cargo.toml"),
        executable: target_directory.join("release").join(&package.name),
        runtime_descriptor: root.join("lenso.generated.descriptor.json"),
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
    fn rust_plugin_scaffold_has_one_plugin_authority_and_four_commands() {
        let files = plugin_scaffold("uppercase");
        let all = files.values().cloned().collect::<String>();

        assert!(all.contains("guest_request_plugin!"));
        let author_source = files.get(Path::new("src/plugin.rs")).unwrap();
        for internal in [
            "wit_bindgen",
            "guest_request_plugin",
            "request_json",
            "arguments_json",
            "lenso.agent.tool-provider",
        ] {
            assert!(
                !author_source.contains(internal),
                "author source leaked `{internal}`"
            );
        }
        assert!(author_source.contains("pub fn execute(arguments: Arguments)"));
        assert!(author_source.contains("JsonSchema"));
        assert!(all.contains("plugin-id = \"uppercase\""));
        assert!(all.contains("root-slot = \"tool-providers\""));
        assert!(all.contains("lenso.agent.tool-provider@2"));
        assert!(all.contains("requests: [\"catalog\", \"execute\"]"));
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
    fn process_plugin_scaffold_keeps_protocol_lowering_generated() {
        let files = process_plugin_scaffold("uppercase");
        let author_source = files.get(Path::new("src/plugin.rs")).unwrap();
        let generated_source = files.get(Path::new("src/lenso.generated.rs")).unwrap();
        let manifest = files.get(Path::new("Cargo.toml")).unwrap();

        assert!(author_source.contains("pub fn execute(arguments: Arguments)"));
        for internal in [
            "ProcessPlugin",
            "ProcessOutcome",
            "descriptor",
            "request_json",
            "arguments_json",
        ] {
            assert!(
                !author_source.contains(internal),
                "author source leaked `{internal}`"
            );
        }
        assert!(generated_source.contains("impl ProcessPlugin for GeneratedPlugin"));
        assert!(generated_source.contains("lenso_process_sdk::serve"));
        assert!(manifest.contains("runtime = \"process\""));
        assert!(manifest.contains("root-slot = \"tool-providers\""));
        assert!(files.contains_key(Path::new("lenso.generated.descriptor.json")));
    }

    #[test]
    fn multi_scaffold_keeps_one_author_source_for_two_outputs() {
        let files = multi_plugin_scaffold("uppercase");
        let manifest = files.get(Path::new("Cargo.toml")).unwrap();

        assert!(manifest.contains("outputs = [\"wasm\", \"process\"]"));
        assert!(files.contains_key(Path::new("src/plugin.rs")));
        assert!(files.contains_key(Path::new("src/lenso.wasm.generated.rs")));
        assert!(files.contains_key(Path::new("src/lenso.process.generated.rs")));
        assert_eq!(
            files
                .keys()
                .filter(|path| path.as_path() == Path::new("src/plugin.rs"))
                .count(),
            1
        );
        let author_source = files.get(Path::new("src/plugin.rs")).unwrap();
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
        verify_bundle_directory(&output).unwrap();
    }
}
