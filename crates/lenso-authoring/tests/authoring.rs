use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use lenso_authoring::{
    AddModule, Binding, CapabilityEndpoint, CapabilityRequirement, CheckOptions, ContractInput,
    ExecutionLane, Module, ModuleRole, PackageInput, PackageSource, ProjectAuthoring, ProjectFile,
    ProjectPath, RequestAdmission, ResolutionOptions, ResolvedProject, WebProfile, run_project,
};
use lenso_bun_adapter::{BunAdapter, BunAdapterConfig, BunWire};
use lenso_kernel::{ExecutionAdapterCatalog, TerminalOutcome};
use lenso_native_adapter::NativeModuleRegistry;
use lenso_native_greeter::GreeterFactory;

fn package(logical: &str, package_name: &str, version: &str) -> PackageInput {
    PackageInput::new(logical, PackageSource::Cargo, version)
        .with_package_name(package_name)
        .with_manifest("Cargo.toml")
}

fn write_cargo_inputs(root: &Path, packages: &[(&str, &str)]) {
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"authoring-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n",
    )
    .unwrap();
    let mut lock = "version = 4\n".to_owned();
    for (name, version) in packages {
        lock.push_str(&format!(
            "\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n"
        ));
    }
    fs::write(root.join("Cargo.lock"), lock).unwrap();
}

fn add_package(project: &mut ProjectFile, input: PackageInput) {
    project
        .packages_mut()
        .insert(input.name().to_owned(), input);
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn add_greeting_contract(project: &mut ProjectFile, root: &Path) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    copy_tree(
        &workspace.join("crates/lenso-capability-greeting"),
        &root.join("contract/greeting"),
    );
    project.contracts_mut().push(ContractInput::new(
        "example.greeting@1",
        "1.0.0",
        "contract/greeting/capability.json",
        "contract/greeting/src/generated.rs",
        "contract/greeting/generated/bindings.ts",
    ));
}

fn add_web_contracts(project: &mut ProjectFile, root: &Path) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (source, target, capability_id) in [
        (
            "lenso-capability-ui-contribution",
            "ui-contribution",
            "lenso.ui.contribution@1",
        ),
        (
            "lenso-capability-web-shell",
            "web-shell",
            "lenso.web.shell@1",
        ),
    ] {
        copy_tree(
            &workspace.join("crates").join(source),
            &root.join("contract").join(target),
        );
        project.contracts_mut().push(ContractInput::new(
            capability_id,
            "1.0.0",
            format!("contract/{target}/capability.json"),
            format!("contract/{target}/src/generated.rs"),
            format!("contract/{target}/generated/bindings.ts"),
        ));
    }
}

#[test]
fn resolved_plan_is_the_only_canonical_execution_document() {
    let root = tempfile_dir();
    write_cargo_inputs(&root, &[("example-a", "1.0.0"), ("example-b", "2.0.0")]);
    let mut first = ProjectFile::default();
    add_package(&mut first, package("a", "example-a", "1.0.0"));
    add_package(&mut first, package("b", "example-b", "2.0.0"));
    first.composition_mut().add_module(Module::new("z", "b"));
    first.composition_mut().add_module(Module::new("a", "a"));
    let mut second = first.clone();
    second.composition_mut().modules_mut().reverse();

    let first = first.resolve(&root, &ResolutionOptions::default()).unwrap();
    let second = second
        .resolve(&root, &ResolutionOptions::default())
        .unwrap();
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
    let loaded = ResolvedProject::from_canonical_bytes(first.canonical_bytes()).unwrap();
    assert_eq!(loaded.plan(), first.plan());
    assert_eq!(
        loaded.plan().module_instances()[0].package_revision(),
        "1.0.0"
    );
}

#[test]
fn meaningful_binding_changes_produce_a_plan_diff() {
    let root = tempfile_dir();
    write_cargo_inputs(
        &root,
        &[
            ("provider-a", "1.0.0"),
            ("provider-b", "1.0.0"),
            ("consumer", "1.0.0"),
        ],
    );
    let mut first = ProjectFile::default();
    for (logical, actual) in [("a", "provider-a"), ("b", "provider-b"), ("c", "consumer")] {
        add_package(&mut first, package(logical, actual, "1.0.0"));
    }
    add_greeting_contract(&mut first, &root);
    for (key, package) in [("a", "a"), ("b", "b")] {
        first
            .composition_mut()
            .add_module(
                Module::new(key, package).with_capability(CapabilityEndpoint::request(
                    "example.greeting@1",
                    "1.0.0",
                    ["greet"],
                )),
            );
    }
    first.composition_mut().add_module(
        Module::new("consumer", "c")
            .with_requirement(CapabilityRequirement::one("example.greeting@1", "1.0.0")),
    );
    first.composition_mut().add_binding(Binding::new(
        "consumer",
        "example.greeting@1",
        "1.0.0",
        "a",
    ));
    let mut second = first.clone();
    second.composition_mut().bindings_mut().clear();
    second.composition_mut().add_binding(
        Binding::new("consumer", "example.greeting@1", "1.0.0", "b")
            .with_admission(RequestAdmission::new(3, 2)),
    );
    assert_ne!(
        first
            .resolve(&root, &ResolutionOptions::default())
            .unwrap()
            .canonical_bytes(),
        second
            .resolve(&root, &ResolutionOptions::default())
            .unwrap()
            .canonical_bytes()
    );
}

