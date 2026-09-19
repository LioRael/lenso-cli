use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
const CLI: &str = env!("CARGO_BIN_EXE_lenso");
fn run(command: &mut Command) -> Output {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn write(root: &Path, path: &str, contents: &str) {
    let file = root.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, contents).unwrap();
}
fn invoke(root: &Path, args: &[&str]) -> Output {
    Command::new(CLI)
        .args(["app", "start", "--from"])
        .arg(root.join("dist"))
        .arg("--")
        .args(args)
        .env_clear()
        .env("PATH", root.join("no-tools"))
        .output()
        .unwrap()
}

#[test]
#[ignore = "requires Bun and registry access; exercises source-free CLI streams without Cargo"]
fn clean_room_cli_conventions_need_no_rust_and_keep_inactive_packages_unresolved() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    let tools = temp.path().join("tools");
    fs::create_dir(&tools).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["cargo", "rustc"] {
            fs::write(
                tools.join(name),
                "#!/bin/sh\nprintf unexpected > \"$LENSO_TEST_CARGO_MARKER\"\nexit 99\n",
            )
            .unwrap();
            fs::set_permissions(tools.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    let path = std::env::join_paths(
        std::iter::once(tools).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    run(Command::new(CLI)
        .args(["app", "create"])
        .arg(&root)
        .arg("--cli")
        .env("PATH", &path)
        .env("LENSO_TEST_CARGO_MARKER", temp.path().join("cargo-ran")));
    write(
        &root,
        "app/status/cli.ts",
        "import { command } from '@lenso/cli'; export default command({description:'Bare entry',run({output}) { output.text('bare entry works'); }});\n",
    );
    write(
        &root,
        "app/fail/cli.ts",
        "import { command } from '@lenso/cli'; export default command({description:'Failure',run() { throw new Error('intentional failure'); }});\n",
    );
    write(
        &root,
        "app/local.hello/tui/tui.rs",
        "compile_error!(\"must not compile\");",
    );
    write(
        &root,
        "app/local.hello/tui/Cargo.toml",
        "not a valid Cargo manifest",
    );
    let manifest = root.join("app/local.hello/package.json");
    let mut metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    metadata["lenso"]["surfaces"] =
        serde_json::json!([{"entry":"cli.ts"},{"entry":"tui/tui.rs","project":"tui"}]);
    fs::write(manifest, serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
    let inspect = run(Command::new(CLI)
        .args(["app", "inspect", "--json", "--root"])
        .arg(&root));
    let plan: serde_json::Value = serde_json::from_slice(&inspect.stdout).unwrap();
    assert!(
        plan["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["reason"] == "support_not_adopted")
    );
    run(Command::new(CLI)
        .args(["app", "build", "--root"])
        .arg(&root)
        .env("PATH", path)
        .env("LENSO_TEST_CARGO_MARKER", temp.path().join("cargo-ran")));
    assert!(!temp.path().join("cargo-ran").exists());
    fs::remove_dir_all(root.join("app")).unwrap();
    for (args, expected) in [
        (vec!["hello", "--name", "Ada"], "Hello, Ada!"),
        (vec!["status"], "bare entry works"),
        (vec!["hello", "--help"], "Say hello"),
    ] {
        let output = invoke(&root, &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
        assert!(String::from_utf8_lossy(&output.stderr).contains("stopped cleanly"));
    }
    for args in [vec!["hello", "--unknown"], vec!["fail"]] {
        let output = invoke(&root, &args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("stopped cleanly"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
#[ignore = "requires Cargo and Bun; proves generated Rust and TS entries share the terminal contracts"]
#[expect(
    clippy::too_many_lines,
    reason = "one source-free mixed-language lifecycle scenario"
)]
fn clean_room_mixed_cli_conventions_share_typed_contracts_and_run_offline() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    run(Command::new(CLI)
        .args(["app", "create"])
        .arg(&root)
        .arg("--cli"));
    for package in ["rust-macros", "rust-sdk"] {
        run(Command::new("cargo")
            .arg("test")
            .arg("--manifest-path")
            .arg(
                root.join("app/lenso-terminal-cli")
                    .join(package)
                    .join("Cargo.toml"),
            ));
    }
    write(
        &root,
        "app/typed/cli.rs",
        r#"
use lenso_cli_support::{command, CommandContext};
/// Typed Rust progress
#[command(name = "typed")]
async fn typed(#[arg(default = "2")] count: u32, loud: bool, #[context] output: CommandContext) -> anyhow::Result<String> {
    anyhow::ensure!(count > 0, "count must be positive");
    output.text("started");
    if count == 99 { std::future::pending::<()>().await; }
    Ok(if loud { format!("COUNT={count}") } else { format!("count={count}") })
}
"#,
    );
    run(Command::new(CLI)
        .args([
            "app",
            "plugin",
            "new",
            "example.rust",
            "--language",
            "rust",
            "--root",
        ])
        .arg(&root));
    run(Command::new(CLI)
        .args(["app", "build", "--root"])
        .arg(&root));
    fs::remove_dir_all(root.join("app")).unwrap();
    for name in ["hello", "hello-rust"] {
        let output = invoke(&root, &[name, "--name", "mixed"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("Hello, mixed!"));
    }
    let output = invoke(&root, &["typed", "--count", "3", "--loud"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("COUNT=3"));
    for count in ["invalid", "0"] {
        let output = invoke(&root, &["typed", "--count", count]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("stopped cleanly"));
    }
    #[cfg(unix)]
    {
        use std::{
            io::{BufRead, BufReader},
            process::Stdio,
        };
        let mut child = Command::new(CLI)
            .args(["app", "start", "--from"])
            .arg(root.join("dist"))
            .args(["--", "typed", "--count", "99"])
            .env_clear()
            .env("PATH", root.join("no-tools"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });
        let started = rx.recv_timeout(std::time::Duration::from_secs(20));
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(i32::try_from(child.id()).unwrap()),
            nix::sys::signal::Signal::SIGTERM,
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() > deadline {
                child.kill().unwrap();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        let output = child.wait_with_output().unwrap();
        assert_eq!(started.unwrap().trim(), "started");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("stopped cleanly"));
    }
}

#[test]
#[ignore = "requires Bun; proves a third-party filename uses the generic compiler protocol"]
fn clean_room_custom_convention_compiler_is_only_executed_by_build() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    run(Command::new(CLI)
        .args(["app", "create"])
        .arg(&root)
        .args(["--runtime", "empty"]));
    let support = temp.path().join("support");
    write(
        &support,
        "package.json",
        r#"{"name":"example.audit-support","version":"1.0.0","type":"module","scripts":{"check":"tsc --noEmit"},"devDependencies":{"typescript":"7.0.2","@types/bun":"1.4.0"},"dependencies":{"@lenso/bun-plugin":"0.4.1"},"lenso":{"pluginId":"example.audit-support","runtime":"bun","rootSlot":"tools","source":"plugin.ts","conventions":[{"id":"example.audit","entries":["audit.ts"],"compiler":{"program":"bun","args":["compile.mjs"]}}]}}"#,
    );
    write(
        &support,
        "plugin.ts",
        "import { definePlugin } from '@lenso/bun-plugin'; export default definePlugin({provides:[],create(){return {};}});",
    );
    write(
        &support,
        "compile.mjs",
        r"import fs from 'node:fs';
const request = JSON.parse(fs.readFileSync(0, 'utf8'));
if (request.schema !== 'lenso.convention-compile.v1') throw new Error('protocol');
fs.writeFileSync('compiler-ran', request.entry);
fs.writeFileSync(request.output + '/package.json', JSON.stringify({name:request.plugin_id,version:request.release_version,type:'module',scripts:{check:'tsc --noEmit'},devDependencies:{typescript:'7.0.2','@types/bun':'1.4.0'},dependencies:{'@lenso/bun-plugin':'0.4.1'},lenso:{pluginId:request.plugin_id,runtime:'bun',rootSlot:'tools',source:'plugin.ts'}}));
fs.writeFileSync(request.output + '/plugin.ts', fs.readFileSync('plugin.ts'));
fs.copyFileSync('tsconfig.json', request.output + '/tsconfig.json');
console.log(JSON.stringify({schema:'lenso.convention-compiled.v1'}));
",
    );
    write(
        &support,
        "tsconfig.json",
        r#"{"compilerOptions":{"strict":true,"noEmit":true,"module":"Preserve","moduleResolution":"bundler","types":["bun"]},"include":["plugin.ts"]}"#,
    );
    // Marker lives in an ignored output directory, not among compiler source inputs.
    let script = fs::read_to_string(support.join("compile.mjs"))
        .unwrap()
        .replace(
            "fs.writeFileSync('compiler-ran'",
            "fs.mkdirSync('dist', {recursive:true}); fs.writeFileSync('dist/compiler-ran'",
        );
    fs::write(support.join("compile.mjs"), script).unwrap();
    write(
        &root,
        "app/reports/audit.ts",
        "// Owned by third-party support, not the CLI builtin.\n",
    );
    run(Command::new(CLI)
        .args(["app", "add"])
        .arg(&support)
        .arg("--root")
        .arg(&root));
    let output = run(Command::new(CLI)
        .args(["app", "inspect", "--json", "--root"])
        .arg(&root));
    assert!(String::from_utf8_lossy(&output.stdout).contains("example.audit"));
    assert!(!support.join("dist/compiler-ran").exists());
    run(Command::new(CLI)
        .args(["app", "build", "--root"])
        .arg(&root));
    assert!(support.join("dist/compiler-ran").exists());
    let provenance = fs::read_to_string(root.join("dist/.lenso/conventions.json")).unwrap();
    assert!(provenance.contains("example.audit"));
    assert!(provenance.contains("surface-"));
}
