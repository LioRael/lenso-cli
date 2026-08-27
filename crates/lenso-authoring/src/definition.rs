use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use lenso_app_plan::{
    AppComposition, ResolvedAppPlan,
    authoring::{AppDefinition, ModuleCatalog, ModuleDescriptor},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AuthoringError, canonical_json_bytes};

const APP_DEFINITION_SCHEMA_VERSION: u32 = 1;
const DESCRIPTOR_START: &[u8] = b"LENSO_MODULE_DESCRIPTOR_V1\0";
const DESCRIPTOR_END: &[u8] = b"\0END_LENSO_MODULE_DESCRIPTOR_V1";
const CARGO_PACKAGE_BUILD_ENVIRONMENT: &[&str] = &[
    "CARGO_BIN_NAME",
    "CARGO_CRATE_NAME",
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_PKG_AUTHORS",
    "CARGO_PKG_DESCRIPTION",
    "CARGO_PKG_HOMEPAGE",
    "CARGO_PKG_LICENSE",
    "CARGO_PKG_LICENSE_FILE",
    "CARGO_PKG_NAME",
    "CARGO_PKG_README",
    "CARGO_PKG_REPOSITORY",
    "CARGO_PKG_RUST_VERSION",
    "CARGO_PKG_VERSION",
    "CARGO_PKG_VERSION_MAJOR",
    "CARGO_PKG_VERSION_MINOR",
    "CARGO_PKG_VERSION_PATCH",
    "CARGO_PKG_VERSION_PRE",
    "CARGO_PRIMARY_PACKAGE",
    "OUT_DIR",
];

fn default_definition_schema() -> u32 {
    APP_DEFINITION_SCHEMA_VERSION
}

fn default_manifest() -> String {
    "Cargo.toml".to_owned()
}

/// Cargo-backed App Definition document.
///
/// The package map names ordinary Cargo packages only. Capability facts,
/// execution classes, lifecycle policy, and bindings come from compiled
/// Module Descriptor artifacts rather than this document.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CargoAppDefinition {
    #[serde(default = "default_definition_schema")]
    schema_version: u32,
    #[serde(default = "default_manifest")]
    manifest: String,
    #[serde(default)]
    packages: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host_package: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    extensions: BTreeMap<String, Value>,
    app: AppDefinition,
}

impl CargoAppDefinition {
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn manifest(&self) -> &str {
        &self.manifest
    }

    pub fn packages(&self) -> &BTreeMap<String, String> {
        &self.packages
    }

    /// Cargo package whose dependency graph closes the statically linked Host.
    pub fn host_package(&self) -> Option<&str> {
        self.host_package.as_deref()
    }

    /// Product-owned App intent preserved without interpretation by generic authoring.
    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    /// Returns one namespaced product extension without assigning it generic semantics.
    pub fn extension(&self, name: &str) -> Option<&Value> {
        self.extensions.get(name)
    }

    pub const fn app(&self) -> &AppDefinition {
        &self.app
    }

    pub fn load(path: &Path) -> Result<Self, AuthoringError> {
        let bytes = fs::read(path).map_err(|source| AuthoringError::Io {
            path: path.to_owned(),
            source,
        })?;
        let definition =
            serde_json::from_slice::<Self>(&bytes).map_err(|source| AuthoringError::Json {
                path: path.to_owned(),
                source,
            })?;
        if definition.schema_version != APP_DEFINITION_SCHEMA_VERSION {
            return Err(AuthoringError::ModuleDescriptor {
                path: path.to_owned(),
                detail: format!(
                    "unsupported App Definition schema {}; expected {APP_DEFINITION_SCHEMA_VERSION}",
                    definition.schema_version
                ),
            });
        }
        Ok(definition)
    }

    pub fn derive(&self, root: &Path) -> Result<AppComposition, AuthoringError> {
        let manifest = root.join(&self.manifest);
        let selected_packages = self.selected_cargo_packages(&manifest)?;
        let artifacts = build_descriptor_artifacts(
            &manifest,
            selected_packages.iter().map(String::as_str),
            self.host_package(),
        )?;
        let catalog = catalog_from_artifacts(&artifacts)?;
        self.derive_with_catalog(&catalog)
    }

