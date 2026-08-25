use std::{
    fmt::{self, Write as _},
    fs,
    path::{Path, PathBuf},
};

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    AddModule, PROJECT_SCHEMA_VERSION, PackageInput, PackageSource, ProjectFile, ProjectPath,
    canonical_pretty_json, sort_json_value,
};

#[derive(Debug)]
pub enum AuthoringError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    UnsupportedProjectSchema {
        actual: u32,
    },
    DuplicateModule {
        key: String,
    },
    MissingPackageInput {
        package: String,
    },
    LockMismatch {
        package: String,
        detail: String,
    },
    PackageManager {
        package: String,
        detail: String,
    },
    UnavailableExecutionClass {
        instance: String,
        execution_class: String,
    },
    MissingEntrypoint {
        instance: String,
    },
    SecretValue {
        path: String,
    },
    InvalidConfiguration {
        path: String,
        detail: String,
    },
    Contract {
        path: PathBuf,
        detail: String,
    },
    Plan {
        detail: String,
    },
    InvalidProfile {
        profile: String,
        detail: String,
    },
    Recipe {
        path: PathBuf,
        detail: String,
    },
    ModuleDescriptor {
        path: PathBuf,
        detail: String,
    },
    Runner {
        source: lenso_kernel::PlanValidationError,
    },
    PlanJson {
        source: serde_json::Error,
    },
    NonCanonicalPlan,
}

impl fmt::Display for AuthoringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::Json { path, source } => write!(formatter, "{}: {source}", path.display()),
            Self::UnsupportedProjectSchema { actual } => write!(
                formatter,
                "unsupported authoring project schema {actual}; expected {PROJECT_SCHEMA_VERSION}"
            ),
            Self::DuplicateModule { key } => write!(formatter, "duplicate Module Instance {key}"),
            Self::MissingPackageInput { package } => write!(
                formatter,
                "Module package {package} has no package-manager input"
            ),
            Self::LockMismatch { package, detail } => {
                write!(formatter, "package {package} lock mismatch: {detail}")
            }
            Self::PackageManager { package, detail } => {
                write!(
                    formatter,
                    "package manager could not resolve {package}: {detail}"
                )
            }
            Self::UnavailableExecutionClass {
                instance,
                execution_class,
            } => write!(
                formatter,
                "Module Instance {instance} requires unavailable Execution Adapter {execution_class}"
            ),
            Self::MissingEntrypoint { instance } => write!(
                formatter,
                "Bun Module Instance {instance} needs a script entrypoint"
            ),
            Self::SecretValue { path } => write!(
                formatter,
                "configuration {path} contains a secret value; use a secret reference"
            ),
            Self::InvalidConfiguration { path, detail } => {
                write!(formatter, "invalid configuration {path}: {detail}")
            }
            Self::Contract { path, detail } => {
                write!(formatter, "contract {}: {detail}", path.display())
            }
            Self::Plan { detail } => {
                write!(formatter, "App Composition could not resolve: {detail}")
            }
            Self::InvalidProfile { profile, detail } => {
                write!(formatter, "invalid authoring profile {profile}: {detail}")
            }
            Self::Recipe { path, detail } => {
                write!(formatter, "Composition recipe {}: {detail}", path.display())
            }
            Self::ModuleDescriptor { path, detail } => {
                write!(formatter, "Module Descriptor {}: {detail}", path.display())
            }
            Self::Runner { source } => {
                write!(formatter, "Runner rejected the resolved Plan: {source}")
            }
            Self::PlanJson { source } => write!(formatter, "invalid Resolved App Plan: {source}"),
            Self::NonCanonicalPlan => formatter.write_str("Resolved App Plan is not canonical"),
        }
    }
}

impl std::error::Error for AuthoringError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json { source, .. } | Self::PlanJson { source } => Some(source),
            Self::Runner { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AddResult {
    changed_files: Vec<PathBuf>,
}

impl AddResult {
    pub fn changed_files(&self) -> &[PathBuf] {
        &self.changed_files
    }
}

pub fn sha256_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(7 + digest.len() * 2);
    value.push_str("sha256:");
    for byte in digest {
        let _ = write!(value, "{byte:02x}");
    }
    value
}

pub fn sha256_file(path: &Path) -> Result<String, AuthoringError> {
    Ok(sha256_bytes(&read_file(path)?))
}

fn read_file(path: &Path) -> Result<Vec<u8>, AuthoringError> {
    fs::read(path).map_err(|source| AuthoringError::Io {
        path: path.to_owned(),
        source,
    })
}

fn write_file(path: &Path, contents: &[u8]) -> Result<(), AuthoringError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| AuthoringError::Io {
            path: parent.to_owned(),
            source,
        })?;
    }
    fs::write(path, contents).map_err(|source| AuthoringError::Io {
        path: path.to_owned(),
        source,
    })
}

impl ProjectPath {
    pub fn load(path: &Path) -> Result<ProjectFile, AuthoringError> {
        serde_json::from_slice(&read_file(path)?).map_err(|source| AuthoringError::Json {
            path: path.to_owned(),
            source,
        })
    }

