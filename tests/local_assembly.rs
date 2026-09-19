use lenso_app_plan::{
    CapabilityEndpointPlan, CapabilityRequirementPlan, ExecutionClassId, authoring::PluginContract,
};
use lenso_plugin_bundle::{
    SourcePluginImplementation, SourcePluginReleaseBuild, build_source_plugin_release_bundle,
};
use std::{fs, path::Path, process::Command};

fn bundle(root: &Path, path: &str, id: &str, consumer: bool) {
    fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    let artifact = root.join(format!("{id}.js"));
    fs::write(
        &artifact,
        "throw new Error('assembly must not start Plugin lifecycle');",
    )
    .unwrap();
    let mut contract = PluginContract::new(id, "1.0.0", "tools")
        .with_authoring_version(2)
        .with_capability(CapabilityEndpointPlan::new(
            if consumer {
                "company.copy@1"
            } else {
                "company.store@1"
            },
            "1",
            ["get"],
        ));
    if consumer {
        contract = contract.with_requirement(
            CapabilityRequirementPlan::one("company.store@1", "1").with_requirement_id("store"),
        );
    }
    build_source_plugin_release_bundle(&SourcePluginReleaseBuild {
        contract,
        implementations: vec![SourcePluginImplementation {
            id: "bun".into(),
            host_targets: vec!["*".into()],
            artifact,
            bundle_path: "implementations/bun/plugin.js".into(),
            media_type: "application/javascript".into(),
            target: "javascript-bun".into(),
            entrypoint: "plugin.js".into(),
            execution_class: ExecutionClassId::bun_child_process(),
            runtime_profile: lenso_app_plan::PLUGIN_AUTHORING_V2_RUNTIME_PROFILE.into(),
        }],
        output: root.join(path),
    })
    .unwrap();
}

fn assemble(root: &Path, out: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lenso"))
        .args(["app", "assemble", "--root"])
        .arg(root)
        .arg("--out")
        .arg(root.join(out))
        .arg("--json")
        .output()
        .unwrap()
}

#[test]
fn shared_sources_require_explicit_root_intent_and_failed_assembly_is_atomic() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    bundle(root, "app/copy", "company.copy", true);
    bundle(root, "shared/store", "company.store", false);
    fs::write(root.join("lenso.toml"), "plugin_sources = ['shared']").unwrap();
    let missing = assemble(root, "missing");
    assert!(!missing.status.success());
    assert!(!root.join("missing").exists());
    assert!(!root.join(".lenso").exists());
    fs::create_dir_all(root.join("plugins/company.store")).unwrap();
    fs::write(root.join("plugins/company.store/default.toml"), "").unwrap();
    let output = assemble(root, "assembled");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["plugin_instances"], 2);
    assert_eq!(report["capability_bindings"], 1);
    let resolved = lenso_app_authoring::load_resolved_app(&root.join("assembled")).unwrap();
    assert_eq!(resolved.instances().len(), 2);
    assert!(!assemble(root, "assembled").status.success());
    bundle(root, "shared/alternative", "company.alternative", false);
    fs::create_dir_all(root.join("plugins/company.alternative")).unwrap();
    fs::write(root.join("plugins/company.alternative/default.toml"), "").unwrap();
    assert!(!assemble(root, "ambiguous").status.success());
    assert!(!root.join("ambiguous").exists());
    fs::copy(
        root.join("assembled/plugins/.dependencies.json"),
        root.join("plugins/.dependencies.json"),
    )
    .unwrap();
    let saved = assemble(root, "saved-choice");
    assert!(
        saved.status.success(),
        "{}",
        String::from_utf8_lossy(&saved.stderr)
    );
    fs::remove_file(root.join("plugins/company.store/default.toml")).unwrap();
    fs::write(root.join("plugins/company.store/default.disabled"), "").unwrap();
    assert!(!assemble(root, "disabled-provider").status.success());
    assert!(!root.join("disabled-provider").exists());
}

#[test]
fn app_defaults_can_be_disabled_without_changing_the_original_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    bundle(root, "app/store", "company.store", false);
    let first = assemble(root, "first");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    fs::create_dir_all(root.join("plugins/company.store")).unwrap();
    fs::write(root.join("plugins/company.store/default.disabled"), "").unwrap();
    let second = assemble(root, "second");
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        lenso_app_authoring::load_resolved_app(&root.join("second"))
            .unwrap()
            .instances()
            .is_empty()
    );
    assert_eq!(
        lenso_app_authoring::load_resolved_app(&root.join("first"))
            .unwrap()
            .instances()
            .len(),
        1
    );
    assert!(!root.join(".lenso").exists());
}
