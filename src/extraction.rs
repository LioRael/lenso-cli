use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use lenso_service::{
    ExtractionBoundaryEvidence, ExtractionBoundaryReference, ExtractionBoundaryReferenceKind,
    ExtractionEvidenceStatus, ExtractionReadinessEvidence, ModuleManifest,
    evaluate_extraction_readiness, extraction_readiness_report_json,
    render_extraction_readiness_report,
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const DEFAULT_SYSTEM_FILE: &str = "lenso.system.json";
const DEFAULT_MODULES_DIR: &str = "modules";

#[derive(Debug, Clone)]
pub(crate) struct ModuleExtractionReadinessOptions {
    pub(crate) evidence_file: Option<PathBuf>,
    pub(crate) json: bool,
    pub(crate) module_manifest: Option<PathBuf>,
    pub(crate) module_name: String,
    pub(crate) modules_root: Option<PathBuf>,
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) system_file: Option<PathBuf>,
}

#[derive(Debug)]
struct RustModule {
    crate_name: String,
    module_name: String,
    root: PathBuf,
}

pub(crate) fn report_module_extraction_readiness(
    options: ModuleExtractionReadinessOptions,
) -> Result<()> {
    let current_dir = std::env::current_dir().context("resolve current directory")?;
    let repo_root = resolve_from(
        &current_dir,
        options.repo_root.as_deref().unwrap_or(Path::new(".")),
    );
    let modules_root = resolve_from(
        &repo_root,
        options
            .modules_root
            .as_deref()
            .unwrap_or_else(|| Path::new(DEFAULT_MODULES_DIR)),
    );
    let manifest_path = options.module_manifest.map_or_else(
        || {
            modules_root
                .join(&options.module_name)
                .join("lenso.module.json")
        },
        |path| resolve_from(&repo_root, &path),
    );
    let system_path = resolve_from(
        &repo_root,
        options
            .system_file
            .as_deref()
            .unwrap_or_else(|| Path::new(DEFAULT_SYSTEM_FILE)),
    );

    let module: ModuleManifest = read_input(
        &manifest_path,
        options.json,
        "module_manifest_invalid",
        "Fix the Module manifest JSON and rerun extraction readiness.",
    )?;
    if module.name != options.module_name {
        return command_failure(
            options.json,
            "target_module_mismatch",
            format!(
                "Requested Module `{}` but {} declares `{}`.",
                options.module_name,
                display_path(&repo_root, &manifest_path),
                module.name
            ),
            "Select the matching Module manifest and rerun extraction readiness.",
        );
    }
    let system: Value = read_input(
        &system_path,
        options.json,
        "system_artifact_invalid",
        "Fix the System artifact JSON and rerun extraction readiness.",
    )?;
    let mut evidence = match options.evidence_file {
        Some(path) => read_input(
            &resolve_from(&repo_root, &path),
            options.json,
            "extraction_evidence_invalid",
            "Fix the extraction evidence JSON and rerun readiness.",
        )?,
        None => ExtractionReadinessEvidence::default(),
    };

    evidence.boundary = Some(scan_rust_module_boundaries(
        &repo_root,
        &modules_root,
        &module,
    ));
    verify_contract_artifact_references(&repo_root, &mut evidence);

    let report = evaluate_extraction_readiness(&module, &system, &evidence);
    if options.json {
        print!("{}", extraction_readiness_report_json(&report)?);
    } else {
        print!("{}", render_extraction_readiness_report(&report));
    }
    if !report.ready {
        bail!("Module extraction readiness is blocked");
    }
    Ok(())
}

fn read_input<T: DeserializeOwned>(
    path: &Path,
    json_output: bool,
    code: &str,
    next_action: &str,
) -> Result<T> {
    match fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))
        .and_then(|source| {
            serde_json::from_str(&source).with_context(|| format!("parse {}", path.display()))
        }) {
        Ok(value) => Ok(value),
        Err(error) => command_failure(json_output, code, error.to_string(), next_action),
    }
}

fn command_failure<T>(
    json_output: bool,
    code: &str,
    message: String,
    next_action: &str,
) -> Result<T> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "artifactVersion": "lenso.command-error.v1",
                "code": code,
                "message": message,
                "nextAction": next_action,
            }))?
        );
    }
    bail!(message)
}

