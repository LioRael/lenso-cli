use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use lenso_cli::Error;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::project::{atomic_write, read_bounded};
use crate::{
    Extension, HandlerSpec, Installation, LoadOptions, Project, ProjectCli, prepare_project,
    project_path,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Legacy {
    schema_version: u32,
    extensions: Vec<serde_json::Value>,
}

/// Migrate experimental installation records into visible sources, TOML, and a lock.
///
/// All packages are staged and validated before the new declaration is written.
/// Recognized legacy files are removed only after the new installation succeeds;
/// unrelated files and existing root declarations are preserved.
///
/// # Errors
/// Rejects existing root declarations, unsafe or changed sources, and invalid packages.
pub fn migrate_project(project: &Path) -> Result<usize, Error> {
    let project = project.canonicalize()?;
    if project_path(&project, Path::new("lenso.toml"))?.exists() {
        return Err(Error::Invalid(
            "lenso.toml already exists; merge legacy declarations manually".into(),
        ));
    }
    let legacy_path = project_path(&project, Path::new(".lenso/project.json"))?;
    let legacy: Legacy = serde_json::from_slice(&read_bounded(&legacy_path, 1024 * 1024)?)?;
    if legacy.schema_version != 1 {
        return Err(Error::Invalid("unsupported legacy schema_version".into()));
    }
    let stage = tempfile::tempdir_in(&project)?;
    let mut extensions = Vec::new();
    let mut packages = Vec::new();
    let mut managed = BTreeSet::from([legacy_path]);
    for mut policy in legacy.extensions {
        let manifest = policy
            .get("manifest")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| Error::Invalid("legacy installation is missing manifest".into()))?
            .to_owned();
        let source = project_path(&project, Path::new(&manifest))?;
        let definition: Extension = serde_json::from_slice(&read_bounded(&source, 1024 * 1024)?)?;
        let name = format!("legacy-{:x}", Sha256::digest(definition.id.as_bytes()));
        let directory = PathBuf::from("lenso/extensions").join(name);
        let target = project_path(&project, &directory)?;
        if target.exists() {
            return Err(Error::Invalid(format!(
                "migration source target already exists: {}",
                directory.display()
            )));
        }
        let staging = stage.path().join(&directory);
        std::fs::create_dir_all(&staging)?;
        for handler in definition.handlers.values() {
            if let HandlerSpec::Wasm(spec) = handler {
                let old = project_path(
                    &project,
                    &Path::new(&manifest)
                        .parent()
                        .unwrap_or(Path::new(""))
                        .join(&spec.module),
                )?;
                let bytes = read_bounded(&old, 16 * 1024 * 1024)?;
                if spec.sha256 != "auto" && crate::cache::digest(&bytes) != spec.sha256 {
                    return Err(Error::Permission(
                        "legacy module digest mismatch; original files were preserved".into(),
                    ));
                }
                let destination = project_path(&staging, Path::new(&spec.module))?;
                std::fs::create_dir_all(
                    destination
                        .parent()
                        .ok_or_else(|| Error::Invalid("module has no parent".into()))?,
                )?;
                std::fs::write(destination, bytes)?;
                if old.starts_with(project.join(".lenso")) {
                    managed.insert(old);
                }
            }
        }
        std::fs::write(
            staging.join("extension.json"),
            serde_json::to_vec_pretty(&definition)?,
        )?;
        let object = policy
            .as_object_mut()
            .ok_or_else(|| Error::Invalid("legacy installation must be an object".into()))?;
        object.remove("manifest");
        object.insert("id".into(), serde_json::Value::String(definition.id));
        object.insert(
            "source".into(),
            serde_json::Value::String(
                directory
                    .join("extension.json")
                    .to_string_lossy()
                    .replace('\\', "/"),
            ),
        );
        extensions.push(serde_json::from_value::<Installation>(policy)?);
        packages.push((staging, target));
        if source.starts_with(project.join(".lenso")) {
            managed.insert(source);
        }
    }
    let config = Project {
        cli: ProjectCli {
            schema_version: 1,
            extensions,
            registries: Default::default(),
            github: Default::default(),
        },
    };
    let declaration = toml::to_string_pretty(&config).map_err(|e| Error::Invalid(e.to_string()))?;
    atomic_write(&stage.path().join("lenso.toml"), declaration.as_bytes())?;
    let count = prepare_project(stage.path(), &LoadOptions::default())?;
    let lock = read_bounded(&stage.path().join("lenso.lock"), 8 * 1024 * 1024)?;
    for (from, to) in packages {
        std::fs::create_dir_all(
            to.parent()
                .ok_or_else(|| Error::Invalid("package target has no parent".into()))?,
        )?;
        std::fs::rename(from, to)?;
    }
    atomic_write(&project.join("lenso.toml"), declaration.as_bytes())?;
    atomic_write(&project.join("lenso.lock"), &lock)?;
    prepare_project(&project, &LoadOptions::default())?;
    for path in managed {
        std::fs::remove_file(path)?;
    }
    Ok(count)
}
