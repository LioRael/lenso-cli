//! Read-only surface selection. No compiler, package manager or business code runs here.
use super::{Candidate, DiscoveryReport, SourceRole, project, read_metadata};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Composite {
    schema: String,
    core: String,
    #[serde(default)]
    surfaces: Vec<Surface>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Surface {
    entry: String,
    project: String,
    #[serde(default)]
    required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Convention {
    id: String,
    entries: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SurfaceSelection {
    pub owner: String,
    pub entry: PathBuf,
    pub support: Option<String>,
    pub convention: Option<String>,
    pub plugin_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ConventionPlan {
    pub schema: &'static str,
    pub candidates: Vec<Candidate>,
    pub surfaces: Vec<SurfaceSelection>,
}

pub(super) fn composite(root: &Path, role: SourceRole) -> anyhow::Result<Candidate> {
    let manifest = root.join("plugin.json");
    let composite: Composite = serde_json::from_str(&read_metadata(&manifest)?)?;
    if composite.schema != "lenso.plugin-project.v1" {
        bail!("unsupported composite Plugin project schema");
    }
    let core = inside(root, &composite.core)?;
    if core == root {
        bail!("composite core must be a nested package");
    }
    let mut candidate =
        project::read(&core, role)?.context("composite core must declare a Plugin")?;
    candidate.evidence = format!("composite:{}", manifest.display());
    // Keep the compiler project and its metadata unchanged. The separate source
    // root is recovered from this manifest by the selection phase.
    candidate.composite = Some(manifest);
    Ok(candidate)
}

fn metadata(candidate: &Candidate) -> anyhow::Result<serde_json::Value> {
    if candidate.format == "bundle" {
        return Ok(serde_json::Value::Null);
    }
    let document = project::document(&candidate.metadata)?;
    Ok(if candidate.format == "cargo" {
        document.pointer("/package/metadata/lenso")
    } else {
        document.get("lenso")
    }
    .cloned()
    .unwrap_or_default())
}

fn active_instances(root: &Path, candidate: &Candidate) -> anyhow::Result<usize> {
    let directory = root.join("plugins").join(&candidate.plugin_id);
    if !directory.try_exists()? {
        return Ok(usize::from(candidate.role == SourceRole::AppOwned));
    }
    if !fs::symlink_metadata(&directory)?.file_type().is_dir() {
        bail!("Plugin Root directory must not be a symbolic link");
    }
    let mut normalized = BTreeMap::new();
    let mut instances = BTreeSet::new();
    if candidate.role == SourceRole::AppOwned {
        instances.insert("default".to_owned());
    }
    let mut disabled = BTreeSet::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("Plugin Root entries cannot be symbolic links");
        }
        if !kind.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = name
            .to_str()
            .context("Plugin Root filename must be UTF-8")?;
        crate::reject_case_collision(&mut normalized, name, "Plugin filename")?;
        if let Some(instance) = name.strip_suffix(".toml") {
            crate::validate_instance_filename(instance)?;
            crate::read_configuration(&entry.path())?;
            instances.insert(instance.to_owned());
        }
        if let Some(instance) = name.strip_suffix(".disabled") {
            crate::validate_instance_filename(instance)?;
            if entry.metadata()?.len() != 0 {
                bail!("disabled marker must be empty");
            }
            disabled.insert(instance.to_owned());
        }
    }
    Ok(instances.difference(&disabled).count())
}

/// Select contributions from adopted support packages, without inspecting inactive
/// package manifests. The ordinary runtime resolver still owns final admission.
pub fn plan(report: &DiscoveryReport) -> anyhow::Result<ConventionPlan> {
    let mut recognition = BTreeMap::<String, (String, String)>::new();
    let mut conventions = BTreeSet::new();
    for candidate in &report.candidates {
        if active_instances(&report.root, candidate)? == 0 {
            continue;
        }
        let meta = metadata(candidate)?;
        let declarations: Vec<Convention> = serde_json::from_value(
            meta.get("conventions")
                .cloned()
                .unwrap_or(serde_json::json!([])),
        )?;
        for declaration in declarations {
            crate::identity::classify_existing_plugin_id(&declaration.id)?;
            if !conventions.insert(declaration.id.clone()) {
                bail!("duplicate convention identity {}", declaration.id);
            }
            if declaration.entries.is_empty() || declaration.entries.len() > 64 {
                bail!("convention entries must contain 1..64 filenames");
            }
            for entry in declaration.entries {
                if entry.is_empty()
                    || Path::new(&entry).components().count() != 1
                    || !matches!(
                        Path::new(&entry).components().next(),
                        Some(Component::Normal(_))
                    )
                {
                    bail!("convention entry must be a filename: {entry}");
                }
                if let Some(previous) = recognition.insert(
                    entry.clone(),
                    (candidate.plugin_id.clone(), declaration.id.clone()),
                ) {
                    bail!(
                        "conflicting convention entry {entry}: {} and {}",
                        previous.0,
                        candidate.plugin_id
                    );
                }
            }
        }
    }
    let mut result = ConventionPlan {
        schema: "lenso.convention-plan.v1",
        candidates: report.candidates.clone(),
        surfaces: Vec::new(),
    };
    let mut ids = report
        .candidates
        .iter()
        .map(|c| c.plugin_id.clone())
        .collect::<BTreeSet<_>>();
    for owner in &report.candidates {
        let (base, surfaces) = if let Some(manifest) = &owner.composite {
            let composite: Composite = serde_json::from_str(&read_metadata(manifest)?)?;
            (
                manifest.parent().context("composite parent")?.to_path_buf(),
                composite.surfaces,
            )
        } else {
            let meta = metadata(owner)?;
            (
                owner.project.clone(),
                serde_json::from_value::<Vec<Surface>>(
                    meta.get("surfaces")
                        .cloned()
                        .unwrap_or(serde_json::json!([])),
                )?,
            )
        };
        if surfaces.len() > 256 {
            bail!("Plugin accepts at most 256 surfaces");
        }
        let owner_instances = active_instances(&report.root, owner)?;
        if !surfaces.is_empty() && owner_instances > 1 {
            bail!(
                "surface composition requires one active owner instance: {}",
                owner.plugin_id
            );
        }
        let selected_owner = owner_instances == 1;
        let mut entries = BTreeSet::new();
        for surface in surfaces {
            let entry = inside(&base, &surface.entry)?;
            if !entry.is_file() {
                bail!("surface entry must be a file: {}", entry.display());
            }
            if !entries.insert(entry.clone()) {
                bail!("duplicate surface entry {}", entry.display());
            }
            let filename = entry
                .file_name()
                .and_then(|s| s.to_str())
                .context("surface filename must be UTF-8")?;
            let support = recognition.get(filename);
            let active = selected_owner && support.is_some();
            if selected_owner && surface.required && support.is_none() {
                bail!(
                    "required surface {} has no adopted convention support",
                    entry.display()
                );
            }
            let mut selection = SurfaceSelection {
                owner: owner.plugin_id.clone(),
                entry: entry.clone(),
                support: support.map(|s| s.0.clone()),
                convention: support.map(|s| s.1.clone()),
                plugin_id: None,
                reason: if !selected_owner {
                    "owner_not_adopted"
                } else if !active {
                    "support_not_adopted"
                } else {
                    "selected"
                }
                .into(),
            };
            if active {
                let package = inside(&base, &surface.project)?;
                if package == owner.project || !entry.starts_with(&package) {
                    bail!("surface requires an independent package containing its entry");
                }
                let mut candidate = project::read(&package, owner.role)?
                    .context("selected surface package must declare a Plugin")?;
                if candidate.release_version != owner.release_version {
                    bail!("surface release must match its logical owner");
                }
                if !ids.insert(candidate.plugin_id.clone()) {
                    bail!("duplicate surface Plugin identity {}", candidate.plugin_id);
                }
                let meta = metadata(&candidate)?;
                if meta.get("conventions").is_some() || meta.get("surfaces").is_some() {
                    bail!("surface packages cannot recursively activate conventions or surfaces");
                }
                candidate.surface_owner = Some(owner.plugin_id.clone());
                candidate.evidence = format!("surface:{}:{}", owner.plugin_id, support.unwrap().1);
                selection.plugin_id = Some(candidate.plugin_id.clone());
                result.candidates.push(candidate);
            }
            result.surfaces.push(selection);
        }
    }
    result
        .candidates
        .sort_by(|a, b| a.plugin_id.cmp(&b.plugin_id));
    result
        .surfaces
        .sort_by(|a, b| (&a.owner, &a.entry).cmp(&(&b.owner, &b.entry)));
    Ok(result)
}

fn inside(root: &Path, relative: &str) -> anyhow::Result<PathBuf> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!("surface paths must be relative and stay inside their owner");
    }
    let mut current = root.to_path_buf();
    for part in relative.components() {
        current.push(part);
        if fs::symlink_metadata(&current)?.file_type().is_symlink() {
            bail!("surface paths cannot traverse symbolic links");
        }
    }
    let path = fs::canonicalize(current)?;
    if !path.starts_with(fs::canonicalize(root)?) {
        bail!("surface path escapes owner");
    }
    Ok(path)
}
