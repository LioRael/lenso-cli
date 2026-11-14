use serde_json::{Value, json};
use std::fs;
use std::process::{Command, Output};

fn write_config(root: &std::path::Path, mut installation: Value) {
    installation.as_object_mut().unwrap().remove("manifest");
    installation["id"] = json!("example");
    installation["source"] = json!("lenso/extension.json");
    let project: lenso_runtime::Project =
        serde_json::from_value(json!({"cli":{"schema_version":1,"extensions":[installation]}}))
            .unwrap();
    fs::write(
        root.join("lenso.toml"),
        toml::to_string_pretty(&project).unwrap(),
    )
    .unwrap();
}

fn setup(extension: Value, installation: Value) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("lenso")).unwrap();
    fs::write(
        root.path().join("lenso/extension.json"),
        serde_json::to_vec(&extension).unwrap(),
    )
    .unwrap();
    fs::write(root.path().join("lenso/plugin.wasm"), b"\0asm\x01\0\0\0").unwrap();
    write_config(root.path(), installation);
    root
}

fn extension() -> Value {
    json!({"schema_version":1,"id":"example","version":"0.1.0",
        "handlers":{"example/step":{"type":"wasm","module":"plugin.wasm","function":"run","sha256":"auto"}},
        "workflows":[{"id":"example/work","steps":[{"id":"work","handler":"example/step"}]}],
        "commands":[{"path":["demo"],"workflow":"example/work","about":"Example command","provider":"example"}]})
}

fn cli(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lenso"))
        .arg("--project")
        .arg(root)
        .args(args)
        .env("LENSO_CACHE_DIR", root.join("cache"))
        .output()
        .unwrap()
}

fn process_fixture() -> tempfile::TempDir {
    let mut ext = extension();
    let process =
        json!({"type":"process","program":env!("CARGO_BIN_EXE_lenso"),"args":["--version"]});
    ext["handlers"]["example/step"] = process;
    setup(
        ext,
        json!({"processes":[{"program":env!("CARGO_BIN_EXE_lenso"),"args":["--version"]}]}),
    )
}

#[test]
fn compact_results_preserve_full_outputs_in_the_same_invocation_report() {
    let root = setup(
        extension(),
        json!({"options":{"template":"native source line\n".repeat(4096)}}),
    );
    let full = cli(root.path(), &["config", "--json"]);
    assert!(full.status.success());
    let full_value: Value = serde_json::from_slice(&full.stdout).unwrap();
    let path = root.path().join("receipt.json");
    let summary = cli(
        root.path(),
        &[
            "config",
            "--json",
            "--summary",
            "--report",
            path.to_str().unwrap(),
        ],
    );
    assert!(
        summary.status.success(),
        "{}",
        String::from_utf8_lossy(&summary.stdout)
    );
    let value: Value = serde_json::from_slice(&summary.stdout).unwrap();
    assert_eq!(value["type"], "result");
    assert_eq!(value["status"], "complete");
    assert_eq!(value["mode"], "summary");
    assert!(value.get("outputs").is_none());
    assert_eq!(value["execution"]["verification_status"], "not_declared");
    assert!(summary.stdout.len() < full.stdout.len());
    let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved["outputs"], full_value["outputs"]);
    assert_eq!(saved["execution"]["id"], value["execution"]["id"]);
    assert_eq!(saved["report"], json!(path.canonicalize().unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let missing = cli(root.path(), &["demo", "--summary"]);
    assert_eq!(missing.status.code(), Some(2), "summary requires JSON mode");
}

#[test]
fn parser_and_input_diagnostics_never_echo_secret_values_even_in_debug_mode() {
    let root = process_fixture();
    for args in [
        vec![
            "demo",
            "--input",
            "{\"token\":\"secret-token\", invalid}",
            "--json",
            "--debug",
        ],
        vec![
            "demo",
            "--color",
            "https://user:password@example.test/?key=secret-token",
            "--json",
            "--debug",
        ],
    ] {
        let output = cli(root.path(), &args);
        assert!(!output.status.success());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["code"], "invalid_input");
        for secret in ["secret-token", "password"] {
            assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
            assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
        }
    }
}