    /// Derives this App from Module Descriptors already linked into a Host.
    ///
    /// This is the normal runtime path for a statically linked product. Cargo
    /// artifact inspection remains available for authoring and validation.
    pub fn derive_with_catalog(
        &self,
        catalog: &ModuleCatalog,
    ) -> Result<AppComposition, AuthoringError> {
        self.selected_cargo_packages(Path::new(&self.manifest))?;
        self.app
            .derive(catalog)
            .map_err(|error| AuthoringError::ModuleDescriptor {
                path: PathBuf::from(&self.manifest),
                detail: error.to_string(),
            })
    }

    pub fn resolve(&self, root: &Path) -> Result<ResolvedAppPlan, AuthoringError> {
        self.derive(root)?
            .resolve()
            .map_err(|error| AuthoringError::Plan {
                detail: error.to_string(),
            })
    }

    pub fn resolve_canonical(&self, root: &Path) -> Result<Vec<u8>, AuthoringError> {
        self.resolve(root).map(|plan| canonical_json_bytes(&plan))
    }

    pub fn resolve_with_catalog_canonical(
        &self,
        catalog: &ModuleCatalog,
    ) -> Result<Vec<u8>, AuthoringError> {
        self.derive_with_catalog(catalog)?
            .resolve()
            .map(|plan| canonical_json_bytes(&plan))
            .map_err(|error| AuthoringError::Plan {
                detail: error.to_string(),
            })
    }

    fn selected_cargo_packages(&self, manifest: &Path) -> Result<BTreeSet<String>, AuthoringError> {
        self.app
            .modules()
            .iter()
            .map(|selection| {
                self.packages
                    .get(selection.package())
                    .cloned()
                    .ok_or_else(|| AuthoringError::ModuleDescriptor {
                        path: manifest.to_owned(),
                        detail: format!(
                            "Module selection `{}` has no Cargo package mapping for `{}`",
                            selection.key(),
                            selection.package()
                        ),
                    })
            })
            .collect()
    }
}

/// Builds a statically linked Host and reads one dependency's Descriptor
/// directly from its Cargo artifacts without loading or executing Module code.
pub fn inspect_cargo_module(
    manifest: &Path,
    host_package: &str,
    cargo_package: &str,
    entrypoint: &str,
) -> Result<ModuleDescriptor, AuthoringError> {
    let artifacts =
        build_descriptor_artifacts(manifest, std::iter::once(cargo_package), Some(host_package))?;
    let mut matches = Vec::new();
    for path in artifacts {
        let bytes = fs::read(&path).map_err(|source| AuthoringError::Io {
            path: path.clone(),
            source,
        })?;
        matches.extend(
            extract_descriptors(&bytes, &path)?
                .into_iter()
                .filter(|descriptor| descriptor.entrypoint() == entrypoint),
        );
    }
    matches.sort_by(|left, right| {
        left.package_id()
            .cmp(right.package_id())
            .then_with(|| left.package_revision().cmp(right.package_revision()))
    });
    matches.dedup();
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(AuthoringError::ModuleDescriptor {
            path: manifest.to_owned(),
            detail: format!(
                "Cargo package `{cargo_package}` exposes no Module Descriptor entrypoint `{entrypoint}`"
            ),
        }),
        count => Err(AuthoringError::ModuleDescriptor {
            path: manifest.to_owned(),
            detail: format!(
                "Cargo package `{cargo_package}` exposes {count} Module Descriptors for entrypoint `{entrypoint}`; select a package with one unambiguous descriptor"
            ),
        }),
    }
}

