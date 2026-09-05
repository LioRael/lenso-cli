//! End-to-end authoring proof with the real TS parser, bundle verifier, and CLI.
//! Requires the repository's `pnpm install` and `pnpm build:shim` prerequisites.

use lenso_app_plan::{
    CapabilityEndpointPlan, CapabilityRequirementPlan, ExecutionClassId, authoring::PluginContract,
};
use lenso_plugin_bundle::{
    SourcePluginImplementation, SourcePluginReleaseBuild, build_source_plugin_release_bundle,
};
use std::{fs, path::Path, process::Command};

fn fixture_bundle(root: &Path, id: &str, consumes_store: bool) {
    fixture_bundle_bytes(
        root,
        id,
        consumes_store,
        "throw new Error('authoring must not execute this Plugin');",
    );
}

fn fixture_bundle_bytes(root: &Path, id: &str, consumes_store: bool, bytes: &str) {
    let artifact = root.join(format!("{id}.js"));
    fs::write(&artifact, bytes).unwrap();
    let mut contract = PluginContract::new(format!("company.{id}"), "1.0.0", id)
        .with_authoring_version(2)
        .with_configuration_schema(serde_json::json!({"type":"object"}))
        .with_capability(CapabilityEndpointPlan::new(
            format!("company.{id}@1"),
            "1",
            ["get"],
        ));
    if consumes_store {
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
        output: root.join(format!("{id}-bundle")),
    })
    .unwrap();
}

fn cli(root: &Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_lenso"))
        .current_dir(root)
        .env("LENSO_HOST_JS_RUNTIME", "node")
        .env(
            "LENSO_HOST_EXTRACTOR",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/host-extract.js"),
        )
        .env(
            "LENSO_HOST_DISTRIBUTION_LIB",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("bin"),
        )
        .args(arguments)
        .output()
        .unwrap()
}

