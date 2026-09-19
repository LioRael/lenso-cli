use std::{fs, process::Command};
#[test]
fn engine_cli_reads_documents_without_app_or_toolchain() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("guide.md"), "# Independent engine").unwrap();
    fs::write(root.path().join("plugin.rs"), "not valid Rust").unwrap();
    for mode in ["inspect", "run"] {
        let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
            .env("PATH", "")
            .args(["engine", mode, "--markdown", "--source"])
            .arg(root.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        if mode == "inspect" {
            assert_eq!(value["steps"].as_array().unwrap().len(), 1);
        } else {
            assert_eq!(
                value["outputs"]["markdown/guide.md"]["guide.md"]["value"]["text"],
                "# Independent engine"
            );
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(["engine", "run", "--source"])
        .arg(root.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["outputs"].as_object().unwrap().is_empty());
}
