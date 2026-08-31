use super::scaffold::{
    bun_plugin_scaffold, create, multi_plugin_scaffold, plugin_scaffold, process_plugin_scaffold,
    web_plugin_scaffold,
};
use super::*;

#[test]
fn web_plugin_scaffold_uses_canonical_endpoint_authoring() {
    let files = web_plugin_scaffold("company.greetings-http");
    let manifest = files.get(Path::new("Cargo.toml")).unwrap();
    let source = files.get(Path::new("src/lib.rs")).unwrap();
    let readme = files.get(Path::new("README.md")).unwrap();

    assert!(manifest.contains("plugin-id = \"company.greetings-http\""));
    assert!(manifest.contains("root-slot = \"web\""));
    assert!(manifest.contains("lenso-capability-http-endpoint"));
    assert!(manifest.contains("version = \"0.2.8\""));
    assert!(source.contains("#[lenso::plugin]"));
    assert!(source.contains("#[endpoint]"));
    assert!(source.contains("#[query("));
    assert!(source.contains("Result<(StatusCode, Json<Greeting>), Problem>"));
    assert!(source.contains("EndpointTest"));
    assert!(source.contains("pub const fn link()"));
    assert!(!source.contains("NativeModuleFactory"));
    assert!(!readme.contains("lenso plugin pack"));
    assert!(readme.contains("lenso plugin dev"));
}

#[test]
fn web_plugin_new_writes_the_complete_project() {
    let root = tempfile::tempdir().unwrap();
    create(PluginNewArgs {
        plugin_id: "company.greetings-http".to_owned(),
        repo_root: Some(root.path().to_path_buf()),
        dir: None,
        runtime: PluginRuntimeArg::Multi,
        web: true,
        no_install: true,
        dry_run: false,
    })
    .unwrap();

    let project = root.path().join("company.greetings-http");
    for path in ["Cargo.toml", "src/lib.rs", "README.md"] {
        assert!(project.join(path).is_file(), "missing generated {path}");
    }
}

#[test]
#[ignore = "clean-room test downloads pinned Web dependencies and runs generated tests"]
fn clean_room_web_plugin_runs_generated_tests() {
    let root = tempfile::tempdir().unwrap();
    create(PluginNewArgs {
        plugin_id: "company.greetings-http".to_owned(),
        repo_root: Some(root.path().to_path_buf()),
        dir: None,
        runtime: PluginRuntimeArg::Multi,
        web: true,
        no_install: false,
        dry_run: false,
    })
    .unwrap();
}

#[test]
#[ignore = "clean-room test downloads pinned Web dependencies and builds the generated dev Host"]
fn clean_room_web_plugin_builds_generated_dev_host() {
    let root = tempfile::tempdir().unwrap();
    create(PluginNewArgs {
        plugin_id: "company.greetings-http".to_owned(),
        repo_root: Some(root.path().to_path_buf()),
        dir: None,
        runtime: PluginRuntimeArg::Multi,
        web: true,
        no_install: false,
        dry_run: false,
    })
    .unwrap();
    let project = root.path().join("company.greetings-http");
    let package = read_package(&project.join("Cargo.toml")).unwrap();
    let host = super::web_dev::DevHost::prepare(&project, &package).unwrap();
    host.build().unwrap();
}

#[test]
fn rust_plugin_scaffold_exposes_only_portable_authoring() {
    let files = plugin_scaffold("uppercase");
    let author_source = files.get(Path::new("src/lib.rs")).unwrap();
    let all = files.values().cloned().collect::<String>();

    assert!(author_source.contains("#[lenso::plugin]"));
    assert!(author_source.contains("#[lenso_agent_tool_sdk::tool_provider]"));
    assert!(author_source.contains("#[tool("));
    assert!(author_source.contains("fn execute(arguments: Arguments)"));
    assert!(all.contains("plugin-id = \"uppercase\""));
    assert!(all.contains("root-slot = \"tool-providers\""));
    assert!(all.contains("lenso plugin new"));
    assert!(all.contains("lenso plugin dev"));
    assert!(all.contains("lenso plugin check"));
    assert!(all.contains("lenso plugin pack"));
    for internal in [
        "wit_bindgen",
        "guest_request_plugin",
        "ProcessPlugin",
        "ProcessOutcome",
        "request_json",
        "arguments_json",
        "lenso.agent.tool-provider",
        "lenso.generated",
    ] {
        assert!(
            !author_source.contains(internal),
            "author source leaked `{internal}`"
        );
    }
    for removed in [
        "src/plugin.rs",
        "src/lenso.generated.rs",
        "src/lenso.wasm.generated.rs",
        "src/lenso.process.generated.rs",
        "lenso.generated.descriptor.json",
        "wit/world.wit",
    ] {
        assert!(
            !files.contains_key(Path::new(removed)),
            "unexpected `{removed}`"
        );
    }
}

