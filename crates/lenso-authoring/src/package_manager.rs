use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;

use crate::{AuthoringError, PackageInput, PackageSource};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedPackage {
    revision: String,
}

impl ResolvedPackage {
    pub(crate) fn revision(&self) -> &str {
        &self.revision
    }
}

pub(crate) fn resolve_package(
    root: &Path,
    package: &PackageInput,
) -> Result<ResolvedPackage, AuthoringError> {
    match package.source() {
        PackageSource::Cargo => resolve_cargo(root, package),
        PackageSource::Npm => resolve_npm(root, package),
        PackageSource::Bun => resolve_bun(root, package),
        PackageSource::Oci => resolve_oci(package),
    }
}

fn manifest_path(root: &Path, package: &PackageInput) -> Result<PathBuf, AuthoringError> {
    let relative = package
        .manifest()
        .ok_or_else(|| AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: "package-manager manifest path is required".to_owned(),
        })?;
    let path = root.join(relative);
    if !path.is_file() {
        return Err(AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: format!("manifest {} does not exist", path.display()),
        });
    }
    Ok(path)
}

fn lockfile_path(
    root: &Path,
    package: &PackageInput,
    default_name: &str,
) -> Result<PathBuf, AuthoringError> {
    let path = package.lockfile().map_or_else(
        || {
            manifest_path(root, package).map(|manifest| {
                manifest
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join(default_name)
            })
        },
        |path| Ok(root.join(path)),
    )?;
    if !path.is_file() {
        return Err(AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: format!("lockfile {} does not exist", path.display()),
        });
    }
    Ok(path)
}

fn resolve_cargo(root: &Path, package: &PackageInput) -> Result<ResolvedPackage, AuthoringError> {
    let lockfile = lockfile_path(root, package, "Cargo.lock")?;
    let contents = fs::read_to_string(&lockfile).map_err(|source| AuthoringError::Io {
        path: lockfile.clone(),
        source,
    })?;
    let versions = cargo_locked_versions(&contents, package.package_name());
    select_version(package, &lockfile, versions)
}

fn cargo_locked_versions(contents: &str, package_name: &str) -> Vec<String> {
    contents
        .split("[[package]]")
        .filter_map(|block| {
            let mut name = None;
            let mut version = None;
            for line in block.lines() {
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                match key.trim() {
                    "name" => name = quoted(value.trim()),
                    "version" => version = quoted(value.trim()),
                    _ => {}
                }
            }
            (name.as_deref() == Some(package_name))
                .then_some(version)
                .flatten()
        })
        .collect()
}

fn quoted(value: &str) -> Option<String> {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .map(ToOwned::to_owned)
}

fn resolve_npm(root: &Path, package: &PackageInput) -> Result<ResolvedPackage, AuthoringError> {
    let lockfile = lockfile_path(root, package, "package-lock.json")?;
    let document: Value =
        serde_json::from_slice(&fs::read(&lockfile).map_err(|source| AuthoringError::Io {
            path: lockfile.clone(),
            source,
        })?)
        .map_err(|source| AuthoringError::Json {
            path: lockfile.clone(),
            source,
        })?;
    let mut versions = Vec::new();
    if let Some(packages) = document.get("packages").and_then(Value::as_object) {
        for (path, entry) in packages {
            let name = entry.get("name").and_then(Value::as_str).or_else(|| {
                path.strip_prefix("node_modules/")
                    .filter(|name| !name.contains("/node_modules/"))
            });
            if name == Some(package.package_name())
                && let Some(version) = entry.get("version").and_then(Value::as_str)
            {
                versions.push(version.to_owned());
            }
        }
    }
    if let Some(entry) = document
        .get("dependencies")
        .and_then(Value::as_object)
        .and_then(|dependencies| dependencies.get(package.package_name()))
        && let Some(version) = entry.get("version").and_then(Value::as_str)
    {
        versions.push(version.to_owned());
    }
    versions.sort();
    versions.dedup();
    select_version(package, &lockfile, versions)
}

fn resolve_bun(root: &Path, package: &PackageInput) -> Result<ResolvedPackage, AuthoringError> {
    let manifest = manifest_path(root, package)?;
    let lockfile = lockfile_path(root, package, "bun.lock")?;
    let working_directory = manifest.parent().unwrap_or_else(|| Path::new("."));
    let output = Command::new("bun")
        .args(["pm", "ls", "--all"])
        .current_dir(working_directory)
        .output()
        .map_err(|source| AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: format!("could not run bun pm ls: {source}"),
        })?;
    if !output.status.success() {
        return Err(AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: format!(
                "bun pm ls failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    let package_name = package.package_name();
    let versions = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.trim()
                .trim_start_matches(['├', '└', '─', ' '])
                .rsplit_once('@')
        })
        .filter(|(name, _)| name.trim() == package_name)
        .map(|(_, version)| version.trim().to_owned())
        .collect();
    select_version(package, &lockfile, versions)
}

fn resolve_oci(package: &PackageInput) -> Result<ResolvedPackage, AuthoringError> {
    if !package.version().starts_with("sha256:") {
        return Err(AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: "OCI inputs must use an immutable sha256 digest".to_owned(),
        });
    }
    Ok(ResolvedPackage {
        revision: package.version().to_owned(),
    })
}

fn select_version(
    package: &PackageInput,
    lockfile: &Path,
    versions: Vec<String>,
) -> Result<ResolvedPackage, AuthoringError> {
    let matches = versions
        .into_iter()
        .filter(|version| version == package.locked_revision())
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [version] => Ok(ResolvedPackage {
            revision: version.clone(),
        }),
        [] => Err(AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: format!(
                "{} does not lock {} at requested version {}",
                lockfile.display(),
                package.package_name(),
                package.locked_revision()
            ),
        }),
        _ => Err(AuthoringError::PackageManager {
            package: package.name().to_owned(),
            detail: format!(
                "{} contains ambiguous entries for {}@{}",
                lockfile.display(),
                package.package_name(),
                package.locked_revision()
            ),
        }),
    }
}
