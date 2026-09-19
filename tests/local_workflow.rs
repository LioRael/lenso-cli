use std::{fs, process::Command};

#[test]
fn create_is_configuration_free_and_preserves_existing_projects() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("project");
    let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(["app", "create"])
        .arg(&destination)
        .args(["--runtime", "empty"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(destination.join("app").is_dir());
    assert!(destination.join("plugins").is_dir());
    assert!(!destination.join("lenso.toml").exists());
    fs::write(destination.join("README.md"), "owner content").unwrap();
    let retry = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(["app", "create"])
        .arg(&destination)
        .args(["--runtime", "empty"])
        .output()
        .unwrap();
    assert!(!retry.status.success());
    assert_eq!(
        fs::read_to_string(destination.join("README.md")).unwrap(),
        "owner content"
    );
    let discovery = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(["app", "discover", "--root"])
        .arg(&destination)
        .arg("--json")
        .output()
        .unwrap();
    assert!(discovery.status.success());
    let report: serde_json::Value = serde_json::from_slice(&discovery.stdout).unwrap();
    assert_eq!(report["candidates"].as_array().unwrap().len(), 0);
}

#[test]
fn local_build_does_not_change_explicit_host_flag_contract() {
    for args in [
        ["app", "build", "--source", "host.ts"],
        ["app", "build", "--target", "aarch64-apple-darwin"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
#[ignore = "requires Cargo registry access and Bun; compiles a real generated Host"]
fn clean_room_local_host_invokes_native_to_bun_and_runs_without_source_or_toolchains() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let distribution = temp.path().join("dist");
    let cli = env!("CARGO_BIN_EXE_lenso");
    let run = |command: &mut Command| {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    run(Command::new(cli)
        .args(["app", "create"])
        .arg(&source)
        .args(["--runtime", "bun", "--no-install"]));
    run(Command::new("bun")
        .arg("install")
        .current_dir(source.join("app/local.starter")));
    // A Bun-only App uses the shipped runtime. Cargo must never be spawned.
    let tools = temp.path().join("build-tools");
    fs::create_dir_all(&tools).unwrap();
    #[cfg(unix)]
    for name in ["cargo", "rustc"] {
        use std::os::unix::fs::PermissionsExt;
        let path = tools.join(name);
        fs::write(
            &path,
            "#!/bin/sh\necho unexpected Rust toolchain invocation >&2\nexit 99\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::join_paths(std::iter::once(tools.clone()).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let portable = temp.path().join("portable");
    run(Command::new(cli)
        .args(["app", "build", "--root"])
        .arg(&source)
        .arg("--out")
        .arg(&portable)
        .env("PATH", path));
    assert_eq!(
        fs::read_to_string(portable.join(".lenso/host-mode"))
            .unwrap()
            .trim(),
        "portable"
    );
    run(Command::new(cli)
        .args(["app", "start", "--from"])
        .arg(&portable)
        .arg("--check")
        .env_clear()
        .env("PATH", temp.path().join("no-toolchains")));
    let native = source.join("app/local.consumer");
    fs::create_dir_all(native.join("src")).unwrap();
    fs::write(
        native.join("Cargo.toml"),
        include_str!("fixtures/local-native-consumer/Cargo.toml"),
    )
    .unwrap();
    fs::write(
        native.join("src/lib.rs"),
        include_str!("fixtures/local-native-consumer/src/lib.rs"),
    )
    .unwrap();
    run(Command::new(cli)
        .args(["app", "build", "--root"])
        .arg(&source)
        .arg("--out")
        .arg(&distribution));
    let lock: serde_json::Value = serde_json::from_slice(
        &fs::read(distribution.join(".lenso/distribution.lock.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(lock["schema"], "lenso.local-host-distribution.v1");
    fs::rename(&source, temp.path().join("unavailable-source")).unwrap();
    let output = run(Command::new(cli)
        .args(["app", "start", "--from"])
        .arg(&distribution)
        .arg("--check")
        .env_clear()
        .env("PATH", temp.path().join("no-toolchains"))
        .current_dir(temp.path()));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("NATIVE_TO_BUN_OK"), "{stderr}");
    assert!(stderr.contains("NATIVE_DEACTIVATED"), "{stderr}");
    assert!(stderr.contains("Local App stopped cleanly"), "{stderr}");
    let authority = distribution.join(".lenso/host-build.json");
    let mut bytes = fs::read(&authority).unwrap();
    bytes.push(b' ');
    fs::write(authority, bytes).unwrap();
    let tampered = Command::new(cli)
        .args(["app", "start", "--from"])
        .arg(&distribution)
        .arg("--check")
        .output()
        .unwrap();
    assert!(!tampered.status.success());
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("runtime file changed"));
    assert!(!String::from_utf8_lossy(&tampered.stderr).contains("NATIVE_TO_BUN_OK"));
}

#[test]
fn schema_contract_scaffold_builds_without_a_rust_authoring_package() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("app");
    let cli = env!("CARGO_BIN_EXE_lenso");
    for args in [
        vec![
            "app",
            "create",
            root.to_str().unwrap(),
            "--runtime",
            "empty",
        ],
        vec![
            "app",
            "contract",
            "new",
            "example.text",
            "--root",
            root.to_str().unwrap(),
        ],
    ] {
        let output = Command::new(cli).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = Command::new(cli)
        .args(["app", "build", "--root"])
        .arg(&root)
        .env_clear()
        .env("PATH", temporary.path().join("no-tools"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        root.join("contracts/example.text/generated/contract.ts")
            .is_file()
    );
    assert!(!root.join("contracts/example.text/Cargo.toml").exists());
    assert!(!root.join("lenso.toml").exists());
}

#[test]
#[ignore = "requires Bun and registry access; builds generated Capability providers and dependencies"]
fn clean_room_bun_generated_capabilities_build_and_start_without_cargo() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("app");
    let cli = env!("CARGO_BIN_EXE_lenso");
    let run = |command: &mut Command| {
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        output
    };
    run(Command::new(cli)
        .args(["app", "create"])
        .arg(&root)
        .args(["--runtime", "empty"]));
    run(Command::new(cli)
        .args(["app", "contract", "new", "example.text", "--root"])
        .arg(&root));
    for (name, source) in [
        (
            "provider",
            "import { definePlugin } from '@lenso/bun-plugin'; import { Text, type TextProvider } from './generated.ts'; export default definePlugin({ provides: [Text], create(): TextProvider { return { async execute(_context, request) { return { ok: true, value: request }; } }; } });",
        ),
        (
            "consumer",
            "import { definePlugin } from '@lenso/bun-plugin'; import { Text } from './generated.ts'; export default definePlugin({ provides: [], dependencies: { text: Text.required() }, create({ dependencies }) { return { text: dependencies.text }; } });",
        ),
    ] {
        let package = root.join("app").join(name);
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("package.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "name":format!("example-{name}"),"version":"1.0.0","private":true,"type":"module",
            "scripts":{"check":"tsc --noEmit"},
            "dependencies":{"@lenso/bun-plugin":"0.4.1","@lenso/contract-runtime":"0.3.0"},
            "devDependencies":{"typescript":"7.0.2","@types/bun":"1.4.0"},
            "lenso":{"pluginId":format!("example.{name}"),"runtime":"bun","rootSlot":"tools","entry":"plugin.ts",
                "contract":{"descriptor":"../../contracts/example.text/capability.json","projection":"typescript","output":"generated.ts"}}
        })).unwrap()).unwrap();
        fs::create_dir_all(package.join("src")).unwrap();
        fs::write(
            package.join("src/plugin.ts"),
            source.replace("'./generated.ts'", "'../generated.ts'"),
        )
        .unwrap();
        fs::write(package.join("tsconfig.json"), r#"{"compilerOptions":{"strict":true,"noEmit":true,"module":"Preserve","moduleResolution":"bundler","allowImportingTsExtensions":true,"types":["bun"]},"include":["src/**/*.ts"]}"#).unwrap();
        run(Command::new("bun").arg("install").current_dir(&package));
    }
    let tools = temporary.path().join("tools");
    fs::create_dir(&tools).unwrap();
    #[cfg(unix)]
    for name in ["cargo", "rustc"] {
        use std::os::unix::fs::PermissionsExt;
        let path = tools.join(name);
        fs::write(&path, "#!/bin/sh\nexit 99\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = std::env::join_paths(
        std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    run(Command::new(cli)
        .args(["app", "build", "--root"])
        .arg(&root)
        .env("PATH", path));
    run(Command::new(cli)
        .args(["app", "start", "--from"])
        .arg(root.join("dist"))
        .arg("--check")
        .env_clear()
        .env("PATH", temporary.path().join("no-tools")));
}