fn scan_rust_module_boundaries(
    repo_root: &Path,
    modules_root: &Path,
    target: &ModuleManifest,
) -> ExtractionBoundaryEvidence {
    let analyzer_reference = format!("analyzer:rust/{}", display_path(repo_root, modules_root));
    let Ok(modules) = discover_rust_modules(modules_root) else {
        return ExtractionBoundaryEvidence {
            complete: false,
            evidence_references: vec![analyzer_reference],
            references: Vec::new(),
        };
    };
    let normalized_target = normalize_crate_name(&target.name);
    let target_indices = modules
        .iter()
        .enumerate()
        .filter_map(|(index, module)| {
            let crate_name = normalize_crate_name(&module.crate_name);
            (module.module_name == target.name
                || crate_name == normalized_target
                || crate_name == format!("lenso_module_{normalized_target}"))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let [target_index] = target_indices.as_slice() else {
        return ExtractionBoundaryEvidence {
            complete: false,
            evidence_references: vec![analyzer_reference],
            references: Vec::new(),
        };
    };
    let target_index = *target_index;

    let mut complete = true;
    let mut references = target
        .dependencies
        .iter()
        .map(|dependency| ExtractionBoundaryReference {
            kind: ExtractionBoundaryReferenceKind::InProcessBoundaryCall,
            from_module: target.name.clone(),
            to_module: dependency.clone(),
            symbol: format!("module dependency {dependency}"),
            evidence_reference: format!("module:manifest/dependency/{dependency}"),
        })
        .collect::<Vec<_>>();

    for (owner_index, _) in modules.iter().enumerate() {
        scan_cargo_dependencies(
            repo_root,
            &modules,
            owner_index,
            target_index,
            &target.name,
            &mut references,
            &mut complete,
        );
        if !scan_rust_source_references(
            repo_root,
            &modules,
            owner_index,
            target_index,
            &target.name,
            &mut references,
        ) {
            complete = false;
        }
    }
    references.sort_by(|left, right| {
        (
            &left.evidence_reference,
            &left.from_module,
            &left.to_module,
            &left.symbol,
        )
            .cmp(&(
                &right.evidence_reference,
                &right.from_module,
                &right.to_module,
                &right.symbol,
            ))
    });
    references.dedup_by(|left, right| {
        left.kind == right.kind
            && left.from_module == right.from_module
            && left.to_module == right.to_module
            && left.symbol == right.symbol
            && left.evidence_reference == right.evidence_reference
    });

    ExtractionBoundaryEvidence {
        complete,
        evidence_references: vec![analyzer_reference],
        references,
    }
}

fn scan_rust_source_references(
    repo_root: &Path,
    modules: &[RustModule],
    owner_index: usize,
    target_index: usize,
    target_name: &str,
    references: &mut Vec<ExtractionBoundaryReference>,
) -> bool {
    let owner = &modules[owner_index];
    let source_root = owner.root.join("src");
    if !source_root.is_dir() {
        return false;
    }
    let mut rust_files = Vec::new();
    if collect_rust_files(&source_root, &mut rust_files).is_err() {
        return false;
    }
    rust_files.sort();
    let mut complete = true;
    for path in rust_files {
        let Ok(source) = fs::read_to_string(&path) else {
            complete = false;
            continue;
        };
        for (line_index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default().trim();
            if code.is_empty() {
                continue;
            }
            for (dependency_index, dependency) in modules.iter().enumerate() {
                if owner_index == dependency_index
                    || (owner_index != target_index && dependency_index != target_index)
                {
                    continue;
                }
                let crate_identifier = normalize_crate_name(&dependency.crate_name);
                let Some(symbol) = referenced_symbol(code, &crate_identifier) else {
                    continue;
                };
                references.push(ExtractionBoundaryReference {
                    kind: if is_import(code) {
                        ExtractionBoundaryReferenceKind::CrossModuleImport
                    } else {
                        ExtractionBoundaryReferenceKind::InProcessBoundaryCall
                    },
                    from_module: module_identity(owner_index, target_index, owner, target_name),
                    to_module: module_identity(
                        dependency_index,
                        target_index,
                        dependency,
                        target_name,
                    ),
                    symbol,
                    evidence_reference: format!(
                        "{}:{}",
                        display_path(repo_root, &path),
                        line_index + 1
                    ),
                });
            }
        }
    }
    complete
}

#[allow(clippy::too_many_arguments)]
fn scan_cargo_dependencies(
    repo_root: &Path,
    modules: &[RustModule],
    owner_index: usize,
    target_index: usize,
    target_name: &str,
    references: &mut Vec<ExtractionBoundaryReference>,
    complete: &mut bool,
) {
    let owner = &modules[owner_index];
    let cargo_path = owner.root.join("Cargo.toml");
    let Ok(source) = fs::read_to_string(&cargo_path) else {
        *complete = false;
        return;
    };
    let mut in_dependencies = false;
    for (line_index, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_dependencies = trimmed.contains("dependencies");
            continue;
        }
        if !in_dependencies || trimmed.starts_with('#') {
            continue;
        }
        let Some(dependency_index) = dependency_module_index(trimmed, modules) else {
            continue;
        };
        if owner_index == dependency_index
            || (owner_index != target_index && dependency_index != target_index)
        {
            continue;
        }
        let dependency = &modules[dependency_index];
        references.push(ExtractionBoundaryReference {
            kind: ExtractionBoundaryReferenceKind::CrossModuleImport,
            from_module: module_identity(owner_index, target_index, owner, target_name),
            to_module: module_identity(dependency_index, target_index, dependency, target_name),
            symbol: format!("Cargo dependency {}", dependency.crate_name),
            evidence_reference: format!(
                "{}:{}",
                display_path(repo_root, &cargo_path),
                line_index + 1
            ),
        });
    }
}

fn discover_rust_modules(modules_root: &Path) -> Result<Vec<RustModule>> {
    let mut roots = Vec::new();
    for entry in
        fs::read_dir(modules_root).with_context(|| format!("read {}", modules_root.display()))?
    {
        let path = entry?.path();
        if path.is_dir() && path.join("Cargo.toml").is_file() {
            roots.push(path);
        }
    }
    roots.sort();
    let modules = roots
        .into_iter()
        .map(|root| {
            let cargo_path = root.join("Cargo.toml");
            let source = fs::read_to_string(&cargo_path)
                .with_context(|| format!("read {}", cargo_path.display()))?;
            let crate_name = package_name(&source)
                .ok_or_else(|| anyhow::anyhow!("{} has no package name", cargo_path.display()))?;
            let module_name = root
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid module path {}", root.display()))?
                .to_owned();
            Ok(RustModule {
                crate_name,
                module_name,
                root,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    for (index, module) in modules.iter().enumerate() {
        if modules[..index].iter().any(|candidate| {
            candidate.module_name == module.module_name || candidate.crate_name == module.crate_name
        }) {
            bail!(
                "duplicate Rust Module or crate identity under {}",
                modules_root.display()
            );
        }
    }
    Ok(modules)
}

fn package_name(source: &str) -> Option<String> {
    let mut in_package = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
            continue;
        }
        if in_package && let Some(value) = string_assignment(trimmed, "name") {
            return Some(value.to_owned());
        }
    }
    None
}

fn dependency_module_index(line: &str, modules: &[RustModule]) -> Option<usize> {
    let direct = line
        .split_once('=')
        .map(|(key, _)| key.trim().trim_matches(['\'', '"']));
    let package = line
        .split("package")
        .nth(1)
        .and_then(|rest| rest.split_once('='))
        .and_then(|(_, value)| value.trim().split([',', '}']).next())
        .map(|value| value.trim().trim_matches(['\'', '"']));
    modules.iter().position(|module| {
        direct == Some(module.crate_name.as_str()) || package == Some(module.crate_name.as_str())
    })
}

fn string_assignment<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (candidate, value) = line.split_once('=')?;
    (candidate.trim() == key).then(|| value.trim().trim_matches(['\'', '"']))
}

fn collect_rust_files(root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn is_import(code: &str) -> bool {
    code.starts_with("use ")
        || code.starts_with("pub use ")
        || code.starts_with("pub(crate) use ")
        || code.starts_with("extern crate ")
}

fn referenced_symbol(code: &str, crate_identifier: &str) -> Option<String> {
    let start = code
        .match_indices(crate_identifier)
        .find_map(|(index, _)| {
            let before = code[..index].chars().next_back();
            let after = code[index + crate_identifier.len()..].chars().next();
            let boundary = |character: char| !character.is_ascii_alphanumeric() && character != '_';
            before
                .is_none_or(boundary)
                .then_some(())
                .filter(|()| after.is_none_or(boundary))
                .map(|()| index)
        })?;
    let end = code[start..]
        .char_indices()
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || *character == '_' || *character == ':'
        })
        .map(|(index, character)| start + index + character.len_utf8())
        .last()
        .unwrap_or(start + crate_identifier.len());
    Some(code[start..end].trim_end_matches(':').to_owned())
}

fn module_identity(
    index: usize,
    target_index: usize,
    module: &RustModule,
    target_name: &str,
) -> String {
    if index == target_index {
        target_name.to_owned()
    } else {
        module.module_name.clone()
    }
}

fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

fn verify_contract_artifact_references(
    repo_root: &Path,
    evidence: &mut ExtractionReadinessEvidence,
) {
    let Some(contracts) = evidence.contracts.as_mut() else {
        return;
    };
    for contract in contracts {
        if contract.status != ExtractionEvidenceStatus::Present {
            continue;
        }
        let local_paths = contract
            .evidence_references
            .iter()
            .filter(|reference| !is_semantic_reference(reference))
            .map(|reference| resolve_from(repo_root, Path::new(reference)))
            .collect::<Vec<_>>();
        if local_paths.is_empty() {
            contract.status = ExtractionEvidenceStatus::Ambiguous;
        } else if !local_paths.iter().any(|path| path.is_file()) {
            contract.status = ExtractionEvidenceStatus::Missing;
        }
    }
}

fn is_semantic_reference(reference: &str) -> bool {
    reference.starts_with("analyzer:")
        || reference.starts_with("module:")
        || reference.starts_with("system:")
        || reference.starts_with("http://")
        || reference.starts_with("https://")
}

fn resolve_from(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn display_path(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
