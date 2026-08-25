use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use lenso_authoring::{
    CapabilityEndpoint, ContractInput, Module, PackageInput, PackageSource, ProjectFile,
};
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct ModuleCreateOptions {
    pub capability: Option<String>,
    pub dir: Option<PathBuf>,
    pub dry_run: bool,
    pub module_id: String,
    pub no_install: bool,
    pub repo_root: Option<PathBuf>,
    pub recipe: ModuleRecipe,
    pub runtime: ModuleRuntime,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ModuleRuntime {
    Rust,
    Bun,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ModuleRecipe {
    Stateless,
    Stateful,
    WebConsole,
    ManagedWork,
}

type PendingWrites = BTreeMap<PathBuf, String>;

pub fn create_module(options: &ModuleCreateOptions) -> Result<()> {
    let module_id = slugify(&options.module_id);
    if module_id.is_empty() {
        bail!("Module id is required");
    }

    match options.runtime {
        ModuleRuntime::Rust => {
            let base = options.repo_root.as_deref().map_or_else(
                || std::env::current_dir().context("resolve current directory"),
                absolutize,
            )?;
            create_standalone_rust_module(options, &base, &module_id)
        }
        ModuleRuntime::Bun => create_bun_module(options),
    }
}

fn create_standalone_rust_module(
    options: &ModuleCreateOptions,
    base: &Path,
    module_id: &str,
) -> Result<()> {
    let target = options
        .dir
        .as_deref()
        .map_or_else(|| base.join(module_id), |dir| resolve_path(base, dir));
    if target.exists() {
        bail!(
            "Rust Module project directory already exists: {}",
            target.display()
        );
    }
    let capability_id = options
        .capability
        .clone()
        .unwrap_or_else(|| format!("local.{module_id}@1"));
    let files = rust_scaffold_files(&target, module_id, &capability_id, options.recipe)?;
    if options.dry_run {
        println!("Rust Module dry run:");
        for path in files.keys() {
            println!("- {}", display_relative(&target, path));
        }
        let generated = target.join("capability/src/generated.rs");
        println!("- {}", display_relative(&target, &generated));
        return Ok(());
    }
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("Rust Module project target must have a parent directory"))?;
    fs::create_dir_all(parent)?;
    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("rust-module");
    let stage = parent.join(format!(
        ".{target_name}.lenso-stage-{}",
        uuid::Uuid::now_v7()
    ));
    fs::create_dir(&stage)?;
    let result = materialize_rust_scaffold(&files, &target, &stage, !options.no_install);
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    fs::rename(&stage, &target).with_context(|| {
        format!(
            "publish complete Rust Module scaffold {} to {}",
            stage.display(),
            target.display()
        )
    })?;
    println!("Created Rust Module project at {}.", target.display());
    println!("Next steps:");
    if options.no_install {
        println!("- cd {} && cargo generate-lockfile", target.display());
    } else {
        println!("- dependencies locked and generated project checked");
    }
    println!("- cd {} && lenso dev", target.display());
    println!("- cd {} && lenso verify", target.display());
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn rust_scaffold_files(
    target: &Path,
    module_id: &str,
    capability_id: &str,
    recipe: ModuleRecipe,
) -> Result<PendingWrites> {
    const DESCRIPTOR_VERSION: &str = "1.0.0";
    const MODULE_VERSION: &str = "0.1.0";
    let package_name = format!("lenso-module-{module_id}");
    let crate_name = snake_case(&package_name);
    let capability_package_name = format!("lenso-capability-{module_id}");
    let capability_crate_name = snake_case(&capability_package_name);
    let package_id = format!("local.{module_id}");
    let type_name = pascal_case(module_id);
    let contract_root = "capability";
    let descriptor_path = format!("{contract_root}/capability.json");
    let rust_path = format!("{contract_root}/src/generated.rs");

    let mut project = ProjectFile::default();
    project.packages_mut().insert(
        package_id.clone(),
        PackageInput::new(&package_id, PackageSource::Cargo, MODULE_VERSION)
            .with_package_name(&package_name)
            .with_manifest("Cargo.toml")
            .with_lockfile("Cargo.lock"),
    );
    project
        .composition_mut()
        .add_module(Module::new(module_id, &package_id).with_capability(
            CapabilityEndpoint::request(capability_id, DESCRIPTOR_VERSION, ["execute"]),
        ));
    project.contracts_mut().push(
        ContractInput::descriptor_only(capability_id, DESCRIPTOR_VERSION, &descriptor_path)
            .with_rust_projection(&rust_path),
    );

    let descriptor = json!({
        "id": capability_id,
        "version": DESCRIPTOR_VERSION,
        "portable": true,
        "cross_lane_transfer": true,
        "operations": [{
            "name": "execute",
            "interaction": "request",
            "request_schema": "schemas/execute-request.schema.json",
            "response_schema": "schemas/execute-response.schema.json",
            "domain_error_schema": "schemas/execute-error.schema.json"
        }]
    });
    let request_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["input"],
        "properties": { "input": { "type": "string" } },
        "additionalProperties": false
    });
    let response_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["output"],
        "properties": { "output": { "type": "string" } },
        "additionalProperties": false
    });
    let error_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "oneOf": [{ "const": "invalid_input" }]
    });
    let cargo_toml = format!(
        r#"[package]
name = "{package_name}"
version = "{MODULE_VERSION}"
edition = "2024"
rust-version = "1.94"
license = "MIT"

[package.metadata.lenso]
package-id = "{package_id}"

[workspace]
members = ["capability"]

[dependencies]
futures = "0.3"
lenso = {{ git = "https://github.com/LioRael/lenso-runtime-rust", rev = "ee531ede8d7b7e94dc284ce20b99f8e277bdcdc0" }}
lenso-app-plan = "0.1.0"
lenso-kernel = "0.1.5"
lenso-native-adapter = {{ git = "https://github.com/LioRael/lenso-runtime-rust", rev = "ee531ede8d7b7e94dc284ce20b99f8e277bdcdc0" }}
lenso-runner = {{ git = "https://github.com/LioRael/lenso-runtime-rust", rev = "ee531ede8d7b7e94dc284ce20b99f8e277bdcdc0" }}
{capability_package_name} = {{ path = "capability" }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
tokio = {{ version = "1.52", features = ["macros", "rt", "signal", "time"] }}
"#
    );
    let capability_cargo_toml = format!(
        r#"[package]
name = "{capability_package_name}"
version = "0.1.0"
edition = "2024"
rust-version = "1.94"
license = "MIT"

[dependencies]
futures = "0.3"
lenso-contract-runtime = "0.1.0"
lenso-kernel = "0.1.5"
lenso-module-authoring = {{ git = "https://github.com/LioRael/lenso-protocols", rev = "16c4aff52c539e16f3024f6414de56e6c181b030" }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#
    );
    let module_source = format!(
        r#"use lenso::prelude::*;
use {capability_crate_name} as capability;
use capability::{{ExecuteError, ExecuteRequest, ExecuteResponse}};

#[allow(dead_code)]
#[derive(Clone, Debug, serde::Deserialize, ModuleConfig)]
struct {type_name}Config {{}}

#[module]
#[derive(Clone, Debug)]
pub struct {type_name}Module {{
    #[config]
    config: {type_name}Config,
}}

#[provides(capability::{type_name})]
impl {type_name}Module {{
    async fn execute(
        &self,
        _ctx: Ctx,
        request: ExecuteRequest,
    ) -> Result<ExecuteResponse, ExecuteError> {{
        let _ = &self.config;
        if request.input.trim().is_empty() {{
            Err(ExecuteError::InvalidInput)
        }} else {{
            Ok(ExecuteResponse {{ output: request.input }})
        }}
    }}
}}

#[cfg(test)]
mod tests {{
    use super::*;

    fn module() -> {type_name}Module {{
        {type_name}Module {{ config: {type_name}Config {{}} }}
    }}

    fn context() -> Ctx {{
        Ctx::new(1, None, lenso_kernel::CancellationToken::new())
    }}

    #[tokio::test(flavor = "current_thread")]
    async fn provider_returns_success() {{
        let result = module().execute(
            context(),
            ExecuteRequest {{ input: "Ada".to_owned() }},
        ).await.unwrap();
        assert_eq!(result.output, "Ada");
    }}

    #[tokio::test(flavor = "current_thread")]
    async fn provider_returns_domain_error() {{
        let result = module().execute(
            context(),
            ExecuteRequest {{ input: " ".to_owned() }},
        ).await;
        assert!(matches!(result, Err(ExecuteError::InvalidInput)));
    }}

    #[test]
    fn generated_descriptor_is_package_owned() {{
        let descriptor: serde_json::Value = serde_json::from_str(MODULE_DESCRIPTOR_JSON).unwrap();
        assert_eq!(descriptor["package_id"], "{package_id}");
        assert_eq!(descriptor["provided_capabilities"][0]["capability_id"], "{capability_id}");
    }}

    #[test]
    fn linked_factory_is_discoverable() {{
        let count = lenso_native_adapter::NativeModuleRegistry::new()
            .with_linked_factories()
            .factories()
            .filter(|factory| factory.package_id() == PACKAGE_ID)
            .count();
        assert_eq!(count, 1);
    }}
}}
"#
    );
    let runner_source = format!(
        r#"use std::{{fs, time::Duration}};

use {crate_name} as _;
use lenso_app_plan::ResolvedAppPlan;
use lenso_kernel::ExecutionAdapterCatalog;
use lenso_native_adapter::NativeModuleRegistry;
use lenso_runner::TokioDriver;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {{
    let plan_path = std::env::args().nth(1).unwrap_or_else(|| ".lenso/resolved-plan.json".to_owned());
    let plan: ResolvedAppPlan = serde_json::from_slice(&fs::read(&plan_path)?)?;
    let driver = TokioDriver::new();
    let shutdown = driver.clone();
    let local = tokio::task::LocalSet::new();
    local.spawn_local(async move {{
        if tokio::signal::ctrl_c().await.is_ok() {{ shutdown.request_shutdown(); }}
    }});
    let adapters = ExecutionAdapterCatalog::single(
        NativeModuleRegistry::new().with_linked_factories(),
    );
    let outcome = local.run_until(lenso_runner::run(
        plan,
        driver,
        adapters,
        Duration::from_secs(10),
    )).await?;
    println!("{{outcome:?}}");
    Ok(())
}}
"#
    );
    let verification = json!({
        "protocol": "lenso.module-verification-manifest.v1",
        "probes": [
            { "id": "package", "purpose": "package", "command": "cargo test --locked" },
            { "id": "success", "purpose": "success", "command": "cargo test --locked provider_returns_success" },
            { "id": "domain-error", "purpose": "domain_error", "command": "cargo test --locked provider_returns_domain_error" },
            { "id": "runtime-failure", "purpose": "runtime_failure", "command": "cargo test --locked generated_descriptor_is_package_owned" },
            { "id": "lifecycle-cleanup", "purpose": "lifecycle_cleanup", "command": "cargo test --locked linked_factory_is_discoverable" }
        ]
    });
    let readme = format!(
        "# {module_id}\n\nStandalone native Rust Module for `{capability_id}`. Business code uses the stable `lenso` facade; Descriptor lowering, endpoints, factory construction, and link-time registration are generated.\n\n```sh\nlenso check\nlenso dev\nlenso verify\n```\n\nThe development Runner discovers this package's generated linked factory; production Apps still own their Runner assembly.\n"
    );

    let mut files = PendingWrites::new();
    queue_write(
        &mut files,
        target.join(".gitignore"),
        "target\n.lenso\n".to_owned(),
    );
    queue_write(&mut files, target.join("Cargo.toml"), cargo_toml);
    queue_write(
        &mut files,
        target.join("capability/Cargo.toml"),
        capability_cargo_toml,
    );
    queue_write(
        &mut files,
        target.join("capability/src/lib.rs"),
        "//! Generated portable Capability contract.\n\ninclude!(\"generated.rs\");\n".to_owned(),
    );
    queue_write(&mut files, target.join("README.md"), readme);
    queue_write(
        &mut files,
        target.join("MODULE.md"),
        module_card(module_id, capability_id, recipe),
    );
    queue_write(&mut files, target.join("src/lib.rs"), module_source);
    queue_write(
        &mut files,
        target.join("src/bin/lenso-module-dev.rs"),
        runner_source,
    );
    queue_write(
        &mut files,
        target.join("lenso.json"),
        format!("{}\n", serde_json::to_string_pretty(&project)?),
    );
    queue_write(
        &mut files,
        target.join("lenso.module.verify.json"),
        json_string_pretty(&verification)?,
    );
    queue_write(
        &mut files,
        target.join(&descriptor_path),
        json_string_pretty(&descriptor)?,
    );
    queue_write(
        &mut files,
        target.join(format!(
            "{contract_root}/schemas/execute-request.schema.json"
        )),
        json_string_pretty(&request_schema)?,
    );
    queue_write(
        &mut files,
        target.join(format!(
            "{contract_root}/schemas/execute-response.schema.json"
        )),
        json_string_pretty(&response_schema)?,
    );
    queue_write(
        &mut files,
        target.join(format!("{contract_root}/schemas/execute-error.schema.json")),
        json_string_pretty(&error_schema)?,
    );
    Ok(files)
}

fn materialize_rust_scaffold(
    files: &PendingWrites,
    target: &Path,
    stage: &Path,
    check: bool,
) -> Result<()> {
    for (path, contents) in files {
        let relative = path
            .strip_prefix(target)
            .with_context(|| format!("Rust scaffold path {} escaped target", path.display()))?;
        write_file(&stage.join(relative), contents.as_bytes())?;
    }
    let descriptor = stage.join("capability/capability.json");
    let generated = lenso_contract_codegen_next::generate_projection(
        &descriptor,
        lenso_contract_codegen_next::ProjectionLanguage::Rust,
    )
    .with_context(|| format!("generate Rust binding from {}", descriptor.display()))?;
    write_file(
        &stage.join("capability/src/generated.rs"),
        generated.source.as_bytes(),
    )?;
    if check {
        run_rust_scaffold_command(stage, &["generate-lockfile"])?;
        run_rust_scaffold_command(stage, &["check", "--locked"])?;
        run_rust_scaffold_command(stage, &["test", "--locked"])?;
    }
    Ok(())
}

fn run_rust_scaffold_command(stage: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("cargo")
        .args(args)
        .current_dir(stage)
        .status()
        .with_context(|| format!("run `cargo {}`", args.join(" ")))?;
    if !status.success() {
        bail!("`cargo {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

fn create_bun_module(options: &ModuleCreateOptions) -> Result<()> {
    let module_id = slugify(&options.module_id);
    if module_id.is_empty() {
        bail!("Module id is required");
    }
    let target = bun_project_target(options, &module_id)?;
    if target.exists() {
        bail!("Bun project directory already exists: {}", target.display());
    }

    let capability_id = options
        .capability
        .clone()
        .unwrap_or_else(|| format!("local.{module_id}@1"));
    let files = bun_scaffold_files(&target, &module_id, &capability_id, options.recipe)?;
    if options.dry_run {
        println!("Bun Module dry run:");
        for path in files.keys() {
            println!("- {}", display_relative(&target, path));
        }
        let generated = target.join(format!("contracts/{module_id}/generated/bindings.ts"));
        println!("- {}", display_relative(&target, &generated));
        return Ok(());
    }

    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("Bun project target must have a parent directory"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create Bun project parent {}", parent.display()))?;
    let target_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("bun-module");
    let stage = parent.join(format!(
        ".{target_name}.lenso-stage-{}",
        uuid::Uuid::now_v7()
    ));
    fs::create_dir(&stage)
        .with_context(|| format!("create Bun scaffold stage {}", stage.display()))?;

    let materialized =
        materialize_bun_scaffold(&files, &target, &stage, &module_id, !options.no_install);
    if let Err(error) = materialized {
        if let Err(cleanup_error) = fs::remove_dir_all(&stage) {
            return Err(error.context(format!(
                "also failed to remove incomplete scaffold {}: {cleanup_error}",
                stage.display()
            )));
        }
        return Err(error);
    }
    if let Err(source) = fs::rename(&stage, &target) {
        let error = anyhow!(source).context(format!(
            "publish complete Bun scaffold {} to {}",
            stage.display(),
            target.display()
        ));
        if let Err(cleanup_error) = fs::remove_dir_all(&stage) {
            return Err(error.context(format!(
                "also failed to remove complete staging directory {}: {cleanup_error}",
                stage.display()
            )));
        }
        return Err(error);
    }

    println!("Created Bun Module project at {}.", target.display());
    println!("Next steps:");
    if options.no_install {
        println!("- cd {} && bun install", target.display());
    } else {
        println!("- dependencies installed and generated types checked");
    }
    println!("- cd {} && lenso dev", target.display());
    Ok(())
}

fn bun_project_target(options: &ModuleCreateOptions, module_id: &str) -> Result<PathBuf> {
    let base = match options.repo_root.as_deref() {
        Some(path) => absolutize(path)?,
        None => std::env::current_dir().context("resolve current directory")?,
    };
    let dir = options
        .dir
        .as_deref()
        .unwrap_or_else(|| Path::new(module_id));
    Ok(if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        base.join(dir)
    })
}

#[allow(clippy::too_many_lines)]
fn bun_scaffold_files(
    target: &Path,
    module_id: &str,
    capability_id: &str,
    recipe: ModuleRecipe,
) -> Result<PendingWrites> {
    const DESCRIPTOR_VERSION: &str = "1.0.0";
    const MODULE_VERSION: &str = "0.1.0";

    let package_name = format!("lenso-module-{module_id}");
    let package_id = format!("local.{module_id}");
    let contract_root = format!("contracts/{module_id}");
    let module_root = format!("modules/{module_id}");
    let workspace_revision = format!("workspace:{module_root}");
    let descriptor_path = format!("{contract_root}/capability.json");
    let typescript_path = format!("{contract_root}/generated/bindings.ts");

    let mut project = ProjectFile::default();
    let package = PackageInput::new(&package_id, PackageSource::Bun, &workspace_revision)
        .with_package_name(&package_name)
        .with_locked_revision(&workspace_revision)
        .with_manifest(format!("{module_root}/package.json"))
        .with_lockfile("bun.lock");
    project.packages_mut().insert(package_id.clone(), package);
    project.composition_mut().add_module(
        Module::new(module_id, &package_id)
            .with_entrypoint(format!("{module_root}/src/index.ts"))
            .with_capability(CapabilityEndpoint::request(
                capability_id,
                DESCRIPTOR_VERSION,
                ["execute"],
            )),
    );
    project.contracts_mut().push(
        ContractInput::descriptor_only(capability_id, DESCRIPTOR_VERSION, &descriptor_path)
            .with_typescript_projection(&typescript_path),
    );

    let descriptor = json!({
        "id": capability_id,
        "version": DESCRIPTOR_VERSION,
        "portable": true,
        "cross_lane_transfer": true,
        "operations": [{
            "name": "execute",
            "interaction": "request",
            "request_schema": "schemas/execute-request.schema.json",
            "response_schema": "schemas/execute-response.schema.json",
            "domain_error_schema": "schemas/execute-error.schema.json"
        }]
    });
    let request_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["input"],
        "properties": { "input": { "type": "string" } },
        "additionalProperties": false
    });
    let response_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "required": ["output"],
        "properties": { "output": { "type": "string" } },
        "additionalProperties": false
    });
    let error_schema = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "oneOf": [{ "const": "invalid_input" }]
    });

    let root_package = json!({
        "name": format!("{module_id}-app"),
        "private": true,
        "type": "module",
        "packageManager": "bun@1.2.21",
        "workspaces": ["modules/*"],
        "scripts": {
            "check": "lenso check",
            "dev": "lenso dev",
            "verify": "lenso verify",
            "test": "bun test",
            "typecheck": "tsc -p tsconfig.json"
        },
        "devDependencies": {
            "@types/bun": "1.2.21",
            "typescript": "5.9.2"
        }
    });
    let module_package = json!({
        "name": package_name,
        "version": MODULE_VERSION,
        "private": true,
        "type": "module",
        "scripts": { "typecheck": "tsc -p ../../tsconfig.json" },
        "dependencies": {
            "@lenso/bun-module": "0.1.0",
            "@lenso/contract-runtime": "0.1.0"
        },
        "engines": { "bun": ">=1.2.21" }
    });
    let tsconfig = json!({
        "compilerOptions": {
            "allowImportingTsExtensions": true,
            "module": "ESNext",
            "moduleResolution": "Bundler",
            "noEmit": true,
            "skipLibCheck": true,
            "strict": true,
            "target": "ES2022",
            "types": ["bun"]
        },
        "include": ["contracts/**/*.ts", "modules/**/*.ts"]
    });
    let module_source = format!(
        r#"import {{ defineModule, serve }} from "@lenso/bun-module";
import {{ bindProvider, type Provider }} from "../../../{typescript_path}";

export const provider: Provider = {{
  async execute(_context, request) {{
    return {{ ok: true, value: {{ output: request.input }} }};
  }},
}};

serve(defineModule({{ providers: [bindProvider(provider)] }}));
"#
    );
    let module_test = r#"import { describe, expect, test } from "bun:test";
