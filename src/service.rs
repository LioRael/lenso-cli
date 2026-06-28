use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{ServiceCreateArgs, ServiceDevArgs, ServiceLanguage, ServicePackageArgs, host, module};

type PendingWrites = BTreeMap<PathBuf, String>;
const LOCAL_SERVICE_BASE_URL: &str = "http://127.0.0.1:4100/lenso/service/v1";

#[derive(Debug, Clone)]
pub(crate) struct ServiceCreateOptions {
    pub(crate) dry_run: bool,
    pub(crate) lang: ServiceLanguage,
    pub(crate) name: String,
    pub(crate) output_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct ServiceDevOptions {
    pub(crate) module_services_file: Option<PathBuf>,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) separate_worker: bool,
    pub(crate) skip_db: bool,
    pub(crate) skip_migrate: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ServicePackageOptions {
    pub(crate) check: bool,
    pub(crate) json: bool,
    pub(crate) manifest: String,
    pub(crate) output_dir: PathBuf,
    pub(crate) service_dir: PathBuf,
}

impl From<&ServiceCreateArgs> for ServiceCreateOptions {
    fn from(args: &ServiceCreateArgs) -> Self {
        Self {
            dry_run: args.dry_run,
            lang: args.lang,
            name: args.name.clone(),
            output_dir: args.output_dir.clone(),
        }
    }
}

impl From<&ServiceDevArgs> for ServiceDevOptions {
    fn from(args: &ServiceDevArgs) -> Self {
        Self {
            module_services_file: args.module_services_file.clone(),
            repo_root: args.repo_root.clone(),
            separate_worker: args.separate_worker,
            skip_db: args.skip_db,
            skip_migrate: args.skip_migrate,
        }
    }
}

impl From<&ServicePackageArgs> for ServicePackageOptions {
    fn from(args: &ServicePackageArgs) -> Self {
        Self {
            check: args.check,
            json: args.json,
            manifest: args.manifest.clone(),
            output_dir: args.output_dir.clone(),
            service_dir: args.service_dir.clone(),
        }
    }
}

pub(crate) fn create_service(options: ServiceCreateOptions) -> Result<()> {
    match options.lang {
        ServiceLanguage::Rust => create_rust_service(options),
        ServiceLanguage::Ts => create_ts_service(options),
    }
}

pub(crate) async fn dev_service(options: ServiceDevOptions) -> Result<()> {
    module::start_declared_module_services(
        options.repo_root.as_deref(),
        options.module_services_file.as_deref(),
    )
    .await?;
    host::serve(
        options.repo_root.as_deref(),
        options.skip_db,
        options.skip_migrate,
        options.separate_worker,
    )
    .await
}

pub(crate) async fn package_service(options: ServicePackageOptions) -> Result<()> {
    let plan = service_package_plan(&options).await?;
    if !options.check {
        let manifest_source =
            serde_json::to_string_pretty(&plan.manifest).context("serialize service manifest")?;
        let package_source =
            serde_json::to_string_pretty(&service_package_manifest(&plan.metadata))
                .context("serialize service package manifest")?;
        write_file(
            &plan.service_manifest_output,
            format!("{manifest_source}\n").as_bytes(),
        )?;
        write_file(
            &plan.package_manifest_output,
            format!("{package_source}\n").as_bytes(),
        )?;
        for release in &plan.module_release_outputs {
            let release_source = serde_json::to_string_pretty(&module_release_manifest(
                &plan.metadata,
                &release.module,
            ))
            .context("serialize module release manifest")?;
            write_file(&release.path, format!("{release_source}\n").as_bytes())?;
        }
    }
    print_service_package_report(&plan, options.check, options.json)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServicePackageModuleMetadata {
    capabilities: Vec<String>,
    dependencies: Vec<String>,
    name: String,
    version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ServicePackageMetadata {
    modules: Vec<ServicePackageModuleMetadata>,
    name: String,
    version: String,
}

#[derive(Debug)]
struct ModuleReleaseOutput {
    module: ServicePackageModuleMetadata,
    path: PathBuf,
}

#[derive(Debug)]
struct ServicePackagePlan {
    manifest: Value,
    manifest_reference: String,
    metadata: ServicePackageMetadata,
    module_release_outputs: Vec<ModuleReleaseOutput>,
    package_dir: PathBuf,
    package_manifest_output: PathBuf,
    service_manifest_output: PathBuf,
}

async fn service_package_plan(options: &ServicePackageOptions) -> Result<ServicePackagePlan> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let service_dir = absolutize_from(&current_dir, &options.service_dir);
    if !service_dir.is_dir() {
        bail!(
            "Service provider directory does not exist: {}",
            service_dir.display()
        );
    }
    let (manifest, manifest_reference) =
        read_service_package_manifest(&options.manifest, &service_dir).await?;
    let metadata = service_package_metadata(&manifest)?;
    let output_dir = absolutize_from(&service_dir, &options.output_dir);
    let package_dir = output_dir.join(&metadata.name);
    let module_release_outputs = metadata
        .modules
        .iter()
        .map(|module| {
            Ok(ModuleReleaseOutput {
                module: module.clone(),
                path: package_dir
                    .join("modules")
                    .join(module_release_path_segment(&module.name)?)
                    .join("lenso.module-release.json"),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(ServicePackagePlan {
        manifest,
        manifest_reference,
        package_manifest_output: package_dir.join("lenso.service-package.json"),
        service_manifest_output: package_dir.join("lenso.service.json"),
        module_release_outputs,
        package_dir,
        metadata,
    })
}

async fn read_service_package_manifest(
    reference: &str,
    service_dir: &Path,
) -> Result<(Value, String)> {
    if is_http_reference(reference) {
        let response = reqwest::get(reference)
            .await
            .with_context(|| format!("fetch service manifest {reference}"))?
            .error_for_status()
            .with_context(|| format!("fetch service manifest {reference}"))?;
        let manifest = response
            .json::<Value>()
            .await
            .with_context(|| format!("parse service manifest {reference}"))?;
        return Ok((manifest, reference.to_owned()));
    }

    let manifest_path = absolutize_from(service_dir, Path::new(reference));
    let manifest_source = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read service manifest {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&manifest_source)
        .with_context(|| format!("parse service manifest {}", manifest_path.display()))?;
    Ok((manifest, path_string(&manifest_path)))
}

fn is_http_reference(reference: &str) -> bool {
    reference.starts_with("http://") || reference.starts_with("https://")
}

fn service_package_metadata(manifest: &Value) -> Result<ServicePackageMetadata> {
    if !manifest.is_object() {
        bail!("Service manifest must be a JSON object");
    }
    let name = required_manifest_string(manifest, "name", "Service manifest name")?;
    let version = required_manifest_string(manifest, "version", "Service manifest version")?;
    let Some(raw_modules) = manifest.get("modules").and_then(Value::as_array) else {
        bail!("Service manifest modules must be an array");
    };
    if raw_modules.is_empty() {
        bail!("Service manifest modules must not be empty");
    }

    let mut seen_modules = BTreeSet::new();
    let mut modules = Vec::new();
    for module in raw_modules {
        if !module.is_object() {
            bail!("Service manifest module entries must be objects");
        }
        let module_name = required_manifest_string(module, "name", "Service manifest module name")?;
        if !seen_modules.insert(module_name.to_owned()) {
            bail!("Service manifest module `{module_name}` is declared more than once");
        }
        let version = module
            .get("version")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(version)
            .to_owned();
        modules.push(ServicePackageModuleMetadata {
            capabilities: optional_manifest_string_array(module, "capabilities")?,
            dependencies: optional_manifest_string_array(module, "dependencies")?,
            name: module_name.to_owned(),
            version,
        });
    }

    Ok(ServicePackageMetadata {
        modules,
        name: name.to_owned(),
        version: version.to_owned(),
    })
}

fn optional_manifest_string_array(value: &Value, field: &str) -> Result<Vec<String>> {
    let Some(raw_values) = value.get(field) else {
        return Ok(Vec::new());
    };
    let Some(values) = raw_values.as_array() else {
        bail!("Service manifest module {field} must be an array");
    };
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    anyhow::anyhow!("Service manifest module {field}[{index}] is required")
                })
        })
        .collect()
}

fn required_manifest_string<'a>(value: &'a Value, field: &str, label: &str) -> Result<&'a str> {
    let Some(raw_value) = value.get(field).and_then(Value::as_str) else {
        bail!("{label} is required");
    };
    let value = raw_value.trim();
    if value.is_empty() {
        bail!("{label} is required");
    }
    Ok(value)
}

