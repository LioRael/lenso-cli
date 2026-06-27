use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::{ServiceCreateArgs, ServiceDevArgs, ServiceLanguage, host, module};

type PendingWrites = BTreeMap<PathBuf, String>;

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
    println!(
        "- cd {}",
        display_relative(&scaffold.output_root, &scaffold.target_dir)
    );
    println!("- {check_command}");
    println!("- lenso service install ./lenso.service.json");
    Ok(())
}

#[derive(Debug)]
struct ServiceScaffold {
    crate_name: String,
    module_name: String,
    output_root: PathBuf,
    package_name: String,
    service_label: String,
    service_name: String,
    target_dir: PathBuf,
}

fn service_scaffold(options: &ServiceCreateOptions) -> Result<ServiceScaffold> {
    let service_name = slugify(&options.name);
    if service_name.is_empty() {
        bail!("Service name is required");
    }
    let output_root = options
        .output_dir
        .as_deref()
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir().context("resolve current directory")?);
    let output_root = absolutize(&output_root)?;
    let target_dir = output_root.join(&service_name);
    if target_dir.exists() {
        bail!("Service directory already exists: {}", target_dir.display());
    }
    let module_name = provided_module_name(&service_name);
    Ok(ServiceScaffold {
        crate_name: snake_case(&service_name),
        module_name: module_name.clone(),
        output_root,
        package_name: service_name.clone(),
        service_label: label_from_slug(&module_name),
        service_name,
        target_dir,
    })
}

fn queue_template(
    pending_writes: &mut PendingWrites,
    path: PathBuf,
    template: &str,
    scaffold: &ServiceScaffold,
) {
    let contents = template
        .replace("{{service_name}}", &scaffold.service_name)
        .replace("{{service_label}}", &scaffold.service_label)
        .replace("{{module_name}}", &scaffold.module_name)
        .replace("{{package_name}}", &scaffold.package_name)
        .replace("{{crate_name}}", &scaffold.crate_name);
    pending_writes.insert(path, contents);
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

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .context("resolve current directory")?
            .join(path))
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
