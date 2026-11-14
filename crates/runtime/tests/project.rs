use lenso_cli::Cli;
use lenso_runtime::{
    LoadMode, LoadOptions, Project, load_project_with_options, migrate_project, prepare_project,
};
use serde_json::json;
use std::fs;

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("lenso")).unwrap();
    fs::write(root.path().join("lenso/guest.wasm"), b"\0asm\x01\0\0\0").unwrap();
    fs::write(root.path().join("lenso/extension.json"), serde_json::to_vec(&json!({
        "schema_version":1,"id":"example","version":"0.1.0",
        "handlers":{"example/work":{"type":"wasm","module":"guest.wasm","function":"run","sha256":"auto"}},
        "workflows":[{"id":"example/work","steps":[{"id":"work","handler":"example/work"}]}],
        "commands":[{"path":["demo"],"workflow":"example/work","about":"Locked command","provider":"example"}]
    })).unwrap()).unwrap();
    let config: Project = serde_json::from_value(json!({"cli":{"schema_version":1,"extensions":[{"id":"example","source":"lenso/extension.json"}]}})).unwrap();
    fs::write(
        root.path().join("lenso.toml"),
        toml::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
    root
}

fn options(root: &std::path::Path) -> LoadOptions {
    LoadOptions {
        cache_dir: Some(root.join("cache")),
        ..Default::default()
    }
}

#[test]
fn static_contracts_follow_project_aliases_and_offline_locks_without_policy_values() {
    let root = fixture();
    let path = root.path().join("lenso/extension.json");
    let mut definition: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    definition["contracts"] = json!({"example/work":{
        "capabilities":["host.reads"],"effects":[],"verification":["Inspect output"]
    }});
    fs::write(&path, serde_json::to_vec(&definition).unwrap()).unwrap();
    let config_path = root.path().join("lenso.toml");
    let mut config: Project = toml::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    config.cli.extensions[0]
        .aliases
        .insert("demo".into(), vec!["tool".into(), "check".into()]);
    config.cli.extensions[0]
        .options
        .insert("private".into(), json!("SECRET"));
    fs::write(&config_path, toml::to_string(&config).unwrap()).unwrap();
    let mut policy = options(root.path());
    policy.mode = LoadMode::Discover;
    let mut cli = Cli::new("test");
    load_project_with_options(&mut cli, root.path(), &policy).unwrap();
    assert!(!root.path().join("lenso.lock").exists());
    assert!(!root.path().join("cache/v1/blobs").exists());
    let wire = serde_json::to_value(cli.describe(&["tool".into()]).unwrap()).unwrap();
    assert_eq!(wire["commands"][0]["provider_version"], "0.1.0");
    assert_eq!(wire["commands"][0]["contract"]["effects"], json!([]));
    assert!(!wire.to_string().contains("SECRET"));
    assert!(cli.describe(&["demo".into()]).is_err());
    assert!(cli.describe(&["ops".into()]).is_err());
    prepare_project(root.path(), &options(root.path())).unwrap();
    let lock = fs::read(root.path().join("lenso.lock")).unwrap();
    fs::remove_file(&path).unwrap();
    fs::remove_file(root.path().join("lenso/guest.wasm")).unwrap();
    policy.offline = true;
    policy.locked = true;
    let mut restored = Cli::new("test");
    load_project_with_options(&mut restored, root.path(), &policy).unwrap();
    assert_eq!(
        serde_json::to_value(restored.describe(&["tool".into()]).unwrap()).unwrap(),
        wire
    );
    assert_eq!(fs::read(root.path().join("lenso.lock")).unwrap(), lock);
}

#[test]
fn invalid_contract_ownership_leaves_discovery_and_lock_unchanged() {
    let root = fixture();
    let path = root.path().join("lenso/extension.json");
    let mut definition: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    definition["contracts"] = json!({"other/work":{}});
    fs::write(path, serde_json::to_vec(&definition).unwrap()).unwrap();
    let mut cli = Cli::new("test");
    assert!(load_project_with_options(&mut cli, root.path(), &options(root.path())).is_err());
    assert!(cli.describe(&[]).unwrap().commands.is_empty());
    assert!(!root.path().join("lenso.lock").exists());
}

