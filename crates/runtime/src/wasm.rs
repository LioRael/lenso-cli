use std::path::Path;
use std::time::Duration;

use extism::{Manifest, PluginBuilder, UserData, ValType};
use lenso_cli::{Context, Diagnostic, DiagnosticCode, Error, Outcome};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A local Wasm export pinned by content digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmSpec {
    /// Path relative to the extension manifest directory.
    pub module: String,
    /// Exported Extism function name.
    pub function: String,
    /// Lowercase SHA-256 of the exact module bytes.
    pub sha256: String,
    /// Optional instruction budget, bounded by the Host to 1..=50 million units.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fuel: Option<u64>,
}

/// Maximum accepted ABI input and output, bounded before deserialization.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Maximum module size before compilation.
const MAX_MODULE_BYTES: u64 = 16 * 1024 * 1024;

impl WasmSpec {
    pub(crate) fn fuel_limit(&self) -> Result<u64, Error> {
        let fuel = self.fuel.unwrap_or(10_000_000);
        if !(1..=50_000_000).contains(&fuel) {
            return Err(Error::Invalid(
                "Wasm fuel must be between 1 and 50000000".into(),
            ));
        }
        Ok(fuel)
    }

    /// Invoke ABI v1 with no WASI and only explicitly granted host read/HTTPS capabilities.
    ///
    /// # Errors
    /// Rejects escaped paths, changed modules, resource exhaustion, or invalid output.
    pub async fn run(&self, base: &Path, ctx: Context) -> Result<Outcome, Error> {
        self.run_identified(base, ctx, serde_json::Value::Null)
            .await
    }

    pub(crate) async fn run_identified(
        &self,
        base: &Path,
        ctx: Context,
        identity: serde_json::Value,
    ) -> Result<Outcome, Error> {
        let module = base.join(&self.module).canonicalize()?;
        if !module.starts_with(base.canonicalize()?) {
            return Err(Error::Permission(
                "Wasm module escapes its extension directory".into(),
            ));
        }
        self.run_cached(
            &module,
            ctx,
            identity,
            Default::default(),
            Default::default(),
        )
        .await
    }

    pub(crate) async fn run_cached(
        &self,
        module: &Path,
        ctx: Context,
        identity: serde_json::Value,
        credentials: std::collections::BTreeMap<String, crate::Credential>,
        source_options: crate::LoadOptions,
    ) -> Result<Outcome, Error> {
        self.run_with_deadline(
            module,
            ctx,
            identity,
            credentials,
            source_options,
            Duration::from_secs(30),
        )
        .await
    }