fn build_descriptor_artifacts<'a>(
    manifest: &Path,
    packages: impl Iterator<Item = &'a str>,
    host_package: Option<&str>,
) -> Result<Vec<PathBuf>, AuthoringError> {
    let packages = packages.map(ToOwned::to_owned).collect::<BTreeSet<_>>();
    let mut command = descriptor_build_command(manifest, &packages, host_package);
    let output = command.output().map_err(|source| AuthoringError::Io {
        path: manifest.to_owned(),
        source,
    })?;
    if !output.status.success() {
        return Err(AuthoringError::ModuleDescriptor {
            path: manifest.to_owned(),
            detail: format!(
                "Cargo could not build selected Module packages: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ),
        });
    }
    let mut artifacts = BTreeSet::new();
    for line in output.stdout.split(|byte| *byte == b'\n') {
        let Ok(message) = serde_json::from_slice::<Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        if !packages
            .iter()
            .any(|package| cargo_artifact_matches(&message, package))
        {
            continue;
        }
        let Some(filenames) = message.get("filenames").and_then(Value::as_array) else {
            continue;
        };
        for filename in filenames.iter().filter_map(Value::as_str) {
            let path = PathBuf::from(filename);
            if matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("rlib" | "rmeta")
            ) {
                artifacts.insert(path);
            }
        }
    }
    Ok(artifacts.into_iter().collect())
}

fn descriptor_build_command(
    manifest: &Path,
    packages: &BTreeSet<String>,
    host_package: Option<&str>,
) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .args([
            "build",
            "--locked",
            "--message-format=json-render-diagnostics",
        ])
        .arg("--manifest-path")
        .arg(manifest);
    for variable in CARGO_PACKAGE_BUILD_ENVIRONMENT {
        command.env_remove(variable);
    }
    if let Some(host_package) = host_package {
        command.arg("--package").arg(host_package);
    } else {
        for package in packages {
            command.arg("--package").arg(package);
        }
    }
    command
}