#[tokio::test]
async fn first_command_prepares_lock_and_deleted_global_cache_recovers_pinned_bytes() {
    let root = fixture();
    let options = options(root.path());
    let mut cli = Cli::new("test");
    load_project_with_options(&mut cli, root.path(), &options).unwrap();
    let lock = fs::read(root.path().join("lenso.lock")).unwrap();
    assert!(!String::from_utf8_lossy(&lock).contains(root.path().to_str().unwrap()));
    assert!(!root.path().join(".lenso").exists());
    assert!(
        !root.path().join("cache/v1/blobs").exists(),
        "metadata loading must not hydrate modules"
    );
    let mut session = cli
        .engine_mut()
        .session("example/work", root.path(), json!({}), Default::default())
        .unwrap();
    assert!(
        session.advance(cli.engine_mut()).await.is_err(),
        "empty module has no run export"
    );
    let blobs = root.path().join("cache/v1/blobs/sha256");
    let module = fs::read_dir(&blobs)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(&module).unwrap(), b"\0asm\x01\0\0\0");
    fs::remove_dir_all(root.path().join("cache")).unwrap();
    let mut help = Cli::new("test");
    load_project_with_options(
        &mut help,
        root.path(),
        &LoadOptions {
            mode: LoadMode::Discover,
            ..options.clone()
        },
    )
    .unwrap();
    assert!(
        !root.path().join("cache").exists(),
        "locked help must not touch cache"
    );
    let mut session = help
        .engine_mut()
        .session("example/work", root.path(), json!({}), Default::default())
        .unwrap();
    assert!(session.advance(help.engine_mut()).await.is_err());
    assert_eq!(fs::read(module).unwrap(), b"\0asm\x01\0\0\0");
    assert_eq!(fs::read(root.path().join("lenso.lock")).unwrap(), lock);
}

#[test]
fn source_edits_keep_pins_until_explicit_update() {
    let root = fixture();
    let mut options = options(root.path());
    prepare_project(root.path(), &options).unwrap();
    let previous = fs::read(root.path().join("lenso.lock")).unwrap();
    let path = root.path().join("lenso/extension.json");
    let mut definition: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    definition["version"] = json!("0.2.0");
    fs::write(&path, serde_json::to_vec(&definition).unwrap()).unwrap();
    prepare_project(root.path(), &options).unwrap();
    assert_eq!(fs::read(root.path().join("lenso.lock")).unwrap(), previous);
    options.mode = LoadMode::Update;
    prepare_project(root.path(), &options).unwrap();
    assert_ne!(fs::read(root.path().join("lenso.lock")).unwrap(), previous);
}

#[test]
fn migration_preserves_policy_and_unknown_local_files() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".lenso")).unwrap();
    let definition = json!({"schema_version":1,"id":"native","version":"0.1.0",
        "handlers":{"native/check":{"type":"process","program":"tool","args":["status"]}},
        "workflows":[{"id":"native/check","steps":[{"id":"check","handler":"native/check"}]}],
        "commands":[{"path":["check"],"workflow":"native/check","about":"Check","provider":"native"}]});
    fs::write(
        root.path().join(".lenso/extension.json"),
        serde_json::to_vec(&definition).unwrap(),
    )
    .unwrap();
    fs::write(root.path().join(".lenso/project.json"), serde_json::to_vec(&json!({"schema_version":1,"extensions":[{"manifest":".lenso/extension.json","processes":[{"program":"tool","args":["status"]}],"disabled_commands":["check"]}]})).unwrap()).unwrap();
    fs::write(root.path().join(".lenso/user-notes.txt"), "keep").unwrap();
    assert_eq!(migrate_project(root.path()).unwrap(), 1);
    let config = lenso_runtime::read_project(root.path()).unwrap().unwrap();
    assert_eq!(config.cli.extensions[0].processes[0].program, "tool");
    assert_eq!(config.cli.extensions[0].disabled_commands, ["check"]);
    assert!(root.path().join("lenso.lock").exists());
    assert!(!root.path().join(".lenso/project.json").exists());
    assert!(!root.path().join(".lenso/extension.json").exists());
    assert_eq!(
        fs::read_to_string(root.path().join(".lenso/user-notes.txt")).unwrap(),
        "keep"
    );
    assert!(
        migrate_project(root.path()).is_err(),
        "existing root declarations must never be overwritten"
    );
}

#[cfg(unix)]
#[test]
fn cache_symlinks_are_rejected_before_writing_outside_project() {
    let root = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("cache")).unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("cache/v1")).unwrap();
    assert!(prepare_project(root.path(), &options(root.path())).is_err());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    assert!(!root.path().join("lenso.lock").exists());
}

#[test]
fn migration_rejects_changed_legacy_wasm_before_committing_or_removing_files() {
    let root = fixture();
    let legacy = root.path().join(".lenso");
    fs::create_dir(&legacy).unwrap();
    let mut definition: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("lenso/extension.json")).unwrap())
            .unwrap();
    definition["handlers"]["example/work"]["sha256"] = json!("0".repeat(64));
    fs::write(
        legacy.join("extension.json"),
        serde_json::to_vec(&definition).unwrap(),
    )
    .unwrap();
    fs::write(legacy.join("guest.wasm"), b"\0asm\x01\0\0\0").unwrap();
    fs::write(
        legacy.join("project.json"),
        serde_json::to_vec(
            &json!({"schema_version":1,"extensions":[{"manifest":".lenso/extension.json"}]}),
        )
        .unwrap(),
    )
    .unwrap();
    fs::remove_file(root.path().join("lenso.toml")).unwrap();
    assert!(
        migrate_project(root.path())
            .unwrap_err()
            .to_string()
            .contains("legacy module digest mismatch")
    );
    assert!(legacy.join("project.json").exists() && legacy.join("guest.wasm").exists());
    assert!(!root.path().join("lenso.toml").exists() && !root.path().join("lenso.lock").exists());
}