#[test]
fn authoring_materializes_lane_placement_and_descriptor_transfer_support() {
    let root = tempfile_dir();
    write_cargo_inputs(&root, &[("provider", "1.0.0"), ("consumer", "1.0.0")]);
    let mut project = ProjectFile::default();
    add_package(&mut project, package("provider", "provider", "1.0.0"));
    add_package(&mut project, package("consumer", "consumer", "1.0.0"));
    add_greeting_contract(&mut project, &root);
    project
        .composition_mut()
        .add_execution_lane(ExecutionLane::new("frontend"));
    project
        .composition_mut()
        .add_execution_lane(ExecutionLane::new("workers"));
    project.composition_mut().add_module(
        Module::new("consumer", "consumer")
            .with_execution_lane("frontend")
            .with_requirement(CapabilityRequirement::one("example.greeting@1", "1.0.0")),
    );
    project.composition_mut().add_module(
        Module::new("provider", "provider")
            .with_execution_lane("workers")
            .with_capability(CapabilityEndpoint::request(
                "example.greeting@1",
                "1.0.0",
                ["greet"],
            )),
    );
    project.composition_mut().add_binding(Binding::new(
        "consumer",
        "example.greeting@1",
        "1.0.0",
        "provider",
    ));

    let resolved = project
        .resolve(&root, &ResolutionOptions::default())
        .expect("Descriptor-declared transfer should permit cross-lane placement");

    assert_eq!(resolved.plan().execution_lanes().len(), 2);
    assert_eq!(
        resolved
            .plan()
            .module_instance("provider")
            .expect("provider should resolve")
            .execution_lane()
            .as_str(),
        "workers"
    );
    assert!(
        resolved
            .plan()
            .module_instance("provider")
            .expect("provider should resolve")
            .provided_capabilities()[0]
            .supports_cross_lane_transfer()
    );
}

#[test]
fn package_manager_lockfile_is_required_and_authoritative() {
    let root = tempfile_dir();
    write_cargo_inputs(&root, &[("real-package", "1.0.0")]);
    let mut project = ProjectFile::default();
    add_package(&mut project, package("runtime-id", "real-package", "2.0.0"));
    project
        .composition_mut()
        .add_module(Module::new("module", "runtime-id"));
    let error = project
        .resolve(&root, &ResolutionOptions::default())
        .unwrap_err();
    assert!(error.to_string().contains("does not lock real-package"));
}

#[test]
fn every_capability_requires_a_matching_fresh_descriptor() {
    let root = tempfile_dir();
    write_cargo_inputs(&root, &[("example-greeter", "1.0.0")]);
    let mut project = ProjectFile::default();
    add_package(
        &mut project,
        package("example.greeter", "example-greeter", "1.0.0"),
    );
    project.composition_mut().add_module(
        Module::new("greeter", "example.greeter").with_capability(CapabilityEndpoint::request(
            "example.greeting@1",
            "1.0.0",
            ["greet"],
        )),
    );
    let error = project.check(&root, &CheckOptions::default()).unwrap_err();
    assert!(error.to_string().contains("without a Descriptor input"));
    add_greeting_contract(&mut project, &root);
    project.check(&root, &CheckOptions::default()).unwrap();

    let mut mismatch = project.clone();
    mismatch.composition_mut().modules_mut()[0] = Module::new("greeter", "example.greeter")
        .with_capability(CapabilityEndpoint::request(
            "example.greeting@1",
            "1.0.0",
            ["not_greet"],
        ));
    let error = mismatch.check(&root, &CheckOptions::default()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("operations do not match Descriptor")
    );
}

#[test]
fn sensitive_configuration_requires_an_explicit_secret_reference() {
    let root = tempfile_dir();
    write_cargo_inputs(&root, &[("configured", "1.0.0")]);
    fs::write(
        root.join("config.schema.json"),
        r#"{"type":"object","properties":{"database_url":{"x-lenso-sensitive":true},"token_bucket_capacity":{"type":"integer"}},"additionalProperties":false}"#,
    )
    .unwrap();
    let mut project = ProjectFile::default();
    add_package(&mut project, package("configured", "configured", "1.0.0"));
    project.composition_mut().add_module(
        Module::new("configured", "configured")
            .with_configuration_schema("config.schema.json")
            .with_configuration(serde_json::json!({
                "database_url":"postgres://secret",
                "token_bucket_capacity": 8
            })),
    );
    assert!(project.check(&root, &CheckOptions::default()).is_err());
    project.composition_mut().modules_mut()[0].set_configuration(serde_json::json!({
        "database_url":{"secret_ref":"DATABASE_URL"},
        "token_bucket_capacity": 8
    }));
    project.check(&root, &CheckOptions::default()).unwrap();
}

