use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::{Value, json};

use crate::{AuthoringError, CargoAppDefinition, inspect_cargo_module};

#[derive(Clone, Debug, Default)]
pub struct CargoModuleSource {
    pub version: Option<String>,
    pub git: Option<String>,
    pub rev: Option<String>,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub path: Option<PathBuf>,
}

impl CargoModuleSource {
    fn is_unspecified(&self) -> bool {
        self.version.is_none()
            && self.git.is_none()
            && self.rev.is_none()
            && self.branch.is_none()
            && self.tag.is_none()
            && self.path.is_none()
    }

    fn append_to(&self, command: &mut Command) {
        if let Some(git) = &self.git {
            command.arg("--git").arg(git);
        }
        if let Some(rev) = &self.rev {
            command.arg("--rev").arg(rev);
        }
        if let Some(branch) = &self.branch {
            command.arg("--branch").arg(branch);
        }
        if let Some(tag) = &self.tag {
            command.arg("--tag").arg(tag);
        }
        if let Some(path) = &self.path {
            command.arg("--path").arg(path);
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppAddRequest {
    pub cargo_package: String,
    pub key: Option<String>,
    pub entrypoint: String,
    pub configuration: Value,
    pub execution_lane: Option<String>,
    pub source: CargoModuleSource,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct AppRemoveRequest {
    pub key: String,
    pub uninstall: bool,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct AppEditResult {
    pub key: String,
    pub runtime_package: String,
    pub cargo_package: String,
    pub changed_files: Vec<PathBuf>,
    pub dry_run: bool,
    pub dependency_changed: bool,
}

#[derive(Debug)]
struct FileSnapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

impl FileSnapshot {
    fn capture(path: PathBuf) -> Result<Self, AuthoringError> {
        let contents = match fs::read(&path) {
            Ok(contents) => Some(contents),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => return Err(AuthoringError::Io { path, source }),
        };
        Ok(Self { path, contents })
    }

    fn restore(&self) -> Result<(), AuthoringError> {
        match &self.contents {
            Some(contents) => {
                fs::write(&self.path, contents).map_err(|source| AuthoringError::Io {
                    path: self.path.clone(),
                    source,
                })
            }
            None => match fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(AuthoringError::Io {
                    path: self.path.clone(),
                    source,
                }),
            },
        }
    }
}

#[derive(Debug)]
struct CargoWorkspace {
    manifest: PathBuf,
    host_manifest: PathBuf,
    lockfile: PathBuf,
    host_package: String,
}

pub fn add_app_module(
    definition_path: &Path,
    request: &AppAddRequest,
) -> Result<AppEditResult, AuthoringError> {
    let definition = CargoAppDefinition::load(definition_path)?;
    validate_source(&request.source, definition_path)?;
    ensure_key_available(&definition, request.key.as_deref(), definition_path)?;
    let workspace = cargo_workspace(definition_path, &definition)?;
    let dependency_already_mapped = definition
        .packages()
        .values()
        .any(|package| package == &request.cargo_package);
    let add_dependency = !dependency_already_mapped || !request.source.is_unspecified();
    let snapshots = snapshots(definition_path, &workspace)?;

    let operation = (|| {
        if add_dependency {
            let mut command = cargo_command("add");
            let dependency = request.source.version.as_ref().map_or_else(
                || request.cargo_package.clone(),
                |version| format!("{}@{version}", request.cargo_package),
            );
            command
                .arg(dependency)
                .arg("--manifest-path")
                .arg(&workspace.manifest)
                .arg("--package")
                .arg(&workspace.host_package);
            request.source.append_to(&mut command);
            run_cargo(&mut command, definition_path, &request.cargo_package)?;
        }

        let descriptor = inspect_cargo_module(
            &workspace.manifest,
            &workspace.host_package,
            &request.cargo_package,
            &request.entrypoint,
        )?;
        let runtime_package = descriptor.package_id().to_owned();
        let key = request.key.clone().unwrap_or_else(|| {
            runtime_package
                .rsplit('.')
                .next()
                .filter(|value| !value.is_empty())
                .unwrap_or(&request.cargo_package)
                .to_owned()
        });
        if definition
            .app()
            .modules()
            .iter()
            .any(|module| module.key() == key)
        {
            return Err(app_error(
                definition_path,
                format!("Module Instance key `{key}` already exists; pass --key to choose another"),
            ));
        }
        if let Some(existing) = definition.packages().get(&runtime_package)
            && existing != &request.cargo_package
        {
            return Err(app_error(
                definition_path,
                format!(
                    "runtime package `{runtime_package}` is already mapped to Cargo package `{existing}`"
                ),
            ));
        }

        let mut document = load_document(definition_path)?;
        packages_mut(&mut document, definition_path)?
            .insert(runtime_package.clone(), json!(request.cargo_package));
        let mut selection = json!({
            "key": key,
            "package": runtime_package,
            "configuration": request.configuration,
        });
        if request.entrypoint != "default" {
            selection["entrypoint"] = json!(request.entrypoint);
        }
        if let Some(lane) = &request.execution_lane {
            selection["execution_lane"] = json!(lane);
        }
        modules_mut(&mut document, definition_path)?.push(selection);
        sort_modules(&mut document, definition_path)?;
        let candidate = parse_candidate(&document, definition_path)?;
        candidate.resolve(definition_root(definition_path))?;
        write_document(definition_path, &document)?;

        Ok(AppEditResult {
            key,
            runtime_package,
            cargo_package: request.cargo_package.clone(),
            changed_files: changed_files(&snapshots),
            dry_run: request.dry_run,
            dependency_changed: add_dependency,
        })
    })();

    finish_transaction(operation, &snapshots, request.dry_run)
}

pub fn remove_app_module(
    definition_path: &Path,
    request: &AppRemoveRequest,
) -> Result<AppEditResult, AuthoringError> {
    let definition = CargoAppDefinition::load(definition_path)?;
    let selected = definition
        .app()
        .modules()
        .iter()
        .find(|module| module.key() == request.key)
        .ok_or_else(|| {
            app_error(
                definition_path,
                format!("Module Instance key `{}` does not exist", request.key),
            )
        })?;
    let runtime_package = selected.package().to_owned();
    let cargo_package = definition
        .packages()
        .get(&runtime_package)
        .cloned()
        .ok_or_else(|| {
            app_error(
                definition_path,
                format!("runtime package `{runtime_package}` has no Cargo package mapping"),
            )
        })?;
    let runtime_still_used = definition
        .app()
        .modules()
        .iter()
        .any(|module| module.key() != request.key && module.package() == runtime_package);
    let cargo_still_used = definition.app().modules().iter().any(|module| {
        module.key() != request.key
            && definition.packages().get(module.package()) == Some(&cargo_package)
    });
    if request.uninstall && cargo_still_used {
        return Err(app_error(
            definition_path,
            format!(
                "Cargo package `{cargo_package}` is still used by another Module Instance; remove it without --uninstall"
            ),
        ));
    }

    let workspace = cargo_workspace(definition_path, &definition)?;
    let snapshots = snapshots(definition_path, &workspace)?;
    let operation = (|| {
        let mut document = load_document(definition_path)?;
        modules_mut(&mut document, definition_path)?.retain(|module| {
            module.get("key").and_then(Value::as_str) != Some(request.key.as_str())
        });
        if !runtime_still_used {
            packages_mut(&mut document, definition_path)?.remove(&runtime_package);
        }
        if request.uninstall {
            packages_mut(&mut document, definition_path)?
                .retain(|_, package| package.as_str() != Some(&cargo_package));
        }
        let candidate = parse_candidate(&document, definition_path)?;
        candidate.resolve(definition_root(definition_path))?;

        if request.uninstall {
            let mut command = cargo_command("remove");
            command
                .arg(&cargo_package)
                .arg("--manifest-path")
                .arg(&workspace.manifest)
                .arg("--package")
                .arg(&workspace.host_package);
            run_cargo(&mut command, definition_path, &cargo_package)?;
            candidate.resolve(definition_root(definition_path))?;
        }
        write_document(definition_path, &document)?;

        Ok(AppEditResult {
            key: request.key.clone(),
            runtime_package,
            cargo_package,
            changed_files: changed_files(&snapshots),
            dry_run: request.dry_run,
            dependency_changed: request.uninstall,
        })
    })();

    finish_transaction(operation, &snapshots, request.dry_run)
}

fn validate_source(source: &CargoModuleSource, path: &Path) -> Result<(), AuthoringError> {
    let locations = usize::from(source.git.is_some()) + usize::from(source.path.is_some());
    if locations > 1 {
        return Err(app_error(path, "--git and --path are mutually exclusive"));
    }
    if source.version.is_some() && locations > 0 {
        return Err(app_error(
            path,
            "--version cannot be combined with --git or --path",
        ));
    }
    let selectors = usize::from(source.rev.is_some())
        + usize::from(source.branch.is_some())
        + usize::from(source.tag.is_some());
    if selectors > 1 {
        return Err(app_error(
            path,
            "--rev, --branch, and --tag are mutually exclusive",
        ));
    }
    if selectors > 0 && source.git.is_none() {
        return Err(app_error(path, "--rev, --branch, and --tag require --git"));
    }
    Ok(())
}

fn ensure_key_available(
    definition: &CargoAppDefinition,
    key: Option<&str>,
    path: &Path,
) -> Result<(), AuthoringError> {
    if let Some(key) = key
        && definition
            .app()
            .modules()
            .iter()
            .any(|module| module.key() == key)
    {
        return Err(app_error(
            path,
            format!("Module Instance key `{key}` already exists"),
        ));
    }
    Ok(())
}

fn cargo_workspace(
    definition_path: &Path,
    definition: &CargoAppDefinition,
) -> Result<CargoWorkspace, AuthoringError> {
    let host_package = definition.host_package().ok_or_else(|| {
        app_error(
            definition_path,
            "app add/remove requires `host_package` for a statically linked Cargo Host",
        )
    })?;
    let manifest = definition_root(definition_path).join(definition.manifest());
    let output = cargo_command("metadata")
        .args(["--no-deps", "--format-version", "1", "--manifest-path"])
        .arg(&manifest)
        .output()
        .map_err(|source| AuthoringError::Io {
            path: manifest.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(app_error(
            definition_path,
            format!(
                "Cargo metadata failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        ));
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).map_err(|source| AuthoringError::Json {
            path: manifest.clone(),
            source,
        })?;
    let host_manifest = metadata["packages"]
        .as_array()
        .and_then(|packages| {
            packages.iter().find_map(|package| {
                (package["name"].as_str() == Some(host_package))
                    .then(|| package["manifest_path"].as_str())
                    .flatten()
            })
        })
        .map(PathBuf::from)
        .ok_or_else(|| {
            app_error(
                definition_path,
                format!("Cargo package `{host_package}` does not exist in the workspace"),
            )
        })?;
    let workspace_root = metadata["workspace_root"]
        .as_str()
        .ok_or_else(|| app_error(definition_path, "Cargo metadata omitted `workspace_root`"))?;
    Ok(CargoWorkspace {
        manifest,
        host_manifest,
        lockfile: Path::new(workspace_root).join("Cargo.lock"),
        host_package: host_package.to_owned(),
    })
}

fn snapshots(
    definition_path: &Path,
    workspace: &CargoWorkspace,
) -> Result<Vec<FileSnapshot>, AuthoringError> {
    [
        definition_path.to_owned(),
        workspace.host_manifest.clone(),
        workspace.lockfile.clone(),
    ]
    .into_iter()
    .collect::<BTreeSet<_>>()
    .into_iter()
    .map(FileSnapshot::capture)
    .collect()
}

fn finish_transaction(
    result: Result<AppEditResult, AuthoringError>,
    snapshots: &[FileSnapshot],
    dry_run: bool,
) -> Result<AppEditResult, AuthoringError> {
    if result.is_err() || dry_run {
        for snapshot in snapshots.iter().rev() {
            snapshot.restore()?;
        }
    }
    result
}

fn changed_files(snapshots: &[FileSnapshot]) -> Vec<PathBuf> {
    snapshots
        .iter()
        .filter_map(|snapshot| {
            let current = fs::read(&snapshot.path).ok();
            (current != snapshot.contents).then(|| snapshot.path.clone())
        })
        .collect()
}

fn cargo_command(subcommand: &str) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command.arg(subcommand);
    command
}

fn run_cargo(
    command: &mut Command,
    definition_path: &Path,
    package: &str,
) -> Result<(), AuthoringError> {
    let output = command.output().map_err(|source| AuthoringError::Io {
        path: definition_path.to_owned(),
        source,
    })?;
    if output.status.success() {
        return Ok(());
    }
    Err(app_error(
        definition_path,
        format!(
            "Cargo could not update `{package}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
    ))
}

fn definition_root(path: &Path) -> &Path {
    path.parent().unwrap_or_else(|| Path::new("."))
}

fn load_document(path: &Path) -> Result<Value, AuthoringError> {
    let bytes = fs::read(path).map_err(|source| AuthoringError::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| AuthoringError::Json {
        path: path.to_owned(),
        source,
    })
}

fn parse_candidate(value: &Value, path: &Path) -> Result<CargoAppDefinition, AuthoringError> {
    serde_json::from_value(value.clone()).map_err(|source| AuthoringError::Json {
        path: path.to_owned(),
        source,
    })
}

fn modules_mut<'a>(
    document: &'a mut Value,
    path: &Path,
) -> Result<&'a mut Vec<Value>, AuthoringError> {
    document
        .pointer_mut("/app/modules")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| app_error(path, "`app.modules` must be an array"))
}

fn packages_mut<'a>(
    document: &'a mut Value,
    path: &Path,
) -> Result<&'a mut serde_json::Map<String, Value>, AuthoringError> {
    let root = document
        .as_object_mut()
        .ok_or_else(|| app_error(path, "document root must be an object"))?;
    let packages = root
        .entry("packages")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    packages
        .as_object_mut()
        .ok_or_else(|| app_error(path, "`packages` must be an object"))
}

fn sort_modules(document: &mut Value, path: &Path) -> Result<(), AuthoringError> {
    modules_mut(document, path)?.sort_by(|left, right| {
        left["key"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["key"].as_str().unwrap_or_default())
    });
    Ok(())
}

fn write_document(path: &Path, document: &Value) -> Result<(), AuthoringError> {
    let mut bytes = serde_json::to_vec_pretty(document).map_err(|source| AuthoringError::Json {
        path: path.to_owned(),
        source,
    })?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|source| AuthoringError::Io {
        path: path.to_owned(),
        source,
    })
}

fn app_error(path: &Path, detail: impl Into<String>) -> AuthoringError {
    AuthoringError::ModuleDescriptor {
        path: path.to_owned(),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESCRIPTOR: &str = r#"{"package_id":"example.fixture","package_revision":"1.0.0","entrypoint":"default","configuration_schema":{"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":false},"provided_capabilities":[],"required_capabilities":[],"execution_class":"lenso.native-rust@1","restart_policy":{"mode":"never","max_attempts":0,"window":{"secs":0,"nanos":0},"backoff":{"secs":0,"nanos":0},"stability":{"secs":0,"nanos":0},"jitter":{"secs":0,"nanos":0}},"criticality":"non_critical"}"#;

    #[test]
    fn source_validation_rejects_ambiguous_locations() {
        let source = CargoModuleSource {
            git: Some("https://example.invalid/module".to_owned()),
            path: Some(PathBuf::from("module")),
            ..CargoModuleSource::default()
        };
        assert!(validate_source(&source, Path::new("lenso.app.json")).is_err());
    }

    #[test]
    fn source_validation_requires_git_for_revision_selectors() {
        let source = CargoModuleSource {
            rev: Some("abc123".to_owned()),
            ..CargoModuleSource::default()
        };
        assert!(validate_source(&source, Path::new("lenso.app.json")).is_err());
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn cargo_add_remove_and_dry_run_are_transactional() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        write(
            &root.join("Cargo.toml"),
            r#"[workspace]
resolver = "2"
members = ["host"]
exclude = ["module"]
"#,
        );
        write(
            &root.join("host/Cargo.toml"),
            r#"[package]
name = "fixture-host"
version = "0.1.0"
edition = "2024"

[dependencies]
"#,
        );
        write(&root.join("host/src/lib.rs"), "pub fn host() {}\n");
        write(
            &root.join("module/Cargo.toml"),
            r#"[package]
name = "fixture-module"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["rlib"]
"#,
        );
        write(
            &root.join("module/src/lib.rs"),
            r#"#[used]
pub static DESCRIPTOR: [u8; include_bytes!("descriptor.bin").len()] =
    *include_bytes!("descriptor.bin");
"#,
        );
        write(
            &root.join("module/src/descriptor.bin"),
            &format!("LENSO_MODULE_DESCRIPTOR_V1\0{DESCRIPTOR}\0END_LENSO_MODULE_DESCRIPTOR_V1"),
        );
        let definition_path = root.join("lenso.app.json");
        write(
            &definition_path,
            r#"{
  "schema_version": 1,
  "manifest": "Cargo.toml",
  "host_package": "fixture-host",
  "extensions": {
    "example.product": {
      "schema_version": 1,
      "enabled": ["fixture@1"]
    }
  },
  "packages": {},
  "app": {
    "name": "fixture",
    "modules": [],
    "decisions": []
  }
}
"#,
        );

        let valid_add = AppAddRequest {
            cargo_package: "fixture-module".to_owned(),
            key: None,
            entrypoint: "default".to_owned(),
            configuration: json!({"name": "fixture"}),
            execution_lane: None,
            source: CargoModuleSource {
                path: Some(root.join("module")),
                ..CargoModuleSource::default()
            },
            dry_run: false,
        };
        let added = add_app_module(&definition_path, &valid_add).unwrap();
        assert_eq!(added.key, "fixture");
        assert_eq!(added.runtime_package, "example.fixture");
        assert!(
            fs::read_to_string(root.join("host/Cargo.toml"))
                .unwrap()
                .contains("fixture-module")
        );
        assert_eq!(
            CargoAppDefinition::load(&definition_path)
                .unwrap()
                .app()
                .modules()
                .len(),
            1
        );
        assert_eq!(
            CargoAppDefinition::load(&definition_path)
                .unwrap()
                .extension("example.product")
                .unwrap()["enabled"],
            json!(["fixture@1"])
        );

        remove_app_module(
            &definition_path,
            &AppRemoveRequest {
                key: "fixture".to_owned(),
                uninstall: false,
                dry_run: false,
            },
        )
        .unwrap();
        assert_eq!(
            CargoAppDefinition::load(&definition_path)
                .unwrap()
                .extension("example.product")
                .unwrap()["enabled"],
            json!(["fixture@1"])
        );
        assert!(
            fs::read_to_string(root.join("host/Cargo.toml"))
                .unwrap()
                .contains("fixture-module")
        );

        let before_definition = fs::read(&definition_path).unwrap();
        let before_manifest = fs::read(root.join("host/Cargo.toml")).unwrap();
        let before_lock = fs::read(root.join("Cargo.lock")).unwrap();
        let mut invalid_add = valid_add.clone();
        invalid_add.configuration = json!({});
        assert!(add_app_module(&definition_path, &invalid_add).is_err());
        assert_eq!(fs::read(&definition_path).unwrap(), before_definition);
        assert_eq!(
            fs::read(root.join("host/Cargo.toml")).unwrap(),
            before_manifest
        );
        assert_eq!(fs::read(root.join("Cargo.lock")).unwrap(), before_lock);

        let mut dry_run = valid_add.clone();
        dry_run.dry_run = true;
        let preview = add_app_module(&definition_path, &dry_run).unwrap();
        assert!(preview.dry_run);
        assert_eq!(fs::read(&definition_path).unwrap(), before_definition);
        assert_eq!(
            fs::read(root.join("host/Cargo.toml")).unwrap(),
            before_manifest
        );
        assert_eq!(fs::read(root.join("Cargo.lock")).unwrap(), before_lock);

        add_app_module(&definition_path, &valid_add).unwrap();
        let second_add = AppAddRequest {
            key: Some("fixture-two".to_owned()),
            source: CargoModuleSource::default(),
            ..valid_add.clone()
        };
        add_app_module(&definition_path, &second_add).unwrap();
        assert!(
            remove_app_module(
                &definition_path,
                &AppRemoveRequest {
                    key: "fixture".to_owned(),
                    uninstall: true,
                    dry_run: false,
                },
            )
            .is_err()
        );
        remove_app_module(
            &definition_path,
            &AppRemoveRequest {
                key: "fixture-two".to_owned(),
                uninstall: false,
                dry_run: false,
            },
        )
        .unwrap();
        remove_app_module(
            &definition_path,
            &AppRemoveRequest {
                key: "fixture".to_owned(),
                uninstall: true,
                dry_run: false,
            },
        )
        .unwrap();
        assert!(
            !fs::read_to_string(root.join("host/Cargo.toml"))
                .unwrap()
                .contains("fixture-module")
        );
    }

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
}
