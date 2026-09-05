use std::{
    any::Any,
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    process::Stdio,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, anyhow, bail};
use lenso_app_plan::{PluginInstancePlan, ResolvedAppPlan};
use lenso_kernel::{CancellationToken, ExecutionAdapter, InvocationContext, RuntimeFailure};
use lenso_plugin_bundle::{ImplementationPolicy, RuntimeAdmission, resolve_implementation};
use lenso_runtime_codec::{ArtifactCatalog, ArtifactHandle, JsonCapabilityCodec};
use lenso_wasm_component_adapter::{WasmComponentAdapter, WasmComponentLimits};

use crate::watch::SourceWatcher;

use super::{
    BuildProfile, BunPackage, CapabilityEndpointPlan, Command, DevImplementationArg,
    ExecutionClassId, Path, PluginCapability, PluginDescriptor, PluginDevArgs, ProjectRuntime,
    Value, VerifiedBundle, WASM_EXECUTION_CLASS, env, fs, materialize_bun, materialize_dev,
    one_capability, parse_descriptor, project_root, project_runtime, read_bun_package,
    read_bundle_manifest, read_package, resolve_dev_selection, run_bun_output,
};

pub(super) async fn run(args: PluginDevArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root.clone())?;
    if super::web_dev::is_web_plugin(&root)? {
        return super::web_dev::run(args).await;
    }
    if !args.watch {
        return dev_once(&args).await;
    }
    let mut watcher = SourceWatcher::new(&root)?;
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
            result = watcher.changed() => {
                result?;
                if let Err(error) = dev_once(&args).await {
                    eprintln!("Plugin rebuild failed: {error:#}");
                }
            }
        }
    }
}

async fn dev_once(args: &PluginDevArgs) -> anyhow::Result<()> {
    let root = project_root(args.repo_root.clone())?;
    if let Some(package) = read_bun_package(&root)? {
        if !matches!(
            args.implementation,
            DevImplementationArg::Auto | DevImplementationArg::All
        ) {
            bail!("Bun Plugin projects support `--implementation auto` or `all`");
        }
        return dev_bun(&root, &package, args);
    }
    let package = read_package(&root.join("Cargo.toml"))?;
    let declared_runtime = project_runtime(&package)?;
    let selection = resolve_dev_selection(declared_runtime, args.implementation)?;
    let dev_runtime = selection.invoke;
    let temporary = tempfile::tempdir().context("create Plugin dev directory")?;
    let output = temporary.path().join("dev.lenso-plugin");
    let verified = materialize_dev(
        &root,
        &output,
        &package,
        declared_runtime,
        selection.build,
        BuildProfile::Development,
    )?;
    let dev_class = if dev_runtime == ProjectRuntime::Process {
        "lenso.process@1"
    } else {
        WASM_EXECUTION_CLASS
    };
    let selected = resolve_implementation(
        &read_bundle_manifest(&output)?,
        &ImplementationPolicy {
            host_target: format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS),
            runtimes: vec![RuntimeAdmission {
                execution_class: ExecutionClassId::new(dev_class),
                runtime_profile: match dev_runtime {
                    ProjectRuntime::Process => "lenso.process@1",
                    ProjectRuntime::Wasm => "lenso.wasm-component@1",
                    ProjectRuntime::Multi | ProjectRuntime::Bun => {
                        unreachable!("development resolves to one invocation runtime")
                    }
                }
                .to_owned(),
            }],
        },
    )?;
    let artifact_path = output.join(&selected.artifact.path);
    let artifact_bytes = fs::read(&artifact_path)?;
    let descriptor = match dev_runtime {
        ProjectRuntime::Wasm => parse_descriptor(&artifact_bytes)?,
        ProjectRuntime::Process => read_process_descriptor(&artifact_path)?,
        ProjectRuntime::Multi | ProjectRuntime::Bun => {
            unreachable!("development resolves to one invocation runtime")
        }
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
    if dev_runtime == ProjectRuntime::Process {
        let response = invoke_dev_process(
            &artifact_path,
            &descriptor,
            capability,
            &operation,
            &request,
        )?;
        return print_dev_response(&verified, capability, &operation, &response, args.json);
    }
    let response = invoke_dev_wasm(
        &artifact_path,
        &selected.artifact.digest,
        &artifact_bytes,
        &verified.plugin_id,
        capability,
        &operation,
        request,
    )
    .await?;
    print_dev_response(&verified, capability, &operation, &response, args.json)
}

