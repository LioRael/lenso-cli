use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use lenso_app_authoring::identity::validate_plugin_id_v1;

use super::{PluginNewArgs, PluginRuntimeArg, WASM_TARGET, run_bun, run_cargo};

const PLUGIN_SDK_REVISION: &str = "7c54f4065012d41769fefbb41098a4657f1f4825";
const AGENT_TOOL_SDK_REVISION: &str = "fd944a4ee56026be708b50c710635d1b17a59758";

pub(super) fn create(args: PluginNewArgs) -> anyhow::Result<()> {
    validate_plugin_id_v1(&args.plugin_id)?;
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

pub(super) fn bun_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
    let package_name = plugin_id.replace('.', "-");
    BTreeMap::from([
        (
            PathBuf::from("package.json"),
            bun_package_manifest(plugin_id, &package_name),
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
            bun_author_source(plugin_id),
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

fn bun_package_manifest(plugin_id: &str, package_name: &str) -> String {
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
    )
}

fn bun_author_source(plugin_id: &str) -> String {
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
    )
}

pub(super) fn multi_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
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

pub(super) fn plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
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

pub(super) fn process_plugin_scaffold(plugin_id: &str) -> BTreeMap<PathBuf, String> {
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