fn service_package_manifest(metadata: &ServicePackageMetadata) -> Value {
    serde_json::json!({
        "protocol": "lenso.service-package.v1",
        "name": metadata.name,
        "version": metadata.version,
        "serviceManifest": "lenso.service.json",
        "modules": module_names(&metadata.modules),
    })
}

fn module_release_manifest(
    metadata: &ServicePackageMetadata,
    module: &ServicePackageModuleMetadata,
) -> Value {
    serde_json::json!({
        "protocol": "lenso.module-release.v1",
        "name": module.name,
        "version": module.version,
        "source": "service",
        "summary": format!("{} module from {}", module.name, metadata.name),
        "provider": {
            "name": metadata.name,
            "servicePackage": "../../lenso.service-package.json"
        },
        "capabilities": module.capabilities,
        "dependencies": module.dependencies,
    })
}

fn module_names(modules: &[ServicePackageModuleMetadata]) -> Vec<String> {
    modules.iter().map(|module| module.name.clone()).collect()
}

fn module_release_path_segment(module_name: &str) -> Result<&str> {
    if module_name.contains('/')
        || module_name.contains('\\')
        || module_name == "."
        || module_name == ".."
    {
        bail!("Service manifest module `{module_name}` cannot be used as a release path");
    }
    Ok(module_name)
}