    pub fn add(&self, request: &AddModule) -> Result<AddResult, AuthoringError> {
        let original_project = read_file(self.path())?;
        let mut project = Self::load(self.path())?;
        add_module(&mut project, request)?;
        let root = self.path().parent().unwrap_or_else(|| Path::new("."));
        let mut changed_files = vec![self.path().to_owned()];
        let manifest_path = project
            .packages()
            .get(request.package().name())
            .and_then(PackageInput::manifest)
            .map(|manifest| root.join(manifest));
        if let Some(path) = &manifest_path
            && !path.is_file()
        {
            return Err(AuthoringError::Io {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "selected package manifest does not exist",
                ),
            });
        }
        let manifest_contents = manifest_path
            .as_deref()
            .map(|path| package_manifest_contents(path, request.package()))
            .transpose()?
            .flatten();
        write_file(self.path(), &canonical_pretty_json(&project))?;
        if let (Some(path), Some(contents)) = (manifest_path, manifest_contents) {
            if let Err(error) = write_file(&path, &contents) {
                let _ = write_file(self.path(), &original_project);
                return Err(error);
            }
            changed_files.push(path);
        }
        Ok(AddResult { changed_files })
    }
}

fn add_module(project: &mut ProjectFile, request: &AddModule) -> Result<(), AuthoringError> {
    if project
        .composition()
        .modules()
        .iter()
        .any(|module| module.key() == request.module().key())
    {
        return Err(AuthoringError::DuplicateModule {
            key: request.module().key().to_owned(),
        });
    }
    if request.module().package() != request.package().name() {
        return Err(AuthoringError::LockMismatch {
            package: request.package().name().to_owned(),
            detail: format!("Module selects {}", request.module().package()),
        });
    }
    if let Some(existing) = project.packages().get(request.package().name()) {
        if existing != request.package() {
            return Err(AuthoringError::LockMismatch {
                package: request.package().name().to_owned(),
                detail: "package input already has different authoring data".to_owned(),
            });
        }
    } else {
        project.packages_mut().insert(
            request.package().name().to_owned(),
            request.package().clone(),
        );
    }
    project
        .composition_mut()
        .add_module(request.module().clone());
    Ok(())
}

fn package_manifest_contents(
    path: &Path,
    package: &PackageInput,
) -> Result<Option<Vec<u8>>, AuthoringError> {
    match package.source() {
        PackageSource::Cargo => cargo_manifest_contents(path, package),
        PackageSource::Bun | PackageSource::Npm => json_manifest_contents(path, package),
        PackageSource::Oci => Ok(None),
    }
}

fn cargo_manifest_contents(
    path: &Path,
    package: &PackageInput,
) -> Result<Option<Vec<u8>>, AuthoringError> {
    let original =
        String::from_utf8(read_file(path)?).map_err(|error| AuthoringError::LockMismatch {
            package: package.name().to_owned(),
            detail: format!("Cargo manifest is not UTF-8: {error}"),
        })?;
    let package_name = package.package_name().to_owned();
    let dependency = format!("{package_name} = \"{}\"", package.version());
    let mut lines: Vec<String> = original.lines().map(ToOwned::to_owned).collect();
    let mut in_dependencies = false;
    let mut changed = false;
    let mut insert_at = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_dependencies = trimmed == "[dependencies]";
        }
        if in_dependencies {
            if trimmed.starts_with(&format!("{package_name} =")) {
                if trimmed == dependency {
                    return Ok(None);
                }
                let right_hand_side = trimmed
                    .split_once('=')
                    .map_or("", |(_, value)| value.trim_start());
                if !is_simple_cargo_version(right_hand_side) {
                    return Err(AuthoringError::LockMismatch {
                        package: package_name.clone(),
                        detail: "existing Cargo dependency declaration is not a simple version"
                            .to_owned(),
                    });
                }
                lines[index].clone_from(&dependency);
                changed = true;
                break;
            }
            insert_at = Some(index + 1);
        }
    }
    if !changed {
        if let Some(index) = insert_at {
            lines.insert(index, dependency);
        } else {
            if !lines.is_empty() && lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push("[dependencies]".to_owned());
            lines.push(dependency);
        }
        changed = true;
    }
    debug_assert!(changed);
    let mut rendered = lines.join("\n");
    rendered.push('\n');
    Ok(Some(rendered.into_bytes()))
}

fn is_simple_cargo_version(value: &str) -> bool {
    let Some(value) = value.strip_prefix('"') else {
        return false;
    };
    let Some(end) = value.find('"') else {
        return false;
    };
    value[end + 1..].trim().is_empty()
}

fn json_manifest_contents(
    path: &Path,
    package: &PackageInput,
) -> Result<Option<Vec<u8>>, AuthoringError> {
    let mut document: Value =
        serde_json::from_slice(&read_file(path)?).map_err(|source| AuthoringError::Json {
            path: path.to_owned(),
            source,
        })?;
    let object = document
        .as_object_mut()
        .ok_or_else(|| AuthoringError::LockMismatch {
            package: package.name().to_owned(),
            detail: "package manifest must be a JSON object".to_owned(),
        })?;
    let section = "dependencies";
    let dependencies = object
        .entry(section.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    let dependencies =
        dependencies
            .as_object_mut()
            .ok_or_else(|| AuthoringError::LockMismatch {
                package: package.name().to_owned(),
                detail: format!("manifest field {section} must be an object"),
            })?;
    let old = dependencies.insert(
        package.package_name().to_owned(),
        Value::String(package.version().to_owned()),
    );
    if old.is_some_and(|value| value == Value::String(package.version().to_owned())) {
        return Ok(None);
    }
    sort_json_value(&mut document);
    Ok(Some(
        serde_json::to_vec_pretty(&document).expect("JSON values are serializable"),
    ))
}