#[test]
fn bun_plugin_scaffold_uses_generated_runtime_lowering() {
    let files = bun_plugin_scaffold("example.echo");
    let package = files.get(Path::new("package.json")).unwrap();
    let author = files.get(Path::new("src/plugin.ts")).unwrap();
    let runtime = files.get(Path::new("src/lenso.bun.generated.ts")).unwrap();

    assert!(package.contains("\"runtime\": \"bun\""));
    assert!(package.contains("\"@lenso/bun\": \"0.2.0\""));
    assert!(author.contains("bindToolProviderProvider"));
    assert!(author.contains("definePlugin"));
    assert!(!author.contains("serve("));
    assert!(runtime.contains("serve(plugin)"));
}

#[test]
fn process_plugin_scaffold_uses_the_sdk_owned_lowering() {
    let files = process_plugin_scaffold("uppercase");
    let manifest = files.get(Path::new("Cargo.toml")).unwrap();
    let entrypoint = files.get(Path::new("src/main.rs")).unwrap();

    assert!(manifest.contains("runtime = \"process\""));
    assert!(manifest.contains("package = \"lenso-plugin-sdk\""));
    assert!(manifest.contains("lenso-agent-tool-sdk"));
    assert_eq!(
        entrypoint,
        "// Cargo Process entrypoint; the SDK supplies main and protocol lowering.\ninclude!(\"lib.rs\");\n"
    );
    assert!(!files.contains_key(Path::new("lenso.generated.descriptor.json")));
}

#[test]
fn multi_scaffold_keeps_one_business_source_for_two_outputs() {
    let files = multi_plugin_scaffold("uppercase");
    let manifest = files.get(Path::new("Cargo.toml")).unwrap();

    assert!(manifest.contains("outputs = [\"wasm\", \"process\"]"));
    assert!(files.contains_key(Path::new("src/lib.rs")));
    assert!(files.contains_key(Path::new("src/main.rs")));
    assert_eq!(
        files
            .keys()
            .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
            .count(),
        2
    );
    let author_source = files.get(Path::new("src/lib.rs")).unwrap();
    for runtime_detail in ["wit_bindgen", "ProcessPlugin", "ProcessOutcome", "Guest"] {
        assert!(
            !author_source.contains(runtime_detail),
            "author source leaked runtime detail `{runtime_detail}`"
        );
    }
}
#[test]
fn duplicate_plugin_identity_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let manifest = root.path().join("Cargo.toml");
    fs::write(
        &manifest,
        r#"[package]
name = "duplicate"
version = "0.1.0"
[package.metadata.lenso]
plugin-id = "first"
plugin-id = "second"
"#,
    )
    .unwrap();

    assert!(read_package(&manifest).is_err());
}

#[test]
fn multi_dev_auto_selects_only_the_fast_process_path() {
    assert_eq!(
        resolve_dev_selection(ProjectRuntime::Multi, DevImplementationArg::Auto).unwrap(),
        DevSelection {
            build: DevBuild::Process,
            invoke: ProjectRuntime::Process,
        }
    );
    assert_eq!(
        resolve_dev_selection(ProjectRuntime::Multi, DevImplementationArg::Wasm).unwrap(),
        DevSelection {
            build: DevBuild::Wasm,
            invoke: ProjectRuntime::Wasm,
        }
    );
    assert_eq!(
        resolve_dev_selection(ProjectRuntime::Multi, DevImplementationArg::All).unwrap(),
        DevSelection {
            build: DevBuild::All,
            invoke: ProjectRuntime::Process,
        }
    );
}

#[test]
fn dev_rejects_an_implementation_the_project_does_not_declare() {
    assert!(resolve_dev_selection(ProjectRuntime::Wasm, DevImplementationArg::Process).is_err());
    assert!(resolve_dev_selection(ProjectRuntime::Process, DevImplementationArg::Wasm).is_err());
}

