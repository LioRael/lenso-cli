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
        error.to_string().contains("Wasm call failed"),
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
            .to_string()
            .contains("digest changed")
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
