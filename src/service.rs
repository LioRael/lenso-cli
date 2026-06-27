use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::{ServiceCreateArgs, ServiceDevArgs, ServiceLanguage, host, module};

type PendingWrites = BTreeMap<PathBuf, String>;
const LOCAL_SERVICE_INSTALL_COMMAND: &str =
    "lenso service install ./lenso.service.json --base-url http://127.0.0.1:4100/lenso/service/v1";

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
    println!("- {LOCAL_SERVICE_INSTALL_COMMAND}");
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
        assert_eq!(
            LOCAL_SERVICE_INSTALL_COMMAND,
            "lenso service install ./lenso.service.json --base-url http://127.0.0.1:4100/lenso/service/v1"
        );
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