fn cargo_artifact_matches(message: &Value, package: &str) -> bool {
    let package_id = message
        .get("package_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    package_id.contains(&format!("#{package}@"))
        || package_id
            .strip_prefix(package)
            .is_some_and(|suffix| suffix.starts_with(' '))
        || message
            .pointer("/target/name")
            .and_then(Value::as_str)
            .is_some_and(|target| target == package.replace('-', "_"))
}

fn catalog_from_artifacts(paths: &[PathBuf]) -> Result<ModuleCatalog, AuthoringError> {
    let mut descriptors = BTreeMap::<(String, String), (PathBuf, ModuleDescriptor)>::new();
    for path in paths {
        let bytes = fs::read(path).map_err(|source| AuthoringError::Io {
            path: path.clone(),
            source,
        })?;
        for descriptor in extract_descriptors(&bytes, path)? {
            let key = (
                descriptor.package_id().to_owned(),
                descriptor.entrypoint().to_owned(),
            );
            if let Some((existing_path, existing_descriptor)) = descriptors.get(&key) {
                if existing_descriptor == &descriptor {
                    continue;
                }
                return Err(AuthoringError::ModuleDescriptor {
                    path: path.clone(),
                    detail: format!(
                        "conflicting descriptor `{}#{}` also found in {}",
                        key.0,
                        key.1,
                        existing_path.display()
                    ),
                });
            }
            descriptors.insert(key, (path.clone(), descriptor));
        }
    }
    ModuleCatalog::new(descriptors.into_values().map(|(_, descriptor)| descriptor)).map_err(
        |error| AuthoringError::ModuleDescriptor {
            path: PathBuf::from("<cargo-artifacts>"),
            detail: error.to_string(),
        },
    )
}

fn extract_descriptors(bytes: &[u8], path: &Path) -> Result<Vec<ModuleDescriptor>, AuthoringError> {
    let mut descriptors = Vec::new();
    let mut remaining = bytes;
    while let Some(start) = find_bytes(remaining, DESCRIPTOR_START) {
        let body = &remaining[start + DESCRIPTOR_START.len()..];
        let Some(end) = find_bytes(body, DESCRIPTOR_END) else {
            return Err(AuthoringError::ModuleDescriptor {
                path: path.to_owned(),
                detail: "descriptor artifact has no closing marker".to_owned(),
            });
        };
        let descriptor =
            serde_json::from_slice::<ModuleDescriptor>(&body[..end]).map_err(|source| {
                AuthoringError::Json {
                    path: path.to_owned(),
                    source,
                }
            })?;
        descriptors.push(descriptor);
        remaining = &body[end + DESCRIPTOR_END.len()..];
    }
    Ok(descriptors)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_envelopes_are_extracted_without_executing_code() {
        let json = r#"{"package_id":"example.tool","package_revision":"1.0.0","entrypoint":"default","configuration_schema":{"type":"object","required":["name"],"properties":{"name":{"type":"string"}},"additionalProperties":false},"provided_capabilities":[],"required_capabilities":[],"execution_class":"lenso.native-rust@1","restart_policy":{"mode":"never","max_attempts":0,"window":{"secs":0,"nanos":0},"backoff":{"secs":0,"nanos":0},"stability":{"secs":0,"nanos":0},"jitter":{"secs":0,"nanos":0}},"criticality":"non_critical"}"#;
        let artifact = [
            b"binary-prefix".as_slice(),
            DESCRIPTOR_START,
            json.as_bytes(),
            DESCRIPTOR_END,
            b"binary-suffix".as_slice(),
        ]
        .concat();

        let descriptors = extract_descriptors(&artifact, Path::new("fixture.rlib")).unwrap();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].package_id(), "example.tool");
        assert_eq!(descriptors[0].package_revision(), "1.0.0");
        assert_eq!(
            descriptors[0].configuration_schema().unwrap()["required"],
            serde_json::json!(["name"])
        );
    }

    #[test]
    fn cargo_artifacts_are_tied_to_the_selected_package() {
        let modern = serde_json::json!({
            "package_id": "registry+https://example#example-tools@1.2.3",
            "target": {"name": "example_tools"}
        });
        let path_package = serde_json::json!({
            "package_id": "path+file:///app/crates/example-tools#1.2.3",
            "target": {"name": "example_tools"}
        });
        let dependency = serde_json::json!({
            "package_id": "path+file:///app/dependency#1.2.3",
            "target": {"name": "dependency"}
        });
        assert!(cargo_artifact_matches(&modern, "example-tools"));
        assert!(cargo_artifact_matches(&path_package, "example-tools"));
        assert!(!cargo_artifact_matches(&dependency, "example-tools"));
    }

    #[test]
    fn host_package_is_optional_and_round_trips() {
        let definition = serde_json::json!({
            "schema_version": 1,
            "manifest": "Cargo.toml",
            "host_package": "example-host",
            "extensions": {
                "example.product": {
                    "schema_version": 1,
                    "enabled": ["alpha@1", "beta@1"]
                }
            },
            "packages": {"example.tool": "example-tool"},
            "app": {
                "name": "example",
                "modules": [{
                    "key": "tool",
                    "package": "example.tool",
                    "configuration": {}
                }],
                "decisions": [],
                "execution_lanes": [{"id": "main"}]
            }
        });
        let parsed: CargoAppDefinition = serde_json::from_value(definition).unwrap();
        assert_eq!(parsed.host_package(), Some("example-host"));
        assert_eq!(parsed.extensions().len(), 1);
        assert_eq!(
            parsed.extension("example.product").unwrap()["enabled"],
            serde_json::json!(["alpha@1", "beta@1"])
        );
        let serialized = serde_json::to_value(parsed).unwrap();
        assert_eq!(serialized["host_package"], "example-host");
        assert_eq!(
            serialized["extensions"]["example.product"]["schema_version"],
            1
        );
    }

    #[test]
    fn host_package_builds_the_linked_graph_instead_of_external_packages_directly() {
        let packages = BTreeSet::from([
            "example-local-module".to_owned(),
            "example-external-module".to_owned(),
        ]);
        let command =
            descriptor_build_command(Path::new("Cargo.toml"), &packages, Some("example-host"));
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--package", "example-host"])
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "example-external-module")
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "example-local-module")
        );
    }

    #[test]
    fn descriptor_build_does_not_inherit_package_build_environment() {
        let packages = BTreeSet::from(["example-module".to_owned()]);
        let command = descriptor_build_command(Path::new("Cargo.toml"), &packages, None);
        let removed = command
            .get_envs()
            .filter_map(|(name, value)| value.is_none().then_some(name.to_string_lossy()))
            .collect::<BTreeSet<_>>();

        assert!(removed.contains("CARGO_MANIFEST_DIR"));
        assert!(removed.contains("CARGO_PKG_NAME"));
        assert!(removed.contains("OUT_DIR"));
    }
}
