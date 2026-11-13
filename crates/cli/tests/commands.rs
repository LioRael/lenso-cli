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
    assert!(
        result["message"]
            .as_str()
            .unwrap()
            .contains("schema_version")
    );
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