import { provider } from "./index.ts";

describe("Module Provider", () => {
  test("returns success", async () => {
    const result = await provider.execute({} as never, { input: "Ada" });
    expect(result).toEqual({ ok: true, value: { output: "Ada" } });
  });

  test("returns a Domain Error", async () => {
    const result = await provider.execute({} as never, { input: " " });
    expect(result).toEqual({ ok: false, error: { kind: "domain", error: "invalid_input" } });
  });

  test("does not retain mutable state across calls", async () => {
    const first = await provider.execute({} as never, { input: "first" });
    const second = await provider.execute({} as never, { input: "second" });
    expect(first).toEqual({ ok: true, value: { output: "first" } });
    expect(second).toEqual({ ok: true, value: { output: "second" } });
  });
});
"#;
    let verification = json!({
        "protocol": "lenso.module-verification-manifest.v1",
        "probes": [
            { "id": "package", "purpose": "package", "command": "bun run typecheck && bun test" },
            { "id": "success", "purpose": "success", "command": "bun test --test-name-pattern 'returns success'" },
            { "id": "domain-error", "purpose": "domain_error", "command": "bun test --test-name-pattern 'returns a Domain Error'" },
            { "id": "runtime-failure", "purpose": "runtime_failure", "command": "lenso check --project .lenso/missing-project.json", "expectFailure": true },
            { "id": "lifecycle-cleanup", "purpose": "lifecycle_cleanup", "command": "bun test --test-name-pattern 'does not retain mutable state'" }
        ]
    });
    let readme = format!(
        r"# {module_id}

Bun Module scaffold for `{capability_id}`.

```sh
bun install
bun run typecheck
lenso check
lenso dev
lenso verify
```

Implement the typed Provider in `{module_root}/src/index.ts`. The checked-in
Descriptor and generated bindings live under `{contract_root}`.
"
    );

    let mut files = PendingWrites::new();
    queue_write(
        &mut files,
        target.join(".gitignore"),
        "node_modules\n.lenso\n".to_owned(),
    );
    queue_write(&mut files, target.join("README.md"), readme);
    queue_write(
        &mut files,
        target.join("MODULE.md"),
        module_card(module_id, capability_id, recipe),
    );
    queue_write(
        &mut files,
        target.join("lenso.json"),
        format!("{}\n", serde_json::to_string_pretty(&project)?),
    );
    queue_write(
        &mut files,
        target.join("package.json"),
        json_string_pretty(&root_package)?,
    );
    queue_write(
        &mut files,
        target.join("tsconfig.json"),
        json_string_pretty(&tsconfig)?,
    );
    queue_write(
        &mut files,
        target.join(&descriptor_path),
        json_string_pretty(&descriptor)?,
    );
    queue_write(
        &mut files,
        target.join(format!(
            "{contract_root}/schemas/execute-request.schema.json"
        )),
        json_string_pretty(&request_schema)?,
    );
    queue_write(
        &mut files,
        target.join(format!(
            "{contract_root}/schemas/execute-response.schema.json"
        )),
        json_string_pretty(&response_schema)?,
    );
    queue_write(
        &mut files,
        target.join(format!("{contract_root}/schemas/execute-error.schema.json")),
        json_string_pretty(&error_schema)?,
    );
    queue_write(
        &mut files,
        target.join(format!("{module_root}/package.json")),
        json_string_pretty(&module_package)?,
    );
    queue_write(
        &mut files,
        target.join(format!("{module_root}/src/index.ts")),
        module_source,
    );
    queue_write(
        &mut files,
        target.join(format!("{module_root}/src/index.test.ts")),
        module_test.to_owned(),
    );
    queue_write(
        &mut files,
        target.join("lenso.module.verify.json"),
        json_string_pretty(&verification)?,
    );
    Ok(files)
}

