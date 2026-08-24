#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

#[test]
fn compose_run_uses_a_hidden_plan_and_the_product_runner() {
    let root = fixture_root();
    let runner = root.join("runner.sh");
    write_executable(
        &runner,
        r#"#!/bin/sh
set -eu
test -f "$LENSO_RESOLVED_PLAN"
test "$LENSO_COMPOSITION_VARIANT" = "example"
test "$LENSO_RESOLVED_PLAN" = "$PWD/.lenso/compose/example/resolved-plan.json"
test "$1" = "runtime-argument"
printf '%s\n' "$LENSO_COMPOSITION_VARIANT" > runner-marker.txt
"#,
    );
    fs::write(root.join("fragment.json"), "{}\n").unwrap();
    fs::write(
        root.join("recipes.json"),
        format!(
            "{}\n",
            serde_json::json!({
                "schema_version": 1,
                "root": ".",
                "runner": { "program": runner, "args": [] },
                "variants": {
                    "example": {
                        "fragments": ["fragment.json"],
                        "output": "release/resolved-plan.json"
                    }
                }
            })
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args([
            "compose",
            "run",
            "--recipe",
            root.join("recipes.json").to_str().unwrap(),
            "--variant",
            "example",
            "--",
            "runtime-argument",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("runner-marker.txt")).unwrap(),
        "example\n"
    );
    assert!(
        root.join(".lenso/compose/example/resolved-plan.json")
            .is_file()
    );
    assert!(!root.join("release/resolved-plan.json").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn compose_dev_restarts_the_product_runner_after_a_source_change() {
    let root = fixture_root();
    let runner = root.join("runner.sh");
    write_executable(
        &runner,
        r#"#!/bin/sh
set -eu
count_file="$PWD/.lenso/runner-count"
count=0
if test -f "$count_file"; then count="$(cat "$count_file")"; fi
count=$((count + 1))
printf '%s\n' "$count" > "$count_file"
trap 'exit 0' INT TERM
while :; do sleep 1; done
"#,
    );
    let fragment = root.join("fragment.json");
    fs::write(&fragment, "{}\n").unwrap();
    fs::write(
        root.join("recipes.json"),
        format!(
            "{}\n",
            serde_json::json!({
                "schema_version": 1,
                "root": ".",
                "runner": { "program": runner, "args": [] },
                "variants": {
                    "example": {
                        "fragments": ["fragment.json"],
                        "output": "release/resolved-plan.json"
                    }
                }
            })
        ),
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args([
            "compose",
            "dev",
            "--recipe",
            root.join("recipes.json").to_str().unwrap(),
            "--variant",
            "example",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let count = root.join(".lenso/runner-count");
    wait_for_value(&count, "1\n", &mut child);

    fs::write(&fragment, "{ }\n").unwrap();
    wait_for_value(&count, "2\n", &mut child);

    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id().cast_signed()),
        nix::sys::signal::Signal::SIGINT,
    )
    .unwrap();
    assert!(child.wait().unwrap().success());
    fs::remove_dir_all(root).unwrap();
}

fn fixture_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lenso-compose-cli-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn wait_for_value(path: &Path, expected: &str, child: &mut std::process::Child) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read_to_string(path).ok().as_deref() == Some(expected) {
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("compose dev exited before writing {expected:?}: {status}");
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {expected:?}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}