async fn invoke_dev_wasm(
    artifact_path: &Path,
    artifact_digest: &str,
    artifact_bytes: &[u8],
    plugin_id: &str,
    capability: &PluginCapability,
    operation: &str,
    request: Value,
) -> anyhow::Result<Value> {
    let artifact =
        ArtifactHandle::open(artifact_path, artifact_digest, artifact_bytes.len() as u64)
            .map_err(|error| runtime_error("open Plugin artifact", &error))?;
    let artifacts = ArtifactCatalog::new()
        .with_artifact("plugin", artifact)
        .map_err(|error| runtime_error("register Plugin artifact", &error))?;
    let plan = ResolvedAppPlan::new(
        vec![
            PluginInstancePlan::new("plugin", plugin_id)
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
    invoke_dev_adapter(&adapter, &plan, operation, request).await
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

pub(super) fn read_process_descriptor(executable: &Path) -> anyhow::Result<PluginDescriptor> {
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

fn dev_bun(root: &Path, package: &BunPackage, args: &PluginDevArgs) -> anyhow::Result<()> {
    let temporary = tempfile::tempdir().context("create Bun Plugin dev directory")?;
    let (verified, descriptor) = materialize_bun(
        root,
        &temporary.path().join("dev.lenso-plugin"),
        package,
        BuildProfile::Development,
    )?;
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

fn runtime_error(action: &str, error: &RuntimeFailure) -> anyhow::Error {
    anyhow!("{action}: {error:?}")
}

#[derive(Debug)]
struct DynamicJsonCodec {
    capability_id: &'static str,
    descriptor_version: &'static str,
    request_operations: &'static [&'static str],
}

impl DynamicJsonCodec {
    fn new(capability: &PluginCapability) -> Self {
        Self {
            capability_id: intern_string(&capability.capability_id),
            descriptor_version: intern_string(&capability.descriptor_version),
            request_operations: intern_operations(&capability.request_operations),
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

fn intern_string(value: &str) -> &'static str {
    static VALUES: OnceLock<Mutex<HashMap<String, &'static str>>> = OnceLock::new();
    let mut values = VALUES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("Plugin dev string interner lock");
    if let Some(value) = values.get(value) {
        return value;
    }
    let interned = Box::leak(value.to_owned().into_boxed_str());
    values.insert(value.to_owned(), interned);
    interned
}

fn intern_operations(operations: &[String]) -> &'static [&'static str] {
    type OperationSet = HashMap<Vec<String>, &'static [&'static str]>;
    static VALUES: OnceLock<Mutex<OperationSet>> = OnceLock::new();
    let mut values = VALUES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .expect("Plugin dev operation interner lock");
    if let Some(value) = values.get(operations) {
        return value;
    }
    let interned = Box::leak(
        operations
            .iter()
            .map(|operation| intern_string(operation))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    values.insert(operations.to_vec(), interned);
    interned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_wasm_rebuilds_reuse_interned_contract_storage() {
        let capability = PluginCapability {
            capability_id: "example.echo@1".to_owned(),
            descriptor_version: "1".to_owned(),
            request_operations: vec!["execute".to_owned()],
        };
        let first = DynamicJsonCodec::new(&capability);
        let second = DynamicJsonCodec::new(&capability);

        assert!(std::ptr::eq(first.capability_id(), second.capability_id()));
        assert!(std::ptr::eq(
            first.descriptor_version(),
            second.descriptor_version()
        ));
        assert!(std::ptr::eq(
            first.request_operations(),
            second.request_operations()
        ));
    }
}
