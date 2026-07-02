use std::collections::BTreeSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub(crate) const PROTOCOL: &str = "lenso.capability-pack.v1";
const LIBRARY_PROTOCOL: &str = "lenso.capability-library.v1";
const MANIFEST: &str = "lenso.capability.json";
const LIBRARY_FILE: &str = ".lenso/lenso.capability-library.json";

#[derive(Debug, Clone)]
pub(crate) struct InitOptions {
    pub(crate) blueprints: Vec<String>,
    pub(crate) dir: PathBuf,
    pub(crate) lang: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CheckOptions {
    pub(crate) json: bool,
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct InspectOptions {
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryInitOptions {
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryAddOptions {
    pub(crate) path: PathBuf,
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryListOptions {
    pub(crate) json: bool,
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub(crate) struct LibraryCheckOptions {
    pub(crate) json: bool,
    pub(crate) repo_root: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityPack {
    pub(crate) protocol: String,
    pub(crate) name: String,
    pub(crate) label: String,
    pub(crate) summary: String,
    pub(crate) supports: CapabilitySupports,
    #[serde(default)]
    pub(crate) modules: Vec<CapabilityModule>,
    #[serde(default)]
    pub(crate) services: Vec<CapabilityService>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<CapabilityAgent>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilitySupports {
    #[serde(default)]
    pub(crate) blueprints: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityModule {
    pub(crate) name: String,
    pub(crate) manifest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityService {
    pub(crate) provider: String,
    pub(crate) service: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) language: Option<String>,
    pub(crate) manifest: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityAgent {
    pub(crate) default_task: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityLibrary {
    pub(crate) protocol: String,
    #[serde(default)]
    pub(crate) packs: Vec<CapabilityLibraryEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityLibraryEntry {
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) label: String,
    pub(crate) summary: String,
    #[serde(default)]
    pub(crate) blueprints: Vec<String>,
    #[serde(default)]
    pub(crate) modules: Vec<String>,
    #[serde(default)]
    pub(crate) services: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityCheckReport {
    pub(crate) name: String,
    pub(crate) issues: Vec<CapabilityCheckIssue>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityCheckIssue {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) severity: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CapabilityLibraryCheckReport {
    pub(crate) status: String,
    pub(crate) packs: Vec<CapabilityCheckReport>,
}

pub(crate) fn init(options: InitOptions) -> Result<()> {
    validate_slug(&options.name)?;
    validate_lang(&options.lang)?;
    fs::create_dir_all(&options.dir)?;
    let manifest_path = options.dir.join(MANIFEST);
    if manifest_path.exists() {
        bail!("{} already exists", manifest_path.display());
    }

    let pack = CapabilityPack {
        agent: Some(CapabilityAgent {
            default_task: format!("add or change {} behavior", options.name),
        }),
        label: title_label(&options.name),
        modules: vec![CapabilityModule {
            manifest: "module/lenso.module.json".to_owned(),
            name: options.name.clone(),
        }],
        name: options.name.clone(),
        protocol: PROTOCOL.to_owned(),
        services: vec![CapabilityService {
            language: Some(options.lang.clone()),
            manifest: "service/lenso.service.json".to_owned(),
            provider: format!("{}-provider", options.name),
            service: "api".to_owned(),
        }],
        summary: format!("Adds {} business behavior.", options.name),
        supports: CapabilitySupports {
            blueprints: options.blueprints,
        },
    };

    write_json(&manifest_path, &pack)?;
    write_seed_manifests(&options.dir, &pack)?;
    fs::write(
        options.dir.join("README.md"),
        format!(
            "# {}\n\nLocal Lenso capability pack.\n\nRun:\n\n```sh\nlenso capability check .\nlenso app compose --pack .\n```\n",
            pack.label
        ),
    )
    .with_context(|| format!("write {}", options.dir.join("README.md").display()))?;
    println!("Created capability pack {}.", pack.name);
    println!("Next: lenso capability check {}", options.dir.display());
    Ok(())
}

pub(crate) fn check(options: CheckOptions) -> Result<()> {
    let report = check_pack(&options.path)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Capability pack: {}", report.name);
        if report.issues.is_empty() {
            println!("Status: ready");
        } else {
            for issue in &report.issues {
                println!("- {}: {}", issue.code, issue.message);
            }
        }
    }
    if report.issues.iter().any(|issue| issue.severity == "error") {
        bail!("capability pack check failed");
    }
    Ok(())
}

pub(crate) fn inspect(options: InspectOptions) -> Result<()> {
    let pack = read_pack(&options.path)?;
    println!("{} ({})", pack.label, pack.name);
    println!("{}", pack.summary);
    println!("blueprints: {}", list_or_none(&pack.supports.blueprints));
    println!(
        "modules: {}",
        list_or_none(
            &pack
                .modules
                .iter()
                .map(|module| module.name.clone())
                .collect::<Vec<_>>()
        )
    );
    println!(
        "services: {}",
        list_or_none(
            &pack
                .services
                .iter()
                .map(|service| format!("{}/{}", service.provider, service.service))
                .collect::<Vec<_>>()
        )
    );
    if let Some(agent) = &pack.agent {
        println!("agent task: {}", agent.default_task);
    }
    println!("Next: lenso app compose --pack {}", options.path.display());
    Ok(())
}

pub(crate) fn library_init(options: LibraryInitOptions) -> Result<()> {
    let repo_root = repo_root(options.repo_root)?;
    let path = library_path(&repo_root);
    if path.exists() {
        println!("Capability library already exists: {}", path.display());
        return Ok(());
    }
    write_json(
        &path,
        &CapabilityLibrary {
            packs: Vec::new(),
            protocol: LIBRARY_PROTOCOL.to_owned(),
        },
    )?;
    println!("Created capability library {}.", path.display());
    Ok(())
}

pub(crate) fn library_add(options: LibraryAddOptions) -> Result<()> {
    let repo_root = repo_root(options.repo_root)?;
    let pack_path = resolve_input_path(&repo_root, &options.path);
    let report = check_pack(&pack_path)?;
    if report.issues.iter().any(|issue| issue.severity == "error") {
        bail!(
            "capability pack check failed; run lenso capability check {}",
            pack_path.display()
        );
    }
    let pack = read_pack(&pack_path)?;
    let mut library = read_library_or_default(&repo_root)?;
    let entry = library_entry_from_pack(&repo_root, &pack_path, &pack);
    library.packs.retain(|existing| existing.name != entry.name);
    library.packs.push(entry.clone());
    library.packs.sort_by(|a, b| a.name.cmp(&b.name));
    write_json(&library_path(&repo_root), &library)?;
    println!("Added capability pack {}.", entry.name);
    println!("Next: lenso app compose --pack {}", entry.name);
    Ok(())
}

pub(crate) fn library_list(options: LibraryListOptions) -> Result<()> {
    let repo_root = repo_root(options.repo_root)?;
    let library = read_library_or_default(&repo_root)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&library)?);
    } else if library.packs.is_empty() {
        println!("Capability library is empty.");
        println!("Next: lenso capability library add ./capabilities/support-sla");
    } else {
        for pack in &library.packs {
            println!("{} -> {}", pack.name, pack.path);
        }
    }
    Ok(())
}

pub(crate) fn library_check(options: LibraryCheckOptions) -> Result<()> {
    let repo_root = repo_root(options.repo_root)?;
    let report = check_library(&repo_root)?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Capability library: {}", report.status);
        for pack in &report.packs {
            println!(
                "- {}: {}",
                pack.name,
                if pack.issues.is_empty() {
                    "ready"
                } else {
                    "failed"
                }
            );
        }
    }
    if report.status == "failed" {
        bail!("capability library check failed");
    }
    Ok(())
}

pub(crate) fn read_pack(path: &Path) -> Result<CapabilityPack> {
    let manifest = manifest_path(path);
    let contents =
        fs::read_to_string(&manifest).with_context(|| format!("read {}", manifest.display()))?;
    serde_json::from_str(&contents).with_context(|| format!("parse {}", manifest.display()))
}

pub(crate) fn resolve_pack_path(repo_root: &Path, requested: &Path) -> PathBuf {
    let path = resolve_input_path(repo_root, requested);
    if manifest_path(&path).exists() {
        return path;
    }
    let Some(name) = requested.to_str() else {
        return path;
    };
    if requested.components().count() == 1
        && let Ok(library) = read_library_or_default(repo_root)
        && let Some(entry) = library.packs.iter().find(|entry| entry.name == name)
    {
        return repo_root.join(&entry.path);
    }
    path
}

pub(crate) fn check_pack(path: &Path) -> Result<CapabilityCheckReport> {
    let mut issues = Vec::new();
    let pack = match read_pack(path) {
        Ok(pack) => pack,
        Err(err) => {
            return Ok(CapabilityCheckReport {
                issues: vec![error("manifest", err.to_string())],
                name: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown")
                    .to_owned(),
            });
        }
    };

    if pack.protocol != PROTOCOL {
        issues.push(error(
            "protocol",
            format!("expected protocol {PROTOCOL}, found {}", pack.protocol),
        ));
    }
    if let Err(err) = validate_slug(&pack.name) {
        issues.push(error("name", err.to_string()));
    }
    if pack.supports.blueprints.is_empty() {
        issues.push(error(
            "blueprints",
            "pack must declare supported blueprints",
        ));
    }

    let mut module_names = BTreeSet::new();
    for module in &pack.modules {
        if !module_names.insert(module.name.as_str()) {
            issues.push(error(
                "duplicate-module",
                format!("duplicate module `{}`", module.name),
            ));
        }
        check_relative_manifest(path, &module.manifest, &mut issues);
    }

    let mut service_keys = BTreeSet::new();
    for service in &pack.services {
        let key = format!("{}/{}", service.provider, service.service);
        if !service_keys.insert(key.clone()) {
            issues.push(error(
                "duplicate-service",
                format!("duplicate service `{key}`"),
            ));
        }
        if let Some(lang) = &service.language
            && let Err(err) = validate_lang(lang)
        {
            issues.push(error("language", err.to_string()));
        }
        check_relative_manifest(path, &service.manifest, &mut issues);
    }

    Ok(CapabilityCheckReport {
        issues,
        name: pack.name,
    })
}

pub(crate) fn check_library(repo_root: &Path) -> Result<CapabilityLibraryCheckReport> {
    let library = read_library_or_default(repo_root)?;
    let packs = library
        .packs
        .iter()
        .map(|entry| check_pack(&repo_root.join(&entry.path)))
        .collect::<Result<Vec<_>>>()?;
    let status = if packs
        .iter()
        .any(|pack| pack.issues.iter().any(|issue| issue.severity == "error"))
    {
        "failed"
    } else {
        "ready"
    }
    .to_owned();
    Ok(CapabilityLibraryCheckReport { packs, status })
}

pub(crate) fn library_path(repo_root: &Path) -> PathBuf {
    repo_root.join(LIBRARY_FILE)
}

fn read_library_or_default(repo_root: &Path) -> Result<CapabilityLibrary> {
    let path = library_path(repo_root);
    if !path.exists() {
        return Ok(CapabilityLibrary {
            packs: Vec::new(),
            protocol: LIBRARY_PROTOCOL.to_owned(),
        });
    }
    let contents = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut library: CapabilityLibrary =
        serde_json::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
    if library.protocol.is_empty() {
        library.protocol = LIBRARY_PROTOCOL.to_owned();
    }
    Ok(library)
}

fn library_entry_from_pack(
    repo_root: &Path,
    pack_path: &Path,
    pack: &CapabilityPack,
) -> CapabilityLibraryEntry {
    CapabilityLibraryEntry {
        blueprints: pack.supports.blueprints.clone(),
        label: pack.label.clone(),
        modules: pack
            .modules
            .iter()
            .map(|module| module.name.clone())
            .collect(),
        name: pack.name.clone(),
        path: display_pack_path(repo_root, pack_path),
        services: pack
            .services
            .iter()
            .map(|service| format!("{}/{}", service.provider, service.service))
            .collect(),
        summary: pack.summary.clone(),
    }
}

fn repo_root(repo_root: Option<PathBuf>) -> Result<PathBuf> {
    match repo_root {
        Some(path) => Ok(path),
        None => std::env::current_dir().context("read current directory"),
    }
}

fn resolve_input_path(repo_root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        let repo_path = repo_root.join(path);
        if manifest_path(&repo_path).exists() {
            repo_path
        } else {
            path.to_path_buf()
        }
    }
}

fn display_pack_path(repo_root: &Path, pack_path: &Path) -> String {
    let absolute_pack = fs::canonicalize(manifest_path(pack_path))
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf));
    let absolute_root = fs::canonicalize(repo_root).ok();
    if let (Some(root), Some(pack)) = (absolute_root, absolute_pack)
        && let Ok(relative) = pack.strip_prefix(root)
    {
        return relative.display().to_string();
    }
    pack_path.display().to_string()
}

fn write_seed_manifests(dir: &Path, pack: &CapabilityPack) -> Result<()> {
    for module in &pack.modules {
        let path = dir.join(&module.manifest);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            write_json(
                &path,
                &serde_json::json!({
                    "protocol": "lenso.module.v1",
                    "name": module.name,
                    "capabilities": [format!("{}.read", module.name.replace('-', "."))]
                }),
            )?;
        }
    }
    for service in &pack.services {
        let path = dir.join(&service.manifest);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            write_json(
                &path,
                &serde_json::json!({
                    "protocol": "lenso.service.v1",
                    "name": service.provider,
                    "services": [{
                        "name": service.service,
                        "manifest": "lenso.service.json"
                    }]
                }),
            )?;
        }
    }
    Ok(())
}

fn check_relative_manifest(root: &Path, relative: &str, issues: &mut Vec<CapabilityCheckIssue>) {
    if !safe_relative_path(relative) {
        issues.push(error(
            "path",
            format!("manifest path `{relative}` must stay inside the pack"),
        ));
        return;
    }
    let path = root.join(relative);
    if !path.exists() {
        issues.push(error(
            "missing-manifest",
            format!("{} does not exist", path.display()),
        ));
    }
}

fn safe_relative_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && path.components().all(|component| {
            !matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn manifest_path(path: &Path) -> PathBuf {
    if path.file_name().and_then(|name| name.to_str()) == Some(MANIFEST) {
        path.to_path_buf()
    } else {
        path.join(MANIFEST)
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

fn validate_slug(value: &str) -> Result<()> {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'))
        || value.starts_with('-')
        || value.ends_with('-')
    {
        bail!("`{value}` must be a lowercase slug");
    }
    Ok(())
}

fn validate_lang(value: &str) -> Result<()> {
    if matches!(value, "rust" | "ts") {
        Ok(())
    } else {
        bail!("language `{value}` is not supported; use rust or ts")
    }
}

fn title_label(value: &str) -> String {
    value
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}

fn error(code: impl Into<String>, message: impl Into<String>) -> CapabilityCheckIssue {
    CapabilityCheckIssue {
        code: code.into(),
        message: message.into(),
        severity: "error".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(name: &str) -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lenso-{name}-{id}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn capability_pack_init_writes_checkable_manifest() {
        let root = temp_test_dir("capability-pack-init");
        let dir = root.join("support-sla");

        init(InitOptions {
            blueprints: vec!["support-desk".to_owned()],
            dir: dir.clone(),
            lang: "ts".to_owned(),
            name: "support-sla".to_owned(),
        })
        .unwrap();

        let report = check_pack(&dir).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        let pack = read_pack(&dir).unwrap();
        assert_eq!(pack.protocol, PROTOCOL);
        assert_eq!(pack.name, "support-sla");
    }

    #[test]
    fn capability_pack_check_blocks_path_escape() {
        let root = temp_test_dir("capability-pack-escape");
        let dir = root.join("bad-pack");
        fs::create_dir_all(&dir).unwrap();
        write_json(
            &dir.join(MANIFEST),
            &CapabilityPack {
                agent: None,
                label: "Bad Pack".to_owned(),
                modules: vec![CapabilityModule {
                    manifest: "../outside.json".to_owned(),
                    name: "bad-pack".to_owned(),
                }],
                name: "bad-pack".to_owned(),
                protocol: PROTOCOL.to_owned(),
                services: Vec::new(),
                summary: "Bad".to_owned(),
                supports: CapabilitySupports {
                    blueprints: vec!["support-desk".to_owned()],
                },
            },
        )
        .unwrap();

        let report = check_pack(&dir).unwrap();
        assert!(report.issues.iter().any(|issue| issue.code == "path"));
    }

    #[test]
    fn capability_library_add_records_pack() {
        let root = temp_test_dir("capability-library-add");
        let dir = root.join("capabilities/support-sla");
        init(InitOptions {
            blueprints: vec!["support-desk".to_owned()],
            dir: dir.clone(),
            lang: "ts".to_owned(),
            name: "support-sla".to_owned(),
        })
        .unwrap();

        library_add(LibraryAddOptions {
            path: PathBuf::from("capabilities/support-sla"),
            repo_root: Some(root.clone()),
        })
        .unwrap();

        let library = read_library_or_default(&root).unwrap();
        assert_eq!(library.packs.len(), 1);
        assert_eq!(library.packs[0].name, "support-sla");
        assert_eq!(library.packs[0].path, "capabilities/support-sla");
    }

    #[test]
    fn resolve_pack_path_uses_library_name() {
        let root = temp_test_dir("capability-library-resolve");
        let dir = root.join("capabilities/support-sla");
        init(InitOptions {
            blueprints: vec!["support-desk".to_owned()],
            dir,
            lang: "ts".to_owned(),
            name: "support-sla".to_owned(),
        })
        .unwrap();
        library_add(LibraryAddOptions {
            path: PathBuf::from("capabilities/support-sla"),
            repo_root: Some(root.clone()),
        })
        .unwrap();

        assert_eq!(
            resolve_pack_path(&root, Path::new("support-sla")),
            root.join("capabilities/support-sla")
        );
    }
}