fn distribution_target() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else {
        "x86_64-unknown-linux-gnu"
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one end-to-end CLI authority and distribution journey"
)]
fn ts_host_cli_build_check_show_and_rejection_use_the_same_authority() {
    let root = tempfile::tempdir().unwrap();
    fixture_bundle(root.path(), "store", false);
    fixture_bundle(root.path(), "notes", true);
    fs::write(root.path().join("store.ts"), "import { pluginBundle } from '@lenso/cli/host'; export default pluginBundle('./store-bundle');").unwrap();
    fs::write(root.path().join("app.ts"), "import { defineHost, pluginBundle } from '@lenso/cli/host'; import store from './store'; export default defineHost({ id: 'company.app', plugins: [store, pluginBundle('./notes-bundle')] });").unwrap();
    let built = cli(
        root.path(),
        &[
            "app",
            "build",
            "--source",
            "app.ts",
            "--target",
            distribution_target(),
            "--out",
            "build",
        ],
    );
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    // The first application workflow can run the same shipped extractor on Bun.
    let bun_build = Command::new(env!("CARGO_BIN_EXE_lenso"))
        .current_dir(root.path())
        .env("LENSO_HOST_JS_RUNTIME", "bun")
        .env(
            "LENSO_HOST_EXTRACTOR",
            Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/host-extract.js"),
        )
        .args([
            "app",
            "build",
            "--source",
            "app.ts",
            "--target",
            distribution_target(),
            "--out",
            "build-bun",
        ])
        .output()
        .unwrap();
    assert!(
        bun_build.status.success(),
        "{}",
        String::from_utf8_lossy(&bun_build.stderr)
    );
    let show = cli(root.path(), &["app", "show", "--root", "build", "--json"]);
    assert!(
        show.status.success(),
        "{}",
        String::from_utf8_lossy(&show.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(report["instances"].as_array().unwrap().len(), 2);
    assert_eq!(report["bindings"].as_array().unwrap().len(), 1);
    assert!(
        cli(root.path(), &["app", "check", "--root", "build"])
            .status
            .success()
    );
    let app_root = root.path().join("app-root");
    fs::create_dir(&app_root).unwrap();
    let runtime_resolution = cli(
        root.path(),
        &[
            "app",
            "show",
            "--root",
            "app-root",
            "--host-build",
            "build/.lenso/host-build.json",
            "--runtime-json",
        ],
    );
    assert!(
        runtime_resolution.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime_resolution.stderr)
    );
    let runtime_resolution: serde_json::Value =
        serde_json::from_slice(&runtime_resolution.stdout).unwrap();
    assert_eq!(
        runtime_resolution["schema"],
        "lenso.runtime-app-resolution.v1"
    );
    assert_eq!(runtime_resolution["app_id"], "company.app");
    assert_eq!(
        runtime_resolution["plan"]["plugin_instances"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for digest in ["authority_digest", "host_build_digest"] {
        let digest = runtime_resolution[digest].as_str().unwrap();
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), 71);
    }
    fs::create_dir(app_root.join(".lenso")).unwrap();
    fs::write(app_root.join(".lenso/host-build.json"), b"{}\n").unwrap();
    let competing_authority = cli(
        root.path(),
        &[
            "app",
            "show",
            "--root",
            "app-root",
            "--host-build",
            "build/.lenso/host-build.json",
            "--runtime-json",
        ],
    );
    assert!(!competing_authority.status.success());
    assert!(
        String::from_utf8_lossy(&competing_authority.stderr)
            .contains("cannot replace distribution Host authority")
    );
    fs::write(
        root.path().join("NOTICES.txt"),
        "fixture redistribution notices",
    )
    .unwrap();
    let executable = env!("CARGO_BIN_EXE_lenso");
    let prepared = cli(
        root.path(),
        &[
            "app",
            "prepare",
            "--build",
            "build",
            "--target",
            distribution_target(),
            "--runtime",
            executable,
            "--owner",
            executable,
            "--resolver",
            executable,
            "--bun",
            executable,
            "--notices",
            "NOTICES.txt",
            "--out",
            "distribution",
        ],
    );
    assert!(
        prepared.status.success(),
        "{}",
        String::from_utf8_lossy(&prepared.stderr)
    );
    let verified = Command::new("node")
        .arg("-e")
        .arg("require('./distribution/host.js').verifyDistribution('./distribution')")
        .current_dir(root.path())
        .status()
        .unwrap();
    assert!(verified.success());
    fs::write(root.path().join("distribution/bundles.json"), "tampered").unwrap();
    let rejected_distribution = Command::new("node")
        .arg("-e")
        .arg("require('./distribution/host.js').verifyDistribution('./distribution')")
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(!rejected_distribution.status.success());
    assert!(String::from_utf8_lossy(&rejected_distribution.stderr).contains("failed integrity"));
    let extra = root.path().join("build/plugins/company.store");
    fs::create_dir_all(&extra).unwrap();
    fs::write(extra.join("extra.toml"), "").unwrap();
    let rejected = cli(root.path(), &["app", "check", "--root", "build"]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("admission denied"));
    fs::write(root.path().join("app.ts"), "import { defineHost } from '@lenso/cli/host'; export default defineHost({ id: 'company.app', plugins: process.env.PLUGINS });").unwrap();
    let rejected = cli(
        root.path(),
        &[
            "app",
            "build",
            "--source",
            "app.ts",
            "--target",
            "javascript-bun",
            "--out",
            "rejected",
        ],
    );
    assert!(!rejected.status.success());
    assert!(!root.path().join("rejected").exists());
}

#[test]
fn ts_host_extension_install_and_configuration_obey_exact_release_and_host_ceiling() {
    let root = tempfile::tempdir().unwrap();
    fixture_bundle(root.path(), "store", false);
    fs::write(root.path().join("app.ts"), r"
        import { defineHost, pluginBundle as packed } from '@lenso/cli/host';
        export default defineHost({ id: 'company.app', plugins: [], slots: [{
            id: 'store', cardinality: 'many', maxInstances: 1,
            allow: [packed('./store-bundle')],
            configurationSchema: { type: 'object', properties: { limit: { type:'integer', maximum:8 } } }
        }] });
    ").unwrap();
    let output = cli(
        root.path(),
        &[
            "app",
            "build",
            "--source",
            "app.ts",
            "--target",
            "javascript-bun",
            "--out",
            "build",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let added = cli(
        root.path(),
        &["plugins", "add", "store-bundle", "--root", "build"],
    );
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    let build = root.path().join("build");
    assert!(
        lenso_app_authoring::load_resolved_app(&build)
            .unwrap()
            .instances()
            .is_empty()
    );
    lenso_app_authoring::configure_instance(&build, "company.store", "a", b"limit = 8\n").unwrap();
    let config = build.join("plugins/company.store/a.toml");
    assert!(
        lenso_app_authoring::configure_instance(&build, "company.store", "a", b"limit = 9\n")
            .unwrap_err()
            .to_string()
            .contains("configuration ceiling")
    );
    assert_eq!(fs::read(&config).unwrap(), b"limit = 8\n");
    assert!(
        lenso_app_authoring::configure_instance(&build, "company.store", "b", b"limit = 4\n")
            .unwrap_err()
            .to_string()
            .contains("maxInstances")
    );
    assert!(!build.join("plugins/company.store/b.toml").exists());
    assert!(
        cli(root.path(), &["app", "check", "--root", "build"])
            .status
            .success()
    );

    // A separately valid bundle with the SAME ID/version/contract but different code
    // must not inherit the admitted bundle's authority.
    let other = root.path().join("other");
    fs::create_dir(&other).unwrap();
    fixture_bundle_bytes(
        &other,
        "store",
        false,
        "throw new Error('different, unadmitted implementation');",
    );
    let error = lenso_app_authoring::prepare_bundle_mutation(
        &build,
        &other.join("store-bundle"),
        lenso_app_authoring::BundleMutation::Replace,
    )
    .unwrap_err();
    assert!(error.to_string().contains("not an exact admitted release"));
    assert_eq!(
        lenso_app_authoring::load_resolved_app(&build)
            .unwrap()
            .instances()
            .len(),
        1
    );
}