#[test]
fn web_profile_requires_explicit_shell_adapter_and_contribution_roles() {
    let root = tempfile_dir();
    write_cargo_inputs(
        &root,
        &[
            ("shell", "1.0.0"),
            ("browser", "1.0.0"),
            ("orders-ui", "1.0.0"),
        ],
    );
    let mut project = ProjectFile::default();
    for name in ["shell", "browser", "orders-ui"] {
        add_package(&mut project, package(name, name, "1.0.0"));
    }
    add_web_contracts(&mut project, &root);
    project.composition_mut().add_module(
        Module::new("shell", "shell")
            .with_role(ModuleRole::WebShell)
            .with_capability(CapabilityEndpoint::request(
                "lenso.web.shell@1",
                "1.0.0",
                ["read_asset", "render_route"],
            ))
            .with_requirement(CapabilityRequirement::many(
                "lenso.ui.contribution@1",
                "1.0.0",
            )),
    );
    project.composition_mut().add_module(
        Module::new("browser", "browser")
            .with_role(ModuleRole::BrowserAdapter)
            .with_requirement(CapabilityRequirement::one("lenso.web.shell@1", "1.0.0")),
    );
    project.composition_mut().add_module(
        Module::new("orders-ui", "orders-ui")
            .with_role(ModuleRole::UiContribution)
            .with_capability(CapabilityEndpoint::request(
                "lenso.ui.contribution@1",
                "1.0.0",
                ["describe"],
            )),
    );
    project.composition_mut().add_binding(Binding::new(
        "shell",
        "lenso.ui.contribution@1",
        "1.0.0",
        "orders-ui",
    ));
    project.composition_mut().add_binding(Binding::new(
        "browser",
        "lenso.web.shell@1",
        "1.0.0",
        "shell",
    ));
    project.profiles_mut().insert(
        "web".to_owned(),
        WebProfile::new("shell", "browser").with_ui_contribution("orders-ui"),
    );
    let plan = project
        .resolve(&root, &ResolutionOptions::default().with_profile("web"))
        .unwrap();
    assert_eq!(plan.plan().module_instances().len(), 3);

    let mut without_ui = project.clone();
    without_ui
        .profiles_mut()
        .insert("web".to_owned(), WebProfile::new("shell", "browser"));
    let non_ui_plan = without_ui
        .resolve(&root, &ResolutionOptions::default().with_profile("web"))
        .unwrap();
    assert_ne!(plan.canonical_bytes(), non_ui_plan.canonical_bytes());
    assert_ne!(
        serde_json::to_vec_pretty(&project).unwrap(),
        serde_json::to_vec_pretty(&without_ui).unwrap()
    );
    assert!(non_ui_plan.plan().module_instance("orders-ui").is_none());

    let mut invalid = project.clone();
    invalid.composition_mut().modules_mut()[0] = Module::new("shell", "shell")
        .with_role(ModuleRole::WebShell)
        .with_capability(CapabilityEndpoint::request(
            "lenso.web.shell@1",
            "1.0.0",
            ["read_asset", "render_route"],
        ));
    let error = invalid
        .resolve(&root, &ResolutionOptions::default().with_profile("web"))
        .expect_err("Web Shell must bind many UI Contributions");
    assert!(
        error
            .to_string()
            .contains("must require many lenso.ui.contribution@1")
    );
}

#[test]
fn add_updates_composition_and_package_manifest_as_reviewable_files() {
    let root = tempfile_dir();
    let project_path = root.join("lenso.json");
    fs::write(
        &project_path,
        serde_json::to_vec_pretty(&ProjectFile::default()).unwrap(),
    )
    .unwrap();
    fs::write(root.join("Cargo.toml"), "[dependencies]\n").unwrap();
    let result = ProjectPath::new(&project_path)
        .add(&AddModule::new(
            Module::new("greeter", "runtime.greeter"),
            package("runtime.greeter", "lenso-native-greeter", "0.1.0"),
        ))
        .unwrap();
    assert_eq!(result.changed_files().len(), 2);
    assert!(
        fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains("lenso-native-greeter = \"0.1.0\"")
    );
}

