use lenso_cli::{Context, serde_json::json};
use lenso_runtime::WasmSpec;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn context() -> Context {
    Context {
        events: Default::default(),
        abi_version: 1,
        project: std::path::PathBuf::from("/"),
        input: json!({}),
        answers: BTreeMap::new(),
        steps: BTreeMap::new(),
        hooks: BTreeMap::new(),
        capabilities: BTreeMap::new(),
    }
}

fn wasm_with_string(imports: &str, instruction: &str, text: &[u8]) -> Vec<u8> {
    let stores = text
        .iter()
        .enumerate()
        .map(|(offset, byte)| {
            format!("local.get $p i64.const {offset} i64.add i32.const {byte} call $store")
        })
        .collect::<Vec<_>>()
        .join("\n");
    wat::parse_str(format!(
        r#"(module
        (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
        (import "extism:host/env" "store_u8" (func $store (param i64 i32)))
        {imports}
        (func (export "run") (result i32) (local $p i64)
            i64.const {} call $alloc local.set $p
            {stores} local.get $p {instruction} i32.const 1))"#,
        text.len()
    ))
    .unwrap()
}

#[tokio::test]
async fn wasm_host_denials_keep_typed_capabilities_and_guest_errors_do_not_leak_text() {
    for (bytes, expected) in [
        (wasm_with_string(r#"(import "extism:host/user" "read_file" (func $read (param i64) (result i64)))"#,
            "call $read drop", b"secret-path"), lenso_cli::DiagnosticCode::PermissionDenied),
        (wasm_with_string(r#"(import "extism:host/env" "error_set" (func $error (param i64)))"#,
            "call $error", b"Bearer secret-token https://user:password@example.test/?key=secret-value\nstack backtrace"), lenso_cli::DiagnosticCode::WasmFailed),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("plugin.wasm"),&bytes).unwrap();
        let spec = WasmSpec { module:"plugin.wasm".into(),function:"run".into(),sha256:format!("{:x}",Sha256::digest(&bytes)),fuel:None };
        let diagnostic = spec.run(root.path(),context()).await.unwrap_err().diagnostic();
        assert_eq!(diagnostic.code(),expected);
        let wire = serde_json::to_string(&diagnostic).unwrap();
        for secret in ["secret-path","secret-token","password","secret-value","backtrace"] {
            assert!(!wire.contains(secret),"leaked {secret}: {wire}");
        }
        if expected == lenso_cli::DiagnosticCode::PermissionDenied {
            assert_eq!(diagnostic.details()["capability"],"host.reads");
        }
    }
}

#[tokio::test]
async fn runaway_wasm_is_stopped_by_fuel_or_timeout() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let bytes =
        wat::parse_str("(module (func (export \"run\") (result i32) (loop br 0) i32.const 0))")?;
    std::fs::write(root.path().join("plugin.wasm"), &bytes)?;
    let spec = WasmSpec {
        fuel: None,
        module: "plugin.wasm".into(),
        function: "run".into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    let started = std::time::Instant::now();
    let error = spec.run(root.path(), context()).await.unwrap_err();
    assert!(
        error.diagnostic().code() == lenso_cli::DiagnosticCode::WasmFuelExhausted,
        "unexpected failure: {error}"
    );
    assert!(
        started.elapsed().as_secs() < 15,
        "runaway module must not hang the host"
    );
    Ok(())
}

#[tokio::test]
async fn malformed_response_and_changed_digest_are_errors() -> Result<(), Box<dyn std::error::Error>>
{
    let root = tempfile::tempdir()?;
    let bytes = wat::parse_str("(module (func (export \"run\") (result i32) i32.const 0))")?;
    std::fs::write(root.path().join("plugin.wasm"), &bytes)?;
    let mut spec = WasmSpec {
        fuel: None,
        module: "plugin.wasm".into(),
        function: "run".into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    assert!(
        spec.run(root.path(), context()).await.is_err(),
        "empty output must not become success"
    );
    spec.sha256 = "incorrect digest".into();
    assert!(
        spec.run(root.path(), context())
            .await
            .unwrap_err()
            .diagnostic()
            .code()
            == lenso_cli::DiagnosticCode::IntegrityMismatch
    );
    Ok(())
}

#[tokio::test]
async fn instruction_budget_is_host_bounded() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    for fuel in [0, 50_000_001] {
        let spec = WasmSpec {
            module: "absent.wasm".into(),
            function: "run".into(),
            sha256: String::new(),
            fuel: Some(fuel),
        };
        // Public path validation precedes compilation; use a file so budget validation is reached.
        std::fs::write(root.path().join("absent.wasm"), [])?;
        assert!(
            spec.run(root.path(), context())
                .await
                .unwrap_err()
                .to_string()
                .contains("fuel must")
        );
    }
    Ok(())
}
