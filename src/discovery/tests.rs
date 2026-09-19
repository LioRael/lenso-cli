use super::*;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn rust(root: &Path, path: &str, id: &str) {
    write(
        root,
        &format!("{path}/Cargo.toml"),
        &format!(
            r#"
[package]
name = "fixture"
version = "1.0.0"
[package.metadata.lenso]
plugin-id = "{id}"
root-slot = "tools"
[package.metadata.lenso-cli]
outputs = ["wasm", "process"]
"#
        ),
    );
}

fn bun(root: &Path, path: &str, id: &str) {
    write(root, &format!("{path}/package.json"), &serde_json::json!({
        "name": "fixture", "version": "1.0.0", "lenso": {"pluginId": id, "rootSlot": "tools", "runtime": "bun"},
        "scripts": {"prepare": "exit 99"}
    }).to_string());
}

#[test]
fn discovers_multiple_languages_without_configuration_or_executing_code() {
    let root = tempfile::tempdir().unwrap();
    rust(root.path(), "app/z", "example.rust");
    bun(root.path(), "app/a", "example.bun");
    let report = discover(root.path()).unwrap();
    assert_eq!(
        report
            .candidates
            .iter()
            .map(|candidate| candidate.plugin_id.as_str())
            .collect::<Vec<_>>(),
        ["example.bun", "example.rust"]
    );
    assert!(
        report
            .candidates
            .iter()
            .all(|candidate| candidate.role == SourceRole::AppOwned)
    );
    assert_eq!(report.candidates[1].implementations.len(), 2);
    assert!(!root.path().join(".lenso").exists());
    assert!(!root.path().join("plugins").exists());
}

#[test]
fn optional_shared_globs_are_relative_to_app_and_remain_candidates() {
    let root = tempfile::tempdir().unwrap();
    rust(root.path(), "shared/a", "example.shared");
    write(
        root.path(),
        "project/lenso.toml",
        "plugin_sources = [\"../shared/*\", \"../shared/a\"]",
    );
    let report = discover(&root.path().join("project")).unwrap();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].role, SourceRole::Shared);
}

#[test]
fn excludes_outputs_dependency_trees_and_plugin_root() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["target", "node_modules", "dist", ".git", "plugins"] {
        rust(
            root.path(),
            &format!("app/{directory}/bad"),
            "example.duplicate",
        );
    }
    assert!(discover(root.path()).unwrap().candidates.is_empty());
}

#[test]
fn rejects_identity_collisions_instead_of_choosing_by_order() {
    let root = tempfile::tempdir().unwrap();
    rust(root.path(), "app/a", "example.same");
    bun(root.path(), "app/b", "example.same");
    let error = format!("{:#}", discover(root.path()).unwrap_err());
    assert!(error.contains("duplicate Plugin identity"));
    assert!(error.contains("app/a") && error.contains("app/b"));
}

#[test]
fn rejects_unknown_configuration_missing_roots_and_remote_sources() {
    let root = tempfile::tempdir().unwrap();
    for text in [
        "preset = 'web'",
        "plugin_sources = ['missing']",
        "plugin_sources = ['missing/*']",
        "plugin_sources = ['https://example.com']",
    ] {
        write(root.path(), "lenso.toml", text);
        assert!(discover(root.path()).is_err(), "{text}");
    }
}

#[test]
fn discovers_native_web_and_inherited_workspace_version() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "app/Cargo.toml",
        "[workspace]\nmembers = ['web']\n[workspace.package]\nversion = '2.0.0'",
    );
    write(
        root.path(),
        "app/web/Cargo.toml",
        "[package]\nname = 'web'\nversion.workspace = true\n[package.metadata.lenso]\nplugin-id = 'example.web'\nroot-slot = 'web'",
    );
    let report = discover(root.path()).unwrap();
    assert_eq!(report.candidates[0].release_version, "2.0.0");
    assert_eq!(
        report.candidates[0].implementations[0].runtime,
        "native-linked"
    );
}

#[test]
fn composite_owns_nested_implementations_without_duplicate_plugins() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "app/mixed/Cargo.toml",
        r#"
[package]
name = "mixed"
version = "1.0.0"
[package.metadata.lenso]
plugin-id = "example.mixed"
[package.metadata.lenso-cli]
implementations = [
  { id = "rust", runtime = "process", path = "." },
  { id = "ts", runtime = "bun", path = "typescript" },
]
"#,
    );
    bun(root.path(), "app/mixed/typescript", "example.mixed");
    let report = discover(root.path()).unwrap();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].implementations.len(), 2);
    bun(root.path(), "app/mixed/typescript", "example.other");
    assert!(
        format!("{:#}", discover(root.path()).unwrap_err()).contains("different Plugin identity")
    );
}

#[test]
fn rejects_corrupt_local_archives_without_installation() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "app/bad.lenso-plugin", "not a bundle");
    assert!(discover(root.path()).is_err());
    assert!(!root.path().join("plugins").exists());
}

#[cfg(unix)]
#[test]
fn nested_symlinks_do_not_escape_sources_or_loop() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    rust(root.path(), "external", "example.external");
    fs::create_dir(root.path().join("app")).unwrap();
    symlink(root.path(), root.path().join("app/loop")).unwrap();
    symlink(root.path().join("external"), root.path().join("app/linked")).unwrap();
    assert!(discover(root.path()).unwrap().candidates.is_empty());
    write(root.path(), "lenso.toml", "plugin_sources = ['app/linked']");
    assert_eq!(
        discover(root.path()).unwrap().candidates[0].role,
        SourceRole::Shared
    );
}

#[test]
fn rejects_sources_with_conflicting_roles() {
    let root = tempfile::tempdir().unwrap();
    rust(root.path(), "app/owned", "example.owned");
    write(root.path(), "lenso.toml", "plugin_sources = ['app/owned']");
    assert!(
        discover(root.path())
            .unwrap_err()
            .to_string()
            .contains("overlaps")
    );
}

#[test]
fn workspace_members_and_excludes_bound_discovery() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "app/Cargo.toml",
        "[workspace]\nmembers = ['packages/*']\nexclude = ['packages/excluded']",
    );
    rust(root.path(), "app/packages/included", "example.included");
    rust(root.path(), "app/packages/excluded", "example.excluded");
    rust(root.path(), "app/not-a-member", "example.other");
    let report = discover(root.path()).unwrap();
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].plugin_id, "example.included");
}

#[test]
fn npm_workspace_members_are_discovered_without_running_package_scripts() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "app/package.json",
        r#"{"workspaces":{"packages":["packages/*"]}}"#,
    );
    bun(root.path(), "app/packages/one", "example.one");
    bun(root.path(), "app/outside", "example.outside");
    assert_eq!(discover(root.path()).unwrap().candidates.len(), 1);
}

#[test]
fn recursive_globs_are_rejected_in_favor_of_bounded_directory_scanning() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "lenso.toml", "plugin_sources = ['../**']");
    assert!(
        discover(root.path())
            .unwrap_err()
            .to_string()
            .contains("recursive **")
    );
}