fn materialize_bun_scaffold(
    files: &PendingWrites,
    target: &Path,
    stage: &Path,
    module_id: &str,
    install: bool,
) -> Result<()> {
    for (path, contents) in files {
        let relative = path
            .strip_prefix(target)
            .with_context(|| format!("scaffold path {} escaped target", path.display()))?;
        write_file(&stage.join(relative), contents.as_bytes())?;
    }

    let descriptor = stage.join(format!("contracts/{module_id}/capability.json"));
    let generated = lenso_contract_codegen_next::generate_projection(
        &descriptor,
        lenso_contract_codegen_next::ProjectionLanguage::TypeScript,
    )
    .with_context(|| format!("generate TypeScript binding from {}", descriptor.display()))?;
    write_file(
        &stage.join(format!("contracts/{module_id}/generated/bindings.ts")),
        generated.source.as_bytes(),
    )?;

    if install {
        run_bun_scaffold_command(stage, &["install"])?;
        run_bun_scaffold_command(stage, &["run", "typecheck"])?;
        run_bun_scaffold_command(stage, &["test"])?;
    }
    Ok(())
}

fn run_bun_scaffold_command(stage: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("bun")
        .args(args)
        .current_dir(stage)
        .status()
        .with_context(|| format!("run `bun {}`", args.join(" ")))?;
    if !status.success() {
        bail!("`bun {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

fn queue_write(pending_writes: &mut PendingWrites, file_path: PathBuf, contents: String) {
    pending_writes.insert(file_path, contents);
}
fn json_string_pretty(value: &Value) -> Result<String> {
    let mut contents = serde_json::to_string_pretty(value)?;
    contents.push('\n');
    Ok(contents)
}
fn slugify(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_dash = false;
    for character in value.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            last_was_dash = false;
        } else if !last_was_dash && !output.is_empty() {
            output.push('-');
            last_was_dash = true;
        }
    }
    output.trim_matches('-').to_owned()
}

fn snake_case(value: &str) -> String {
    value.replace('-', "_")
}

fn pascal_case(value: &str) -> String {
    let mut output = String::new();
    for part in value.split(['-', '_']).filter(|part| !part.is_empty()) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.push(first.to_ascii_uppercase());
            output.push_str(chars.as_str());
        }
    }
    output
}