#[test]
fn native_clean_project_add_resolve_reload_and_run_use_the_same_plan() {
    let root = tempfile_dir();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = workspace.join("fixtures/vnext-native-greeter");
    fs::write(
        root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"clean-native-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n\n[patch.crates-io]\nlenso-native-greeter = {{ path = \"{}\" }}\n",
            fixture.display()
        ),
    )
    .unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let mut clean = ProjectFile::default();
    add_greeting_contract(&mut clean, &root);
    let project_path = root.join("lenso.json");
    fs::write(&project_path, serde_json::to_vec_pretty(&clean).unwrap()).unwrap();
    ProjectPath::new(&project_path)
        .add(&AddModule::new(
            Module::new("greeter", "example.native-greeter").with_capability(
                CapabilityEndpoint::request("example.greeting@1", "1.0.0", ["greet"]),
            ),
            package("example.native-greeter", "lenso-native-greeter", "0.1.0"),
        ))
        .unwrap();
    let cargo = std::env::var_os("CARGO").map_or_else(|| PathBuf::from("cargo"), PathBuf::from);
    assert!(
        Command::new(cargo)
            .args(["generate-lockfile", "--offline"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let project = ProjectPath::load(&project_path).unwrap();
    let resolved = project
        .resolve(&root, &ResolutionOptions::default())
        .unwrap();
    let plan_path = root.join("resolved-plan.json");
    fs::write(&plan_path, resolved.canonical_bytes()).unwrap();
    let loaded = ResolvedProject::from_canonical_bytes(&fs::read(plan_path).unwrap()).unwrap();
    let driver = lenso_runner::TokioDriver::new();
    driver.request_shutdown();
    let adapters =
        ExecutionAdapterCatalog::single(NativeModuleRegistry::new().with_factory(GreeterFactory));
    let outcome = run_on_local_set(run_project(
        &loaded,
        driver,
        adapters,
        std::time::Duration::from_secs(1),
    ))
    .unwrap();
    assert!(matches!(outcome, TerminalOutcome::CleanShutdown));
}

#[test]
#[ignore = "requires Bun; CI runs this exact clean-project production-wire test"]
fn bun_clean_project_add_resolve_reload_and_run_use_the_same_plan() {
    let root = tempfile_dir();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = "fixtures/bun/request-provider.ts";
    let fixture_path = root.join(fixture);
    fs::create_dir_all(fixture_path.parent().unwrap()).unwrap();
    fs::copy(workspace.join(fixture), &fixture_path).unwrap();
    for relative in [
        "crates/lenso-capability-greeting/generated/bindings.ts",
        "crates/lenso-capability-secure-greeting/generated/bindings.ts",
        "crates/lenso-auth-sdk/typescript/actor.ts",
        "crates/lenso-otel-module/typescript/trace-context.ts",
    ] {
        let destination = root.join(relative);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(workspace.join(relative), destination).unwrap();
    }
    fs::create_dir_all(root.join("module")).unwrap();
    fs::write(
        root.join("module/package.json"),
        r#"{"name":"example-bun-module","version":"1.0.0"}"#,
    )
    .unwrap();
    fs::write(
        root.join("package.json"),
        r#"{"name":"clean-bun-app","version":"1.0.0","dependencies":{}}"#,
    )
    .unwrap();
    let project_path = root.join("lenso.json");
    fs::write(
        &project_path,
        serde_json::to_vec_pretty(&ProjectFile::default()).unwrap(),
    )
    .unwrap();
    let input = PackageInput::new("example.bun", PackageSource::Bun, "file:./module")
        .with_package_name("example-bun-module")
        .with_locked_revision("module")
        .with_manifest("package.json");
    ProjectPath::new(&project_path)
        .add(&AddModule::new(
            Module::new("bun", "example.bun").with_entrypoint(fixture),
            input,
        ))
        .unwrap();
    assert!(
        Command::new("bun")
            .args(["install", "--lockfile-only", "--ignore-scripts"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let project = ProjectPath::load(&project_path).unwrap();
    let resolved = project
        .resolve(
            &root,
            &ResolutionOptions::default()
                .with_check_options(CheckOptions::new(["lenso.bun-process@1"])),
        )
        .unwrap();
    let loaded = ResolvedProject::from_canonical_bytes(resolved.canonical_bytes()).unwrap();
    let driver = lenso_runner::TokioDriver::new();
    driver.request_shutdown();
    let config = BunAdapterConfig::new("bun", BunWire::JsonRpcHttp).with_working_directory(&root);
    let adapters =
        ExecutionAdapterCatalog::single(BunAdapter::production("bun").with_config(config));
    let outcome = run_on_local_set(run_project(
        &loaded,
        driver,
        adapters,
        std::time::Duration::from_secs(3),
    ))
    .unwrap();
    assert!(matches!(outcome, TerminalOutcome::CleanShutdown));
}

fn run_on_local_set<F: std::future::Future>(future: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, future)
}

fn tempfile_dir() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "lenso-authoring-test-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}