    async fn run_with_deadline(
        &self,
        module: &Path,
        ctx: Context,
        identity: serde_json::Value,
        credentials: std::collections::BTreeMap<String, crate::Credential>,
        source_options: crate::LoadOptions,
        timeout: Duration,
    ) -> Result<Outcome, Error> {
        let fuel = self.fuel_limit()?;
        let mut file = std::fs::File::open(module)?;
        use std::io::Read;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MAX_MODULE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MODULE_BYTES {
            return Err(Error::Invalid("Wasm module exceeds 16 MiB".into()));
        }
        let digest = format!("{:x}", Sha256::digest(&bytes));
        if digest != self.sha256 {
            return Err(Diagnostic::new(DiagnosticCode::IntegrityMismatch)
                .with_details(serde_json::json!({"artifact":"wasm"}))
                .with_retryable(false)
                .with_recovery(
                    "restore_pinned",
                    "Restore the pinned bytes, or explicitly review and update the dependency.",
                )
                .into());
        }
        let input = serde_json::to_vec(&ctx)?;
        if input.len() > MAX_MESSAGE_BYTES {
            return Err(Error::Invalid("Wasm context exceeds 1 MiB".into()));
        }
        let function = self.function.clone();
        let module_name = self.module.clone();
        let (ready, started) = tokio::sync::oneshot::channel();
        let task = tokio::task::spawn_blocking(move || {
            let mut policy = crate::host::Host::new(&ctx)?;
            policy.source_options = source_options;
            if let Some(owner) = identity.get("id")
                && ctx.capabilities.get("host.credential_owner") == Some(owner)
            {
                policy.credentials = credentials;
            }
            let host = UserData::new(policy);
            let manifest = Manifest::new([extism::Wasm::data(bytes)])
                .with_memory_max(256) // 16 MiB of linear memory.
                .disallow_all_hosts();
            let mut plugin = PluginBuilder::new(manifest)
                .with_wasi(false)
                .with_fuel_limit(fuel)
                .with_function(
                    "source_resolve",
                    [ValType::I64],
                    [ValType::I64],
                    host.clone(),
                    crate::host::source_resolve,
                )
                .with_function(
                    "source_read",
                    [ValType::I64],
                    [ValType::I64],
                    host.clone(),
                    crate::host::source_read,
                )
                .with_function(
                    "read_file",
                    [ValType::I64],
                    [ValType::I64],
                    host.clone(),
                    crate::host::read_file,
                )
                .with_function(
                    "http_get",
                    [ValType::I64],
                    [ValType::I64],
                    host.clone(),
                    crate::host::http_get,
                )
                .with_function(
                    "http_request",
                    [ValType::I64],
                    [ValType::I64],
                    host.clone(),
                    crate::host::http_request,
                )
                .with_function(
                    "list_files",
                    [ValType::I64],
                    [ValType::I64],
                    host.clone(),
                    crate::host::list_files,
                )
                .build()
                .map_err(|_| Diagnostic::new(DiagnosticCode::WasmFailed))?;
            let _ = ready.send(plugin.cancel_handle());
            let output = match plugin.call::<&[u8], &[u8]>(&function, &input) {
                Ok(output) => output,
                Err(error) => {
                    if let Some(error) = error
                        .chain()
                        .find_map(|cause| cause.downcast_ref::<Error>())
                    {
                        return Err(error.diagnostic().into());
                    }
                    let code = if plugin.fuel_consumed().is_some_and(|used| used >= fuel) {
                        DiagnosticCode::WasmFuelExhausted
                    } else {
                        DiagnosticCode::WasmFailed
                    };
                    return Err(Diagnostic::new(code).with_retryable(false)
                        .with_recovery("inspect_extension", "Inspect the extension and its resource limits before retrying; no automatic rollback is provided.").into());
                }
            };
            if output.len() > MAX_MESSAGE_BYTES {
                return Err(Error::Invalid("Wasm response exceeds 1 MiB".into()));
            }
            let mut outcome: Outcome = serde_json::from_slice(output)
                .map_err(|_| Diagnostic::new(DiagnosticCode::InvalidResponse))?;
            if let Outcome::Complete { output } = &mut outcome {
                let mut evidence = host
                    .get()
                    .map_err(|e| Error::Operation(e.to_string()))?
                    .lock()
                    .map_err(|_| Error::Operation("host state poisoned".into()))?
                    .evidence
                    .clone();
                evidence.modules.push(serde_json::json!({"extension":identity,"module":module_name,"function":function,"sha256":digest}));
                if let Some(object) = output.as_object_mut() {
                    object.insert("_lenso".into(), serde_json::to_value(evidence)?);
                }
            }
            Ok(outcome)
        });
        await_call(task, started, timeout).await
    }
}

async fn await_call(
    mut task: tokio::task::JoinHandle<Result<Outcome, Error>>,
    started: tokio::sync::oneshot::Receiver<extism::CancelHandle>,
    timeout: Duration,
) -> Result<Outcome, Error> {
    let finish = |result: Result<Result<Outcome, Error>, tokio::task::JoinError>| {
        result.map_err(|_| Error::from(Diagnostic::new(DiagnosticCode::WasmFailed)))?
    };
    let Ok(cancel) = started.await else {
        return finish(task.await);
    };
    tokio::select! {
        biased;
        result = &mut task => finish(result),
        () = tokio::time::sleep(timeout) => {
            let cancelled = cancel.cancel().is_ok();
            let _ = task.await;
            Err(Diagnostic::new(DiagnosticCode::WasmTimeout)
                .with_details(serde_json::json!({"timeout_ms":timeout.as_millis(),"cancellation_requested":cancelled}))
                .with_retryable(false).with_recovery("inspect_extension", "Inspect the extension and completed effects before retrying after a timeout.").into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn actual_wasm_deadline_has_a_typed_error_without_parsing_trap_text() {
        let root = tempfile::tempdir().unwrap();
        let bytes =
            wat::parse_str("(module (func (export \"run\") (result i32) (loop br 0) i32.const 0))")
                .unwrap();
        let module = root.path().join("loop.wasm");
        std::fs::write(&module, &bytes).unwrap();
        let spec = WasmSpec {
            module: "loop.wasm".into(),
            function: "run".into(),
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            fuel: Some(50_000_000),
        };
        let ctx = Context {
            events: Default::default(),
            abi_version: 1,
            project: root.path().into(),
            input: serde_json::json!({}),
            answers: Default::default(),
            steps: Default::default(),
            hooks: Default::default(),
            capabilities: Default::default(),
        };
        let error = spec
            .run_with_deadline(
                &module,
                ctx,
                serde_json::Value::Null,
                Default::default(),
                Default::default(),
                Duration::from_millis(1),
            )
            .await
            .unwrap_err();
        assert_eq!(error.diagnostic().code(), DiagnosticCode::WasmTimeout);
        assert_eq!(error.diagnostic().details()["cancellation_requested"], true);
    }
}