#[test]
fn malformed_plugin_package_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("lenso-plugin.json"), b"{}\n").unwrap();

    assert!(verify_bundle_directory(root.path()).is_err());
}

#[tokio::test]
#[ignore = "clean-room test downloads released crates and compiles wasm32"]
async fn clean_room_plugin_runs_new_check_dev_and_pack() {
    let root = tempfile::tempdir().unwrap();
    create(PluginNewArgs {
        plugin_id: "company.uppercase".to_owned(),
        repo_root: Some(root.path().to_path_buf()),
        dir: None,
        runtime: PluginRuntimeArg::Wasm,
        web: false,
        no_install: false,
        dry_run: false,
    })
    .unwrap();
    let project = root.path().join("company.uppercase");
    check(PluginCheckArgs {
        repo_root: Some(project.clone()),
        json: true,
    })
    .unwrap();
    dev::run(PluginDevArgs {
        repo_root: Some(project.clone()),
        operation: Some("execute".to_owned()),
        request_json: r#"{"name":"company.uppercase","arguments_json":"{\"text\":\"hello\"}"}"#
            .to_owned(),
        json: true,
        watch: false,
        implementation: DevImplementationArg::Auto,
    })
    .await
    .unwrap();
    let output = project.join("dist/company.uppercase.lenso-plugin");
    pack(PluginPackArgs {
        repo_root: Some(project.clone()),
        output: Some(output.clone()),
        json: true,
    })
    .unwrap();
    with_bundle_directory(&output, |directory| {
        verify_bundle_directory(directory)
            .map(|_| ())
            .map_err(Into::into)
    })
    .unwrap();
    assert!(
        pack(PluginPackArgs {
            repo_root: Some(project),
            output: Some(output),
            json: false,
        })
        .is_err()
    );
}

#[tokio::test]
#[ignore = "clean-room test downloads git dependencies and compiles both scaffold outputs"]
async fn clean_room_multi_plugin_auto_dev_runs_the_process_build() {
    let root = tempfile::tempdir().unwrap();
    create(PluginNewArgs {
        plugin_id: "company.multi-smoke".to_owned(),
        repo_root: Some(root.path().to_path_buf()),
        dir: None,
        runtime: PluginRuntimeArg::Multi,
        web: false,
        no_install: false,
        dry_run: false,
    })
    .unwrap();
    dev::run(PluginDevArgs {
        repo_root: Some(root.path().join("company.multi-smoke")),
        operation: Some("execute".to_owned()),
        request_json:
            r#"{"name":"company.multi-smoke","arguments_json":"{\"text\":\"auto-process\"}"}"#
                .to_owned(),
        json: true,
        watch: false,
        implementation: DevImplementationArg::Auto,
    })
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "clean-room test downloads git dependencies and compiles a native executable"]
async fn clean_room_process_plugin_runs_new_check_dev_and_pack() {
    let root = tempfile::tempdir().unwrap();
    create(PluginNewArgs {
        plugin_id: "company.uppercase".to_owned(),
        repo_root: Some(root.path().to_path_buf()),
        dir: None,
        runtime: PluginRuntimeArg::Process,
        web: false,
        no_install: false,
        dry_run: false,
    })
    .unwrap();
    let project = root.path().join("company.uppercase");
    check(PluginCheckArgs {
        repo_root: Some(project.clone()),
        json: true,
    })
    .unwrap();
    dev::run(PluginDevArgs {
        repo_root: Some(project.clone()),
        operation: Some("execute".to_owned()),
        request_json: r#"{"name":"company.uppercase","arguments_json":"{\"text\":\"hello\"}"}"#
            .to_owned(),
        json: true,
        watch: false,
        implementation: DevImplementationArg::Auto,
    })
    .await
    .unwrap();
    let output = project.join("dist/company.uppercase.lenso-plugin");
    pack(PluginPackArgs {
        repo_root: Some(project),
        output: Some(output.clone()),
        json: true,
    })
    .unwrap();
    with_bundle_directory(&output, |directory| {
        verify_bundle_directory(directory)
            .map(|_| ())
            .map_err(Into::into)
    })
    .unwrap();
}