#[cfg(unix)]
#[test]
fn reports_refuse_overwrites_before_handlers_and_keep_process_failure_exit_codes() {
    let command =
        json!({"type":"process","program":"/bin/sh","args":["-c","printf ran > marker; exit 7"]});
    let mut definition = extension();
    definition["handlers"]["example/step"] = command;
    let root = setup(
        definition,
        json!({"processes":[{"program":"/bin/sh","args":["-c","printf ran > marker; exit 7"]}]}),
    );
    let existing = root.path().join("existing.json");
    fs::write(&existing, "preserve me").unwrap();
    let result = cli(
        root.path(),
        &[
            "demo",
            "--json",
            "--summary",
            "--report",
            existing.to_str().unwrap(),
        ],
    );
    assert_eq!(result.status.code(), Some(1));
    assert!(!root.path().join("marker").exists());
    assert_eq!(fs::read_to_string(existing).unwrap(), "preserve me");
    let path = root.path().join("failed.json");
    let result = cli(
        root.path(),
        &[
            "demo",
            "--json",
            "--summary",
            "--report",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(result.status.code(), Some(7));
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    let saved: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(value["code"], "process_failed");
    assert_eq!(value["details"]["exit_code"], 7);
    assert_eq!(value["execution"]["id"], saved["execution"]["id"]);
    assert_eq!(value["execution"]["status"], "failed");
    assert_eq!(saved["outputs"], json!({}));
    assert_eq!(
        fs::read_to_string(root.path().join("marker")).unwrap(),
        "ran"
    );
}

#[test]
fn machine_help_uses_read_only_discovery_and_reports_real_project_commands() {
    let mut definition = extension();
    definition["contracts"] = json!({"example/work":{
        "effects":[], "capabilities":["host.reads"]
    }});
    let root = setup(definition, json!({"aliases":{"demo":["tool","check"]}}));
    let output = cli(
        root.path(),
        &["help", "tool", "check", "--json", "--offline"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let catalog: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(catalog["type"], "command_catalog");
    assert_eq!(catalog["commands"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["commands"][0]["path"], json!(["tool", "check"]));
    assert_eq!(catalog["commands"][0]["provider_version"], "0.1.0");
    assert_eq!(catalog["commands"][0]["contract"]["effects"], json!([]));
    assert!(
        !root.path().join("lenso.lock").exists(),
        "help cannot create an execution lock"
    );
    assert!(
        !root.path().join("cache/v1/blobs").exists(),
        "help cannot hydrate Wasm"
    );
    assert_eq!(
        cli(root.path(), &["help", "ops", "--json"]).status.code(),
        Some(1)
    );
    let human = cli(root.path(), &["help", "tool", "check"]);
    assert!(human.status.success());
    assert!(String::from_utf8_lossy(&human.stdout).contains("Example command"));
    let empty = tempfile::tempdir().unwrap();
    let output = cli(empty.path(), &["help", "--json"]);
    assert!(output.status.success());
    let catalog: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !catalog["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["provider"] == "example")
    );
}

#[test]
fn human_summary_and_color_policy_do_not_change_json_results() {
    let root = process_fixture();
    let human = cli(root.path(), &["demo", "--color", "never"]);
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(human.status.success());
    assert!(text.contains("Completed example/work"));
    assert!(!text.contains("\"outputs\"") && !text.contains("_lenso") && !text.contains('\u{1b}'));
    let colored = cli(root.path(), &["demo", "--color", "always"]);
    assert!(
        colored.stdout.contains(&0x1b),
        "explicit color should work even through a pipe"
    );
    let json_output = cli(root.path(), &["demo", "--json", "--color", "always"]);
    assert!(!json_output.stdout.contains(&0x1b));
    let result: Value = serde_json::from_slice(&json_output.stdout).unwrap();
    assert_eq!(result["type"], "result");
    assert_eq!(result["status"], "complete");
    assert_eq!(result["outputs"]["work"]["exit_code"], 0);
    assert!(
        !String::from_utf8_lossy(&json_output.stderr).contains("✓"),
        "JSON mode must suppress terminal progress"
    );
    let no_color = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .arg("--project")
        .arg(root.path())
        .env("LENSO_CACHE_DIR", root.path().join("cache"))
        .arg("demo")
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(!no_color.stdout.contains(&0x1b) && !no_color.stderr.contains(&0x1b));
    let debug = cli(root.path(), &["demo", "--debug", "--color", "never"]);
    assert!(String::from_utf8_lossy(&debug.stderr).contains("\"outputs\""));
    let invalid = cli(root.path(), &["demo", "--json", "--color", "invalid"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&invalid.stdout).unwrap()["type"],
        "diagnostic"
    );
}

#[test]
fn implicit_project_discovery_walks_up_from_subdirectories() {
    let root = process_fixture();
    let locked = cli(root.path(), &["demo", "--locked", "--json"]);
    assert_eq!(locked.status.code(), Some(1));
    assert!(!root.path().join("lenso.lock").exists());
    fs::create_dir_all(root.path().join("src/nested")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .current_dir(root.path().join("src/nested"))
        .env("LENSO_CACHE_DIR", root.path().join("cache"))
        .args(["demo", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["status"],
        "complete"
    );
}

#[test]
fn discovery_help_does_not_load_wasm_and_disabled_commands_disappear() {
    let root = setup(extension(), json!({"manifest":".lenso/extension.json"}));
    let output = cli(root.path(), &["--help"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Example command [example]"));
    assert!(
        !root.path().join("lenso.lock").exists(),
        "help must not create a lock"
    );
    assert!(
        !root.path().join("cache").exists(),
        "local help must not create cache"
    );
    let output = cli(root.path(), &["demo", "--json"]);
    assert!(
        !output.status.success(),
        "invoking a missing module must fail"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["type"],
        "diagnostic"
    );
    write_config(root.path(), json!({"disabled_commands":["demo"]}));
    let output = cli(root.path(), &["--help"]);
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Example command"));
}

#[test]
fn duplicate_aliases_report_both_providers_and_schema_errors_are_structured() {
    let mut ext = extension();
    let duplicate = ext["commands"][0].clone();
    ext["commands"].as_array_mut().unwrap().push(duplicate);
    let root = setup(ext, json!({"manifest":".lenso/extension.json"}));
    let output = cli(root.path(), &["demo", "--json"]);
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("command conflict")
    );
    fs::write(
        root.path().join("lenso.toml"),
        "[cli]\nschema_version = 99\n",
    )
    .unwrap();
    let result: Value = serde_json::from_slice(&cli(root.path(), &["--json"]).stdout).unwrap();
    assert_eq!(result["code"], "invalid_input");
    assert_eq!(result["details"]["field"], "cli.schema_version");
    let output = cli(root.path(), &["cache", "path", "--json"]);
    assert!(
        output.status.success(),
        "cache discovery must not load a broken project"
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result["outputs"]["main"]["cache_dir"],
        root.path().join("cache").to_str().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn process_grants_are_exact_logs_do_not_corrupt_json_and_exit_status_survives() {
    let mut ext = extension();
    ext["handlers"]["example/step"] =
        json!({"type":"process","program":"/bin/sh","args":["-c","printf 'child log\\n'; exit 7"]});
    let root = setup(ext, json!({"manifest":".lenso/extension.json"}));
    let output = cli(root.path(), &["demo", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["message"]
            .as_str()
            .unwrap()
            .contains("permission denied")
    );
    write_config(
        root.path(),
        json!({"processes":[{"program":"/bin/sh","args":["-c","printf 'child log\\n'; exit 7"]}]}),
    );
    let output = cli(root.path(), &["demo", "--json"]);
    assert_eq!(output.status.code(), Some(7));
    assert!(String::from_utf8_lossy(&output.stderr).contains("child log"));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["type"],
        "diagnostic"
    );
}

#[cfg(unix)]
#[test]
fn cancellation_stops_the_process_group() {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;
    use std::time::{Duration, Instant};
    let script = "echo $$ > supervisor.pid; sleep 30 & echo $! > descendant.pid; wait";
    let mut ext = extension();
    ext["handlers"]["example/step"] =
        json!({"type":"process","program":"/bin/sh","args":["-c",script]});
    let root = setup(
        ext,
        json!({"manifest":".lenso/extension.json","processes":[{"program":"/bin/sh","args":["-c",script]}]}),
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .arg("--project")
        .arg(root.path())
        .env("LENSO_CACHE_DIR", root.path().join("cache"))
        .args(["demo", "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let (supervisor, descendant) = loop {
        let read_pid = |name| {
            fs::read_to_string(root.path().join(name))
                .ok()?
                .trim()
                .parse::<i32>()
                .ok()
        };
        if let (Some(supervisor), Some(descendant)) =
            (read_pid("supervisor.pid"), read_pid("descendant.pid"))
        {
            break (supervisor, descendant);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("process did not write its PIDs within five seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    kill(
        Pid::from_raw(child.id().try_into().unwrap()),
        Signal::SIGINT,
    )
    .unwrap();
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        let _ = child.kill();
        let _ = kill(Pid::from_raw(supervisor), Signal::SIGKILL);
        let _ = kill(Pid::from_raw(descendant), Signal::SIGKILL);
        panic!("CLI did not cancel within five seconds");
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(130),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    while kill(Pid::from_raw(descendant), None).is_ok() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        kill(Pid::from_raw(supervisor), None).is_err(),
        "supervisor survived cancellation"
    );
    assert!(
        kill(Pid::from_raw(descendant), None).is_err(),
        "descendant survived cancellation"
    );
}

#[test]
fn onboarding_is_reviewed_and_optional_native_providers_are_selected_explicitly() {
    let root = tempfile::tempdir().unwrap();
    let output = cli(root.path(), &["init", "--preset", "empty", "--json"]);
    assert_eq!(output.status.code(), Some(2));
    let pending: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(pending["status"], "needs_input");
    assert!(!root.path().join("lenso.toml").exists());
    assert!(!root.path().join("lenso.lock").exists());
    let answers = root.path().join("answers.json");
    fs::write(
        &answers,
        serde_json::to_vec(&json!({pending["question"]["id"].as_str().unwrap():false})).unwrap(),
    )
    .unwrap();
    let output = cli(
        root.path(),
        &[
            "init",
            "--preset",
            "empty",
            "--answers",
            answers.to_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(output.status.code(), Some(130));
    assert!(!root.path().join("lenso.toml").exists());
    let output = cli(
        root.path(),
        &["init", "--preset", "empty", "--yes", "--json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let lock: Value =
        serde_json::from_slice(&fs::read(root.path().join("lenso.lock")).unwrap()).unwrap();
    assert_eq!(lock["extensions"], json!([]));
    let output = cli(
        root.path(),
        &["ext", "add", "builtin:project", "--yes", "--json"],
    );
    #[cfg(feature = "project")]
    {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let output = cli(
            root.path(),
            &["ext", "tree", "--locked", "--offline", "--json"],
        );
        assert!(output.status.success());
        let tree: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            tree["outputs"]["main"]["extensions"][0]["source"],
            "builtin:project"
        );
        let output = cli(
            root.path(),
            &["ext", "remove", "project", "--yes", "--json"],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let lock: Value =
            serde_json::from_slice(&fs::read(root.path().join("lenso.lock")).unwrap()).unwrap();
        assert_eq!(lock["extensions"], json!([]));
    }
    #[cfg(not(feature = "project"))]
    assert!(!output.status.success());
    let output = cli(
        root.path(),
        &["new", "child", "--preset", "empty", "--json"],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(!root.path().join("child").exists());
    let output = cli(
        root.path(),
        &["new", "child", "--preset", "empty", "--yes", "--json"],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(root.path().join("child/lenso.toml").is_file());
}

#[test]
fn self_update_is_project_independent_and_requires_explicit_adoption() {
    let root = installation_root();
    fs::write(
        root.path().join("lenso.toml"),
        "invalid project declaration",
    )
    .unwrap();
    let executable = root
        .path()
        .join(if cfg!(windows) { "lenso.exe" } else { "lenso" });
    fs::hard_link(env!("CARGO_BIN_EXE_lenso"), &executable).unwrap();
    let run = |args: &[&str]| {
        Command::new(&executable)
            .current_dir(root.path())
            .args(args)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let help = run(&["self", "update", "--help"]);
    assert!(
        help.status.success(),
        "{}",
        String::from_utf8_lossy(&help.stderr)
    );
    assert!(String::from_utf8_lossy(&help.stdout).contains("--version"));
    let invalid = run(&["self", "update", "--version", "bad", "--json"]);
    assert!(!invalid.status.success());
    let diagnostic: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(diagnostic["code"], "invalid_input");
    assert_eq!(diagnostic["details"]["field"], "version");
    let unmanaged = run(&["self", "update", "--offline", "--json"]);
    assert!(!unmanaged.status.success());
    assert!(String::from_utf8_lossy(&unmanaged.stdout).contains("--adopt"));
    let adopted = run(&[
        "self",
        "update",
        "--adopt",
        "--version",
        "1.0.0",
        "--offline",
        "--json",
    ]);
    assert!(!adopted.status.success());
    let diagnostic: Value = serde_json::from_slice(&adopted.stdout).unwrap();
    assert_eq!(diagnostic["details"]["required"], "network_access");
    assert!(!root.path().join("lenso-install.json").exists());
    assert_eq!(
        fs::read(&executable).unwrap(),
        fs::read(env!("CARGO_BIN_EXE_lenso")).unwrap()
    );
}

#[test]
fn package_manager_ownership_cannot_be_overridden_by_adoption() {
    let root = installation_root();
    let name = if cfg!(windows) { "lenso.exe" } else { "lenso" };
    for (directory, expected) in [
        (root.path().join("custom-cargo/bin"), "Cargo"),
        (root.path().join("Cellar/lenso/1.0.0/bin"), "Homebrew"),
    ] {
        fs::create_dir_all(&directory).unwrap();
        if expected == "Cargo" {
            fs::write(directory.parent().unwrap().join(".crates2.json"), "{}").unwrap();
        }
        let executable = directory.join(name);
        fs::hard_link(env!("CARGO_BIN_EXE_lenso"), &executable).unwrap();
        let result = Command::new(&executable)
            .current_dir(root.path())
            .args(["self", "update", "--adopt", "--offline", "--json"])
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(result.status.success());
        let report: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["outputs"]["main"]["state"], "managed");
        assert!(
            report["outputs"]["main"]["message"]
                .as_str()
                .unwrap()
                .contains(expected)
        );
        assert!(!directory.join("lenso-install.json").exists());
    }
}

fn installation_root() -> tempfile::TempDir {
    // Keep hard links on the binary's filesystem; no writable copy can cross a concurrent fork.
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_lenso"));
    tempfile::tempdir_in(binary.parent().unwrap()).unwrap()
}
