use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub(crate) const APP_COMPOSITION_FILE: &str = "lenso.app.json";
pub(crate) const APP_COMPOSITION_PROTOCOL: &str = "lenso.app-composition.v1";
const PRODUCT_CONTRACT_PROTOCOL: &str = "lenso.module-product-contract.v1";

#[derive(Debug, Clone)]
pub(crate) struct CompositionAuthoring {
    pub(crate) app_id: String,
    pub(crate) modules: Vec<CompositionModuleInput>,
    pub(crate) provenance: CompositionProvenance,
}

#[derive(Debug, Clone)]
pub(crate) struct CompositionModuleInput {
    pub(crate) module_id: String,
    pub(crate) version: String,
    pub(crate) owner: String,
    pub(crate) business_contributions: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) implementation: ImplementationInput,
}

#[derive(Debug, Clone)]
pub(crate) enum ImplementationInput {
    Linked,
    Service { service_reference: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppComposition {
    pub(crate) protocol: String,
    pub(crate) app_id: String,
    pub(crate) revision: u64,
    pub(crate) content_digest: String,
    pub(crate) modules: Vec<CompositionModule>,
    pub(crate) provenance: CompositionProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompositionModule {
    pub(crate) module_id: String,
    pub(crate) release: ImmutableModuleRelease,
    pub(crate) implementation: ImplementationBinding,
    pub(crate) dependencies: Vec<ResolvedDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImmutableModuleRelease {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) owner: String,
    pub(crate) content_digest: String,
    pub(crate) business_contributions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum ImplementationBinding {
    Linked,
    Service {
        #[serde(rename = "serviceReference")]
        service_reference: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolvedDependency {
    pub(crate) requirement: String,
    pub(crate) module_id: String,
    pub(crate) version: String,
    pub(crate) content_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CompositionProvenance {
    pub(crate) blueprint: Option<String>,
    pub(crate) addons: Vec<String>,
    pub(crate) capability_packs: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProductContractFingerprint<'a> {
    protocol: &'static str,
    id: &'a str,
    version: &'a str,
    owner: &'a str,
    business_contributions: &'a [String],
    dependencies: &'a [String],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CompositionDigestPayload<'a> {
    protocol: &'a str,
    app_id: &'a str,
    revision: u64,
    modules: &'a [CompositionModule],
    provenance: &'a CompositionProvenance,
}

#[derive(Debug)]
struct CompositionLock {
    path: PathBuf,
    _file: File,
}

impl Drop for CompositionLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn build_composition(authoring: CompositionAuthoring) -> Result<AppComposition> {
    build_composition_at_revision(authoring, 1)
}

pub(crate) fn create(path: &Path, authoring: CompositionAuthoring) -> Result<AppComposition> {
    let _lock = acquire_lock(path)?;
    if path.exists() {
        bail!("App Composition already exists: {}", path.display());
    }
    let composition = build_composition(authoring)?;
    write_atomic(path, &composition)?;
    Ok(composition)
}

pub(crate) fn read(path: &Path) -> Result<AppComposition> {
    let source =
        fs::read(path).with_context(|| format!("read App Composition {}", path.display()))?;
    let composition: AppComposition = serde_json::from_slice(&source)
        .with_context(|| format!("parse App Composition {}", path.display()))?;
    validate_composition(&composition)?;
    Ok(composition)
}

pub(crate) fn replace(
    path: &Path,
    observed_revision: u64,
    authoring: CompositionAuthoring,
) -> Result<AppComposition> {
    let _lock = acquire_lock(path)?;
    let current = read(path)?;
    if current.revision != observed_revision {
        bail!(
            "App Composition revision conflict: observed revision {observed_revision}, current revision {}",
            current.revision
        );
    }
    let next = build_composition_at_revision(authoring, current.revision + 1)?;
    write_atomic(path, &next)?;
    Ok(next)
}

pub(crate) fn authoring_from_composition(composition: &AppComposition) -> CompositionAuthoring {
    CompositionAuthoring {
        app_id: composition.app_id.clone(),
        modules: composition
            .modules
            .iter()
            .map(|module| CompositionModuleInput {
                module_id: module.module_id.clone(),
                version: module.release.version.clone(),
                owner: module.release.owner.clone(),
                business_contributions: module.release.business_contributions.clone(),
                dependencies: module
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.requirement.clone())
                    .collect(),
                implementation: match &module.implementation {
                    ImplementationBinding::Linked => ImplementationInput::Linked,
                    ImplementationBinding::Service { service_reference } => {
                        ImplementationInput::Service {
                            service_reference: service_reference.clone(),
                        }
                    }
                },
            })
            .collect(),
        provenance: composition.provenance.clone(),
    }
}

pub(crate) fn build_composition_at_revision(
    authoring: CompositionAuthoring,
    revision: u64,
) -> Result<AppComposition> {
    if authoring.app_id.is_empty() {
        bail!("App Composition appId must not be empty");
    }
    if !is_safe_identity(&authoring.app_id) {
        bail!(
            "App Composition appId `{}` is not a safe identity",
            authoring.app_id
        );
    }
    if revision == 0 {
        bail!("App Composition revision must be positive");
    }

    let mut inputs = authoring.modules;
    inputs.sort_by(|left, right| left.module_id.cmp(&right.module_id));
    let mut modules = Vec::with_capacity(inputs.len());
    for input in &inputs {
        validate_product_contract(input)?;
        if modules
            .iter()
            .any(|module: &CompositionModule| module.module_id == input.module_id)
        {
            bail!("duplicate Module Product Contract `{}`", input.module_id);
        }
        let implementation = implementation_binding(&input.implementation)?;
        let release = ImmutableModuleRelease {
            content_digest: product_contract_digest(input)?,
            business_contributions: input.business_contributions.clone(),
            id: input.module_id.clone(),
            owner: input.owner.clone(),
            version: input.version.clone(),
        };
        modules.push(CompositionModule {
            dependencies: Vec::new(),
            implementation,
            module_id: input.module_id.clone(),
            release,
        });
    }

    for index in 0..inputs.len() {
        let module_id = modules[index].module_id.clone();
        let resolved = inputs[index]
            .dependencies
            .iter()
            .map(|requirement| {
                let selected = modules
                .iter()
                .find(|candidate| candidate.module_id == *requirement)
                .or_else(|| {
                    modules.iter().find(|candidate| {
                        candidate
                            .release
                            .business_contributions
                            .iter()
                            .any(|contribution| contribution == requirement)
                    })
                })
                .with_context(|| {
                    format!(
                        "Module `{module_id}` dependency `{requirement}` has no exact selected release"
                    )
                })?;
            if selected.module_id == module_id {
                bail!(
                    "Module `{module_id}` cannot resolve dependency `{requirement}` to itself"
                );
            }
            Ok(ResolvedDependency {
                content_digest: selected.release.content_digest.clone(),
                module_id: selected.module_id.clone(),
                requirement: requirement.clone(),
                version: selected.release.version.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
        modules[index].dependencies = resolved;
    }

    let composition = AppComposition {
        app_id: authoring.app_id,
        content_digest: String::new(),
        modules,
        protocol: APP_COMPOSITION_PROTOCOL.to_owned(),
        provenance: normalize_provenance(authoring.provenance),
        revision,
    };
    let content_digest = composition_digest(&composition)?;
    Ok(AppComposition {
        content_digest,
        ..composition
    })
}

fn validate_product_contract(input: &CompositionModuleInput) -> Result<()> {
    for (label, value) in [
        ("module id", &input.module_id),
        ("version", &input.version),
        ("owner", &input.owner),
    ] {
        if value.trim().is_empty() {
            bail!("Module Product Contract {label} must not be empty");
        }
    }
    if input.business_contributions.is_empty()
        || input
            .business_contributions
            .iter()
            .any(|contribution| contribution.trim().is_empty())
    {
        bail!(
            "Module Product Contract `{}` requires at least one business contribution",
            input.module_id
        );
    }
    if input
        .dependencies
        .iter()
        .any(|dependency| dependency.trim().is_empty())
    {
        bail!(
            "Module Product Contract `{}` contains an empty dependency requirement",
            input.module_id
        );
    }
    Ok(())
}

fn implementation_binding(input: &ImplementationInput) -> Result<ImplementationBinding> {
    match input {
        ImplementationInput::Linked => Ok(ImplementationBinding::Linked),
        ImplementationInput::Service { service_reference } => {
            validate_service_reference(service_reference)?;
            Ok(ImplementationBinding::Service {
                service_reference: service_reference.clone(),
            })
        }
    }
}

fn validate_service_reference(reference: &str) -> Result<()> {
    let Some(value) = reference.strip_prefix("service:") else {
        bail!("Service-backed implementation must use a stable `service:` reference");
    };
    let segments = value.split('/').collect::<Vec<_>>();
    if value.is_empty()
        || segments.len() != 2
        || reference.contains("://")
        || reference.chars().any(char::is_whitespace)
        || reference.contains('@')
        || value
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        || value.chars().any(|character| {
            !(character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | '/'))
        })
    {
        bail!("Service Reference `{reference}` is not stable and deployment-neutral");
    }
    Ok(())
}

fn is_safe_identity(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn product_contract_digest(input: &CompositionModuleInput) -> Result<String> {
    let bytes = serde_json::to_vec(&ProductContractFingerprint {
        business_contributions: &input.business_contributions,
        dependencies: &input.dependencies,
        id: &input.module_id,
        owner: &input.owner,
        protocol: PRODUCT_CONTRACT_PROTOCOL,
        version: &input.version,
    })?;
    Ok(sha256_digest(&bytes))
}

fn validate_composition(composition: &AppComposition) -> Result<()> {
    if composition.protocol != APP_COMPOSITION_PROTOCOL {
        bail!(
            "unsupported App Composition protocol `{}`",
            composition.protocol
        );
    }
    if composition.revision == 0 {
        bail!("App Composition revision must be positive");
    }
    if composition.content_digest != composition_digest(composition)? {
        bail!("App Composition content digest does not match its content");
    }
    if !is_safe_identity(&composition.app_id) {
        bail!(
            "App Composition appId `{}` is not a safe identity",
            composition.app_id
        );
    }
    let mut module_ids = std::collections::BTreeSet::new();
    for module in &composition.modules {
        if module.module_id != module.release.id {
            bail!(
                "Module `{}` release identity does not match its selection",
                module.module_id
            );
        }
        if module.release.business_contributions.is_empty() {
            bail!(
                "Module Product Contract `{}` has no business contribution",
                module.module_id
            );
        }
        if !module_ids.insert(module.module_id.as_str()) {
            bail!("duplicate Module selection `{}`", module.module_id);
        }
        if let ImplementationBinding::Service { service_reference } = &module.implementation {
            validate_service_reference(service_reference)?;
        }
        let mut requirements = std::collections::BTreeSet::new();
        for dependency in &module.dependencies {
            if !requirements.insert(dependency.requirement.as_str()) {
                bail!(
                    "Module `{}` resolves dependency `{}` more than once",
                    module.module_id,
                    dependency.requirement
                );
            }
            let selected = composition
                .modules
                .iter()
                .find(|candidate| candidate.module_id == dependency.module_id)
                .with_context(|| {
                    format!(
                        "Module `{}` dependency `{}` selects unknown Module `{}`",
                        module.module_id, dependency.requirement, dependency.module_id
                    )
                })?;
            if selected.module_id == module.module_id {
                bail!(
                    "Module `{}` cannot resolve dependency `{}` to itself",
                    module.module_id,
                    dependency.requirement
                );
            }
            if selected.release.version != dependency.version
                || selected.release.content_digest != dependency.content_digest
            {
                bail!(
                    "Module `{}` dependency `{}` does not pin the selected release digest",
                    module.module_id,
                    dependency.requirement
                );
            }
        }
    }
    let rebuilt = build_composition_at_revision(
        authoring_from_composition(composition),
        composition.revision,
    )?;
    if rebuilt.modules != composition.modules || rebuilt.provenance != composition.provenance {
        bail!("App Composition contains an invalid release or dependency selection");
    }
    Ok(())
}

fn composition_digest(composition: &AppComposition) -> Result<String> {
    let payload = serde_json::to_vec(&CompositionDigestPayload {
        app_id: &composition.app_id,
        modules: &composition.modules,
        provenance: &composition.provenance,
        protocol: &composition.protocol,
        revision: composition.revision,
    })?;
    Ok(sha256_digest(&payload))
}

fn normalize_provenance(mut provenance: CompositionProvenance) -> CompositionProvenance {
    provenance.addons.sort();
    provenance.addons.dedup();
    provenance.capability_packs.sort();
    provenance.capability_packs.dedup();
    provenance
}

fn acquire_lock(path: &Path) -> Result<CompositionLock> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create App Composition directory {}", parent.display()))?;
    let lock_path = parent.join(format!(
        ".{}.lock",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(APP_COMPOSITION_FILE)
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| {
            format!(
                "acquire App Composition write lock {}; another composition update may be active",
                lock_path.display()
            )
        })?;
    writeln!(file, "pid={}", std::process::id())?;
    Ok(CompositionLock {
        path: lock_path,
        _file: file,
    })
}

fn write_atomic(path: &Path, composition: &AppComposition) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(APP_COMPOSITION_FILE),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("create temporary App Composition {}", temp.display()))?;
        let source = serde_json::to_vec_pretty(composition)?;
        file.write_all(&source)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temp, path)
            .with_context(|| format!("atomically replace App Composition {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn sha256_digest(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    let hex = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exact_composition_records_release_bindings_and_resolved_dependencies() {
        let composition = build_composition(CompositionAuthoring {
            app_id: "support-desk".to_owned(),
            modules: vec![
                CompositionModuleInput {
                    module_id: "support-ticket".to_owned(),
                    version: "0.1.0".to_owned(),
                    owner: "example/support".to_owned(),
                    business_contributions: vec!["support.tickets.read".to_owned()],
                    dependencies: vec!["identity".to_owned()],
                    implementation: ImplementationInput::Service {
                        service_reference: "service:support-desk/support-ticket".to_owned(),
                    },
                },
                CompositionModuleInput {
                    module_id: "identity".to_owned(),
                    version: "0.1.0".to_owned(),
                    owner: "lenso/auth".to_owned(),
                    business_contributions: vec!["identity".to_owned()],
                    dependencies: Vec::new(),
                    implementation: ImplementationInput::Linked,
                },
            ],
            provenance: CompositionProvenance {
                blueprint: Some("support-desk".to_owned()),
                addons: Vec::new(),
                capability_packs: Vec::new(),
            },
        })
        .unwrap();

        assert_eq!(composition.protocol, APP_COMPOSITION_PROTOCOL);
        assert_eq!(composition.revision, 1);
        assert!(composition.content_digest.starts_with("sha256:"));
        assert_eq!(composition.modules[0].module_id, "identity");
        assert_eq!(composition.modules[1].module_id, "support-ticket");
        assert_eq!(
            composition.modules[1].implementation,
            ImplementationBinding::Service {
                service_reference: "service:support-desk/support-ticket".to_owned()
            }
        );
        assert_eq!(composition.modules[1].dependencies[0].module_id, "identity");
        assert_eq!(
            composition.modules[1].dependencies[0].content_digest,
            composition.modules[0].release.content_digest
        );
    }

    #[test]
    fn product_contract_requires_a_business_contribution_and_stable_service_reference() {
        let error = build_composition(CompositionAuthoring {
            app_id: "support-desk".to_owned(),
            modules: vec![CompositionModuleInput {
                module_id: "support-ticket".to_owned(),
                version: "0.1.0".to_owned(),
                owner: "example/support".to_owned(),
                business_contributions: Vec::new(),
                dependencies: Vec::new(),
                implementation: ImplementationInput::Service {
                    service_reference: "https://127.0.0.1:4110".to_owned(),
                },
            }],
            provenance: CompositionProvenance {
                blueprint: None,
                addons: Vec::new(),
                capability_packs: Vec::new(),
            },
        })
        .expect_err("invalid product contract must be rejected");

        let message = error.to_string();
        assert!(message.contains("business contribution"));
    }

    #[test]
    fn composition_update_rejects_a_stale_observed_revision_without_mutation() {
        let root = test_root("composition-revision");
        let path = root.join(APP_COMPOSITION_FILE);
        let authoring = basic_authoring();
        create(&path, authoring.clone()).unwrap();

        let error = replace(&path, 0, authoring).expect_err("stale revision must fail");
        assert!(error.to_string().contains("observed revision 0"));
        assert_eq!(read(&path).unwrap().revision, 1);

        fs::remove_dir_all(root).unwrap();
    }

    fn basic_authoring() -> CompositionAuthoring {
        CompositionAuthoring {
            app_id: "support-desk".to_owned(),
            modules: vec![CompositionModuleInput {
                module_id: "support-ticket".to_owned(),
                version: "0.1.0".to_owned(),
                owner: "example/support".to_owned(),
                business_contributions: vec!["support.tickets.read".to_owned()],
                dependencies: Vec::new(),
                implementation: ImplementationInput::Linked,
            }],
            provenance: CompositionProvenance {
                blueprint: Some("support-desk".to_owned()),
                addons: Vec::new(),
                capability_packs: Vec::new(),
            },
        }
    }

    fn test_root(name: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("lenso-{name}-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
