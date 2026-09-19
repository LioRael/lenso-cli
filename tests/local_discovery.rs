//! Real CLI discovery: no Host installation or execution is needed.
use lenso_app_plan::{CapabilityEndpointPlan, ExecutionClassId, authoring::PluginContract};
use lenso_plugin_bundle::{
    SourcePluginImplementation, SourcePluginReleaseBuild, build_source_plugin_release_bundle,
};
use std::{fs, process::Command};

#[test]
fn discovers_sources_and_verified_archives_without_executing_or_installing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("app/rust")).unwrap();
    fs::write(root.join("app/rust/Cargo.toml"), "[package]\nname = 'fixture'\nversion = '1.0.0'\n[package.metadata.lenso]\nplugin-id = 'company.rust'\n[package.metadata.lenso-cli]\nruntime = 'process'").unwrap();
    fs::create_dir_all(root.join("app/typescript")).unwrap();
    fs::write(root.join("app/typescript/package.json"), r#"{"name":"fixture","version":"1.0.0","lenso":{"pluginId":"company.typescript","runtime":"bun"},"scripts":{"prepare":"exit 99"}}"#).unwrap();
    let artifact = root.join("plugin.js");
    fs::write(
        &artifact,
        "throw new Error('discovery must not execute this');",
    )
    .unwrap();
    let bundle = root.join("bundle");
    build_source_plugin_release_bundle(&SourcePluginReleaseBuild {
        contract: PluginContract::new("company.shared", "1.0.0", "tools")
            .with_authoring_version(2)
            .with_capability(CapabilityEndpointPlan::new(
                "company.shared@1",
                "1",
                ["get"],
            )),
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
        output: bundle.clone(),
    })
    .unwrap();
    let archive = root.join("shared.lenso-plugin");
    lenso_app_authoring::bundle_archive::archive_bundle(&bundle, &archive).unwrap();
    for source in ["shared.lenso-plugin", "bundle"] {
        fs::write(
            root.join("lenso.toml"),
            format!("plugin_sources = ['{source}']"),
        )
        .unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_lenso"))
            .args(["app", "discover", "--root"])
            .arg(root)
            .arg("--json")
            .current_dir(std::env::temp_dir())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["candidates"].as_array().unwrap().len(), 3);
        assert_eq!(report["candidates"][1]["role"], "shared");
        assert_eq!(
            report["candidates"][1]["evidence"],
            "verified_bundle_not_admitted"
        );
        assert!(!root.join(".lenso").exists());
        assert!(!root.join("plugins").exists());
    }
}
