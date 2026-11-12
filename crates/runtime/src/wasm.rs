use std::path::Path;
use std::time::Duration;

use extism::{Manifest, PluginBuilder, UserData, ValType};
use lenso_cli::{Context, Error, Outcome};
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
}

/// Maximum accepted ABI input and output, bounded before deserialization.
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
/// Maximum module size before compilation.
const MAX_MODULE_BYTES: u64 = 16 * 1024 * 1024;

impl WasmSpec {
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
            return Err(Error::Permission(
                "Wasm module digest changed; restore its pinned bytes or run lenso update".into(),
            ));
        }
        let input = serde_json::to_vec(&ctx)?;
        if input.len() > MAX_MESSAGE_BYTES {
            return Err(Error::Invalid("Wasm context exceeds 1 MiB".into()));
        }
        let function = self.function.clone();
        let module_name = self.module.clone();
        tokio::task::spawn_blocking(move || {
            let mut policy = crate::host::Host::new(&ctx)?;
            policy.source_options=source_options;
            if let Some(owner) = identity.get("id")
                && ctx.capabilities.get("host.credential_owner") == Some(owner) {
                policy.credentials = credentials;
            }
            let host = UserData::new(policy);
            let manifest = Manifest::new([extism::Wasm::data(bytes)])
                .with_memory_max(256) // 16 MiB of linear memory.
                .with_timeout(Duration::from_secs(30))
                .disallow_all_hosts();
            let mut plugin = PluginBuilder::new(manifest)
                .with_wasi(false)
                .with_fuel_limit(10_000_000)
                .with_function("source_resolve", [ValType::I64], [ValType::I64], host.clone(), crate::host::source_resolve)
                .with_function("source_read", [ValType::I64], [ValType::I64], host.clone(), crate::host::source_read)
                .with_function("read_file", [ValType::I64], [ValType::I64], host.clone(), crate::host::read_file)
                .with_function("http_get", [ValType::I64], [ValType::I64], host.clone(), crate::host::http_get)
                .with_function("http_request", [ValType::I64], [ValType::I64], host.clone(), crate::host::http_request)
                .with_function("list_files", [ValType::I64], [ValType::I64], host.clone(), crate::host::list_files)
                .build()
                .map_err(|e| Error::Operation(format!("Wasm initialization failed: {e:#}")))?;
            let output = plugin
                .call::<&[u8], &[u8]>(&function, &input)
                .map_err(|e| Error::Operation(format!("Wasm call failed: {e:#}")))?;
            if output.len() > MAX_MESSAGE_BYTES {
                return Err(Error::Invalid("Wasm response exceeds 1 MiB".into()));
            }
            let mut outcome: Outcome = serde_json::from_slice(output)?;
            if let Outcome::Complete { output } = &mut outcome {
                let mut evidence = host.get().map_err(|e| Error::Operation(e.to_string()))?.lock().map_err(|_| Error::Operation("host state poisoned".into()))?.evidence.clone();
                evidence.modules.push(serde_json::json!({"extension":identity,"module":module_name,"function":function,"sha256":digest}));
                if let Some(object) = output.as_object_mut() { object.insert("_lenso".into(), serde_json::to_value(evidence)?); }
            }
            Ok(outcome)
        })
        .await
        .map_err(|e| Error::Operation(format!("Wasm task failed: {e}")))?
    }
}