fn module_card(module_id: &str, capability_id: &str, recipe: ModuleRecipe) -> String {
    let (shape, owned_resources, lifecycle, first_behavior) = match recipe {
        ModuleRecipe::Stateless => (
            "Stateless Request Module",
            "None by default",
            "Create a fresh Provider generation; no managed work",
            "One typed request returns success or a Domain Error",
        ),
        ModuleRecipe::Stateful => (
            "Stateful Module",
            "Module-owned tables, migrations, and optional transactional Outbox",
            "Validate configuration in prepare; open state in activate; close it in deactivate",
            "One state transition is observable through the provided Capability",
        ),
        ModuleRecipe::WebConsole => (
            "Web and Console UI Module",
            "Module-owned UI artifact and any product-specific HTTP routes",
            "Keep ingress behind the App Ready Gate and bind the UI artifact to the Module Release",
            "One real browser route consumes a typed Capability",
        ),
        ModuleRecipe::ManagedWork => (
            "Managed background-work Module",
            "Generation-owned tasks, cancellation handles, and checkpoints",
            "Spawn only in activate; cancel and join every task in deactivate",
            "One work item reaches a terminal observable outcome",
        ),
    };
    format!(
        "# Module card: {module_id}\n\n- Shape: {shape}\n- Deletion boundary: removing `{module_id}` removes its behavior, state meaning, policy, tasks, and operational complexity.\n- Owned facts: TODO — name the business facts for which this Module has final authorization.\n- Provided Capabilities: `{capability_id}`\n- Required Capabilities: none in the starter; declare every dependency explicitly before use.\n- Configuration: opaque, non-secret values only; use secret references for credentials.\n- External resources: {owned_resources}\n- Lifecycle: {lifecycle}\n- First observable behavior: {first_behavior}\n\n## Verification\n\nRun `lenso check`, `lenso verify`, and then remove this Instance from a test App Definition and resolve the remainder. Replace every TODO before treating the card as design evidence.\n"
    )
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("resolve current directory")?
            .join(path))
    }
}

fn resolve_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        repo_root.join(path)
    }
}

fn display_relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}
fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_scaffold_uses_the_public_facade_and_generated_registration() {
        let root = tempfile::tempdir().unwrap();
        let options = ModuleCreateOptions {
            capability: None,
            dir: None,
            dry_run: false,
            module_id: "greeting".to_owned(),
            no_install: true,
            repo_root: Some(root.path().to_path_buf()),
            recipe: ModuleRecipe::Stateless,
            runtime: ModuleRuntime::Rust,
        };

        create_module(&options).unwrap();
        let project = root.path().join("greeting");
        let module = fs::read_to_string(project.join("src/lib.rs")).unwrap();
        assert!(module.contains("#[module]"));
        assert!(module.contains("#[provides(capability::Greeting)]"));
        assert!(!module.contains("NativeModuleFactory"));
        assert!(!module.contains("GreetingEndpoint"));

        let runner = fs::read_to_string(project.join("src/bin/lenso-module-dev.rs")).unwrap();
        assert!(runner.contains("with_linked_factories"));
        assert!(!runner.contains("with_factory"));
        assert!(project.join("capability/src/generated.rs").is_file());
    }
}