fn print_service_package_report(plan: &ServicePackagePlan, check: bool, json: bool) -> Result<()> {
    if json {
        let status = if check { "checked" } else { "packaged" };
        let mut files = vec![
            path_string(&plan.service_manifest_output),
            path_string(&plan.package_manifest_output),
        ];
        files.extend(
            plan.module_release_outputs
                .iter()
                .map(|release| path_string(&release.path)),
        );
        let report = serde_json::json!({
            "status": status,
            "name": plan.metadata.name,
            "version": plan.metadata.version,
            "modules": module_names(&plan.metadata.modules),
            "manifestReference": plan.manifest_reference,
            "packageDir": path_string(&plan.package_dir),
            "moduleReleases": plan.module_release_outputs.iter().map(|release| serde_json::json!({
                "module": release.module.name,
                "path": path_string(&release.path),
            })).collect::<Vec<_>>(),
            "files": files,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&report).context("serialize service package report")?
        );
        return Ok(());
    }

    if check {
        println!("Service package check ok.");
    } else {
        println!(
            "Packaged service {}@{}.",
            plan.metadata.name, plan.metadata.version
        );
    }
    println!(
        "- modules: {}",
        module_names(&plan.metadata.modules).join(", ")
    );
    println!("- manifest: {}", plan.manifest_reference);
    println!("- package: {}", plan.package_dir.display());
    if !check {
        println!("- wrote: {}", plan.service_manifest_output.display());
        println!("- wrote: {}", plan.package_manifest_output.display());
        for release in &plan.module_release_outputs {
            println!("- wrote: {}", release.path.display());
        }
    }
    Ok(())
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn create_ts_service(options: ServiceCreateOptions) -> Result<()> {
    let scaffold = service_scaffold(&options)?;
    let mut pending_writes = PendingWrites::new();
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("package.json"),
        include_str!("../templates/service-ts/package.json.tmpl"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("pnpm-workspace.yaml"),
        include_str!("../templates/service-ts/pnpm-workspace.yaml.tmpl"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("src/server.ts"),
        include_str!("../templates/service-ts/src/server.ts"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("src/service.ts"),
        include_str!("../templates/service-ts/src/service.ts"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("lenso.service.json"),
        include_str!("../templates/service-ts/lenso.service.json.tmpl"),
        &scaffold,
    );
    finish_service_create(&scaffold, pending_writes, options.dry_run, "pnpm install")
}

fn create_rust_service(options: ServiceCreateOptions) -> Result<()> {
    let scaffold = service_scaffold(&options)?;
    let mut pending_writes = PendingWrites::new();
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("Cargo.toml"),
        include_str!("../templates/service-rust/Cargo.toml.tmpl"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("src/main.rs"),
        include_str!("../templates/service-rust/src/main.rs"),
        &scaffold,
    );
    queue_template(
        &mut pending_writes,
        scaffold.target_dir.join("lenso.service.json"),
        include_str!("../templates/service-rust/lenso.service.json.tmpl"),
        &scaffold,
    );
    finish_service_create(&scaffold, pending_writes, options.dry_run, "cargo check")
}

fn finish_service_create(
    scaffold: &ServiceScaffold,
    pending_writes: PendingWrites,
    dry_run: bool,
    check_command: &str,
) -> Result<()> {
    if dry_run {
        println!("Service dry run:");
        for path in pending_writes.keys() {
            println!("- {}", display_relative(&scaffold.output_root, path));
        }
        return Ok(());
    }

    write_pending_files(&pending_writes)?;
    println!("Created service {}.", scaffold.service_name);
    println!("Next steps:");
    println!("- cd {}", scaffold.target_dir_display);
    println!("- {check_command}");
    println!("- lenso service package --check");
    println!(
        "- {}",
        local_service_install_command(&scaffold.repo_root_display)
    );
    if let Some(note) = &scaffold.publish_note {
        println!("- {note}");
    }
    Ok(())
}

#[derive(Debug)]
struct ServiceScaffold {
    crate_name: String,
    lenso_service_dependency: String,
    module_name: String,
    output_root: PathBuf,
    package_name: String,
    pnpm_workspace_overrides: String,
    publish_note: Option<String>,
    remote_module_kit_dependency: String,
    repo_root_display: String,
    service_cwd: String,
    service_kit_dependency: String,
    service_label: String,
    service_name: String,
    target_dir: PathBuf,
    target_dir_display: String,
}

fn service_scaffold(options: &ServiceCreateOptions) -> Result<ServiceScaffold> {
    let service_name = slugify(&options.name);
    if service_name.is_empty() {
        bail!("Service name is required");
    }
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let output_root = options
        .output_dir
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| current_dir.clone());
    let output_root = absolutize_from(&current_dir, &output_root);
    let target_dir = output_root.join(&service_name);
    if target_dir.exists() {
        bail!("Service directory already exists: {}", target_dir.display());
    }
    let module_name = provided_module_name(&service_name);
    let dependencies = service_dependencies();
    Ok(ServiceScaffold {
        crate_name: snake_case(&service_name),
        lenso_service_dependency: dependencies.lenso_service_dependency,
        module_name: module_name.clone(),
        output_root,
        package_name: service_name.clone(),
        pnpm_workspace_overrides: dependencies.pnpm_workspace_overrides,
        publish_note: dependencies.publish_note,
        remote_module_kit_dependency: dependencies.remote_module_kit_dependency,
        repo_root_display: current_dir.to_string_lossy().to_string(),
        service_cwd: json_string(&display_relative(&current_dir, &target_dir)),
        service_kit_dependency: dependencies.service_kit_dependency,
        service_label: label_from_slug(&module_name),
        service_name,
        target_dir_display: display_relative(&current_dir, &target_dir),
        target_dir,
    })
}

fn queue_template(
    pending_writes: &mut PendingWrites,
    path: PathBuf,
    template: &str,
    scaffold: &ServiceScaffold,
) {
    let contents = render_template(template, scaffold);
    pending_writes.insert(path, contents);
}

fn render_template(template: &str, scaffold: &ServiceScaffold) -> String {
    template
        .replace("{{service_name}}", &scaffold.service_name)
        .replace("{{service_label}}", &scaffold.service_label)
        .replace("{{module_name}}", &scaffold.module_name)
        .replace("{{package_name}}", &scaffold.package_name)
        .replace("{{crate_name}}", &scaffold.crate_name)
        .replace(
            "{{service_kit_dependency}}",
            &scaffold.service_kit_dependency,
        )
        .replace(
            "{{remote_module_kit_dependency}}",
            &scaffold.remote_module_kit_dependency,
        )
        .replace("{{service_cwd}}", &scaffold.service_cwd)
        .replace(
            "{{pnpm_workspace_overrides}}",
            &scaffold.pnpm_workspace_overrides,
        )
        .replace(
            "{{lenso_service_dependency}}",
            &scaffold.lenso_service_dependency,
        )
}

#[derive(Debug)]
struct ServiceDependencyPlan {
    lenso_service_dependency: String,
    pnpm_workspace_overrides: String,
    publish_note: Option<String>,
    remote_module_kit_dependency: String,
    service_kit_dependency: String,
}

fn service_dependencies() -> ServiceDependencyPlan {
    let Some(framework_root) = find_framework_root() else {
        return ServiceDependencyPlan {
            lenso_service_dependency: "lenso-service = \"0.1.0\"".to_owned(),
            pnpm_workspace_overrides: String::new(),
            publish_note: Some(
                "@lenso/service-kit and lenso-service must be published, or replace dependencies with local paths.".to_owned(),
            ),
            remote_module_kit_dependency: json_string("0.1.3"),
            service_kit_dependency: json_string("0.1.0"),
        };
    };

    let service_kit = framework_root.join("lenso-runtime-console/packages/service-kit");
    let remote_module_kit = framework_root.join("lenso-runtime-console/packages/remote-module-kit");
    let lenso_service = framework_root.join("lenso/crates/lenso-service");
    ServiceDependencyPlan {
        lenso_service_dependency: format!(
            "lenso-service = {{ path = \"{}\" }}",
            toml_string(&lenso_service)
        ),
        pnpm_workspace_overrides: format!(
            "overrides:\n  \"@lenso/remote-module-kit\": {}\n",
            json_string(&format!("file:{}", remote_module_kit.display()))
        ),
        publish_note: None,
        remote_module_kit_dependency: json_string(&format!("file:{}", remote_module_kit.display())),
        service_kit_dependency: json_string(&format!("file:{}", service_kit.display())),
    }
}

fn find_framework_root() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        if current
            .join("lenso-runtime-console/packages/service-kit")
            .is_dir()
            && current.join("lenso/crates/lenso-service").is_dir()
        {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization is infallible")
}

fn toml_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn local_service_install_command(repo_root: &str) -> String {
    format!(
        "lenso service install ./lenso.service.json --base-url {LOCAL_SERVICE_BASE_URL} --repo-root {}",
        shell_arg(repo_root)
    )
}

fn shell_arg(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "/._-:".contains(character))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn provided_module_name(service_name: &str) -> String {
    service_name
        .strip_suffix("-provider")
        .or_else(|| service_name.strip_suffix("-service"))
        .filter(|name| !name.is_empty())
        .unwrap_or(service_name)
        .to_owned()
}

fn slugify(value: &str) -> String {
    let mut output = String::new();
    let mut last_was_dash = false;
    for character in value.trim().chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            output.push(character);
            last_was_dash = false;
        } else if !last_was_dash && !output.is_empty() {
            output.push('-');
            last_was_dash = true;
        }
    }
    output.trim_matches('-').to_owned()
}

fn snake_case(value: &str) -> String {
    value.replace('-', "_")
}

fn label_from_slug(value: &str) -> String {
    value
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn absolutize_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn display_relative(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn write_pending_files(pending_writes: &PendingWrites) -> Result<()> {
    for (path, contents) in pending_writes {
        write_file(path, contents.as_bytes())?;
    }
    Ok(())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn scaffold() -> ServiceScaffold {
        ServiceScaffold {
            crate_name: "support_suite_provider".to_owned(),
            lenso_service_dependency: "lenso-service = \"0.1.0\"".to_owned(),
            module_name: "support-suite".to_owned(),
            output_root: PathBuf::from("/tmp/services"),
            package_name: "support-suite-provider".to_owned(),
            pnpm_workspace_overrides: String::new(),
            publish_note: None,
            remote_module_kit_dependency: json_string("0.1.3"),
            repo_root_display: "/tmp/host".to_owned(),
            service_cwd: json_string("../services/support-suite-provider"),
            service_kit_dependency: json_string("0.1.0"),
            service_label: "Support Suite".to_owned(),
            service_name: "support-suite-provider".to_owned(),
            target_dir: PathBuf::from("/tmp/services/support-suite-provider"),
            target_dir_display: "/tmp/services/support-suite-provider".to_owned(),
        }
    }

    #[test]
    fn install_command_uses_local_manifest_and_base_url() {
        let command = local_service_install_command(&scaffold().repo_root_display);

        assert_eq!(
            command,
            "lenso service install ./lenso.service.json --base-url http://127.0.0.1:4100/lenso/service/v1 --repo-root /tmp/host"
        );
        assert!(!command.contains("support-suite-provider"));
    }

    #[test]
    fn ts_manifest_records_install_services() {
        let source = render_template(
            include_str!("../templates/service-ts/lenso.service.json.tmpl"),
            &scaffold(),
        );
        assert_no_template_tokens(&source);
        let manifest: Value = serde_json::from_str(&source).unwrap();

        assert_eq!(
            manifest["install"]["services"][0]["name"],
            "support-suite-provider"
        );
        assert_eq!(manifest["install"]["services"][0]["command"], "pnpm start");
        assert_eq!(
            manifest["install"]["services"][0]["readyUrl"],
            "http://127.0.0.1:4100/lenso/service/v1/status"
        );
        assert_eq!(manifest["install"]["services"][0]["autoStart"], json!(true));
        assert_eq!(
            manifest["install"]["services"][0]["readyTimeoutMs"],
            json!(10_000)
        );
    }

    #[test]
    fn rust_manifest_records_install_services() {
        let source = render_template(
            include_str!("../templates/service-rust/lenso.service.json.tmpl"),
            &scaffold(),
        );
        assert_no_template_tokens(&source);
        let manifest: Value = serde_json::from_str(&source).unwrap();

        assert_eq!(
            manifest["install"]["services"][0]["name"],
            "support-suite-provider"
        );
        assert_eq!(manifest["install"]["services"][0]["command"], "cargo run");
        assert_eq!(
            manifest["install"]["services"][0]["readyUrl"],
            "http://127.0.0.1:4100/lenso/service/v1/status"
        );
        assert_eq!(manifest["install"]["services"][0]["autoStart"], json!(true));
        assert_eq!(
            manifest["install"]["services"][0]["readyTimeoutMs"],
            json!(10_000)
        );
    }

    #[test]
    fn generated_service_manifests_use_service_protocol() {
        for template in [
            include_str!("../templates/service-ts/lenso.service.json.tmpl"),
            include_str!("../templates/service-rust/lenso.service.json.tmpl"),
        ] {
            let manifest: Value = serde_json::from_str(&render_template(template, &scaffold()))
                .expect("manifest should parse");

            assert_eq!(manifest["protocol"], "lenso.service.v1");
        }
    }

    #[test]
    fn service_package_metadata_lists_declared_modules() {
        let metadata = service_package_metadata(&json!({
            "name": "support-suite-provider",
            "version": "0.2.0",
            "modules": [
                {"name": "support-ticket"},
                {"name": "support-inbox"}
            ]
        }))
        .unwrap();

        assert_eq!(
            metadata,
            ServicePackageMetadata {
                modules: vec![
                    ServicePackageModuleMetadata {
                        capabilities: Vec::new(),
                        dependencies: Vec::new(),
                        name: "support-ticket".to_owned(),
                        version: "0.2.0".to_owned(),
                    },
                    ServicePackageModuleMetadata {
                        capabilities: Vec::new(),
                        dependencies: Vec::new(),
                        name: "support-inbox".to_owned(),
                        version: "0.2.0".to_owned(),
                    },
                ],
                name: "support-suite-provider".to_owned(),
                version: "0.2.0".to_owned(),
            }
        );

        let artifact = service_package_manifest(&metadata);
        assert_eq!(artifact["protocol"], "lenso.service-package.v1");
        assert_eq!(artifact["serviceManifest"], "lenso.service.json");
    }

    #[tokio::test]
    async fn package_service_writes_manifest_and_package_metadata() {
        let root = std::env::temp_dir().join(format!(
            "lenso-cli-service-package-{}",
            uuid::Uuid::now_v7()
        ));
        let service_dir = root.join("support-suite-provider");
        fs::create_dir_all(&service_dir).unwrap();
        fs::write(
            service_dir.join("lenso.service.json"),
            serde_json::to_string_pretty(&json!({
                "protocol": "lenso.service.v1",
                "name": "support-suite-provider",
                "version": "0.2.0",
                "modules": [
                    {
                        "capabilities": ["support_ticket.tickets.read"],
                        "dependencies": ["auth"],
                        "name": "support-ticket",
                        "version": "0.2.1"
                    }
                ]
            }))
            .unwrap(),
        )
        .unwrap();

        package_service(ServicePackageOptions {
            check: false,
            json: false,
            manifest: "lenso.service.json".to_owned(),
            output_dir: PathBuf::from("dist/services"),
            service_dir: service_dir.clone(),
        })
        .await
        .unwrap();

        let package_manifest: Value = serde_json::from_str(
            &fs::read_to_string(
                service_dir.join("dist/services/support-suite-provider/lenso.service-package.json"),
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(package_manifest["protocol"], "lenso.service-package.v1");
        assert_eq!(package_manifest["modules"], json!(["support-ticket"]));
        let module_release: Value = serde_json::from_str(
            &fs::read_to_string(
                service_dir.join(
                    "dist/services/support-suite-provider/modules/support-ticket/lenso.module-release.json",
                ),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(module_release["protocol"], "lenso.module-release.v1");
        assert_eq!(module_release["name"], "support-ticket");
        assert_eq!(module_release["version"], "0.2.1");
        assert_eq!(
            module_release["provider"]["servicePackage"],
            "../../lenso.service-package.json"
        );
        assert_eq!(
            module_release["capabilities"],
            json!(["support_ticket.tickets.read"])
        );
        assert_eq!(module_release["dependencies"], json!(["auth"]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_templates_render_without_tokens() {
        let scaffold = scaffold();
        for template in [
            include_str!("../templates/service-ts/package.json.tmpl"),
            include_str!("../templates/service-ts/pnpm-workspace.yaml.tmpl"),
            include_str!("../templates/service-rust/Cargo.toml.tmpl"),
            include_str!("../templates/service-ts/src/service.ts"),
            include_str!("../templates/service-ts/src/server.ts"),
            include_str!("../templates/service-rust/src/main.rs"),
        ] {
            assert_no_template_tokens(&render_template(template, &scaffold));
        }
    }

    fn assert_no_template_tokens(source: &str) {
        assert!(!source.contains("{{"), "{source}");
        assert!(!source.contains("}}"), "{source}");
    }
}
