//! Host-owned repository source pins. Catalog interpretation remains in extensions.
use crate::{
    LoadMode, LoadOptions,
    cache::{check_digest, digest},
    github::{Github, Repo},
    host::Host,
    project::{read_lock, save_resource_lock},
};
use lenso_cli::Error;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

/// A resolved repository snapshot and the files observed from that snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourcePin {
    /// Owning extension ID.
    pub owner: String,
    /// Explicit repository locator.
    pub source: String,
    /// Requested branch, tag or commit.
    pub reference: String,
    /// Configured API authority.
    pub api: String,
    /// Full resolved commit SHA.
    pub commit: String,
    /// Whether this source was resolved with an API credential.
    pub authenticated: bool,
    /// SHA-256 of files observed at the pinned commit.
    pub files: BTreeMap<String, String>,
}
impl ResourcePin {
    pub(crate) fn key(&self) -> String {
        digest(
            json!([self.owner, self.source, self.reference])
                .to_string()
                .as_bytes(),
        )
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        repository(&self.source)?;
        crate::github::commit(&self.commit)?;
        if self.owner.is_empty()
            || self.reference.is_empty()
            || self.reference.len() > 256
            || self.files.len() > 128
        {
            return Err(Error::Invalid("invalid resource pin".into()));
        }
        for (path, sha) in &self.files {
            crate::github::relative(path)?;
            check_digest(sha)?;
        }
        Ok(())
    }
}
fn repository(source: &str) -> Result<Repo, Error> {
    Repo::parse(source.strip_prefix("github-repo:").ok_or_else(|| {
        Error::Invalid("repository sources require github-repo:owner/repo".into())
    })?)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolve {
    source: String,
    #[serde(default = "head")]
    reference: String,
    #[serde(default)]
    refresh: bool,
}
fn head() -> String {
    "HEAD".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Read {
    handle: String,
    path: String,
}

impl Host {
    pub(crate) fn source_resolve(&mut self, request: &str) -> Result<String, Error> {
        let request: Resolve = serde_json::from_str(request)?;
        if self.owner.is_empty() {
            return Err(Error::Permission(
                "source operations require a workflow owner".into(),
            ));
        }
        let repo = repository(&request.source)?;
        if !self
            .source_grants
            .contains(&format!("github:{}", repo.name()))
        {
            return Err(lenso_cli::Diagnostic::missing_capability("host.sources").into());
        }
        if request.refresh && self.source_options.locked {
            return Err(Error::Invalid(
                "source update cannot be combined with --locked".into(),
            ));
        }
        let github = Github::new(&self.source_options.github)?;
        let cache = self.source_options.cache()?;
        let _guard = cache.lock(&format!("project:{}", self.project.display()))?;
        let mut lock = read_lock(&self.project)?.ok_or_else(|| {
            Error::Invalid("repository sources require a prepared lenso.lock".into())
        })?;
        let mut pin = ResourcePin {
            owner: self.owner.clone(),
            source: request.source,
            reference: request.reference,
            api: github.config.api.clone(),
            commit: String::new(),
            authenticated: github.authenticated(),
            files: BTreeMap::new(),
        };
        let key = pin.key();
        if !request.refresh
            && let Some(previous) = lock.resources.get(&key).filter(|p| p.api == pin.api)
        {
            pin = previous.clone();
        }
        if pin.commit.is_empty() {
            if self.source_options.locked {
                return Err(Error::Invalid(
                    "repository source is not pinned with --locked".into(),
                ));
            }
            let options = LoadOptions {
                mode: if request.refresh {
                    LoadMode::Update
                } else {
                    LoadMode::Execute
                },
                ..self.source_options.clone()
            };
            pin.commit = github.revision(&repo, &pin.reference, &cache, &options)?;
            if let Some(old) = lock
                .resources
                .get(&key)
                .filter(|p| p.commit == pin.commit && p.api == pin.api)
            {
                pin.files = old.files.clone();
            }
        }
        if pin.authenticated && !github.authenticated() {
            return Err(Error::Permission(
                "pinned repository source requires configured GitHub credentials".into(),
            ));
        }
        pin.validate()?;
        if lock.resources.len() >= 128 && !lock.resources.contains_key(&key) {
            return Err(Error::Invalid(
                "repository source pin limit exceeded".into(),
            ));
        }
        lock.resources.insert(key.clone(), pin.clone());
        save_resource_lock(&self.project, &lock, &self.source_options)?;
        self.evidence.resources.insert(key.clone(), pin.clone());
        self.source_handles.insert(key.clone(), pin.clone());
        Ok(json!({"handle":key,"commit":pin.commit,"repository":repo.name()}).to_string())
    }
    pub(crate) fn source_read(&mut self, request: &str) -> Result<String, Error> {
        let request: Read = serde_json::from_str(request)?;
        let mut pin = self
            .source_handles
            .get(&request.handle)
            .cloned()
            .ok_or_else(|| Error::Permission("unknown repository snapshot handle".into()))?;
        crate::github::relative(&request.path)?;
        if self.source_options.locked && !pin.files.contains_key(&request.path) {
            return Err(Error::Invalid(
                "repository file is not pinned with --locked".into(),
            ));
        }
        let github = Github::new(&self.source_options.github)?;
        let cache = self.source_options.cache()?;
        let bytes = github
            .file(
                &repository(&pin.source)?,
                &pin.commit,
                &request.path,
                &cache,
                &self.source_options,
            )?
            .ok_or_else(|| {
                lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::InvalidInput).with_details(
                    serde_json::json!({"field":"source.path","reason":"not_found_in_snapshot"}),
                )
            })?;
        let hash = digest(&bytes);
        if pin
            .files
            .get(&request.path)
            .is_some_and(|expected| expected != &hash)
        {
            return Err(Error::Permission(
                "pinned repository file digest changed".into(),
            ));
        }
        pin.files.insert(request.path.clone(), hash.clone());
        pin.validate()?;
        let _guard = cache.lock(&format!("project:{}", self.project.display()))?;
        let mut lock = read_lock(&self.project)?
            .ok_or_else(|| Error::Invalid("source lock is missing".into()))?;
        let stored = lock
            .resources
            .get_mut(&request.handle)
            .filter(|old| old.commit == pin.commit && old.api == pin.api)
            .ok_or_else(|| Error::Operation("repository pin changed during execution".into()))?;
        stored.files.extend(pin.files.clone());
        save_resource_lock(&self.project, &lock, &self.source_options)?;
        self.evidence.sources.insert(
            format!("{}@{}/{}", pin.source, pin.commit, request.path),
            hash,
        );
        self.evidence
            .resources
            .insert(request.handle.clone(), pin.clone());
        self.source_handles.insert(request.handle, pin);
        String::from_utf8(bytes)
            .map_err(|_| Error::Invalid("repository resource is not UTF-8".into()))
    }
}

/// Reject a plan when its repository source pins changed after resolution.
///
/// # Errors
/// Returns an error for missing, changed, or inconsistent source provenance.
pub fn verify_resource_pins(
    project: &Path,
    pins: &BTreeMap<String, ResourcePin>,
) -> Result<(), Error> {
    if pins.is_empty() {
        return Ok(());
    }
    let lock = read_lock(project)?
        .ok_or_else(|| Error::Operation("repository source lock is missing".into()))?;
    for (key, pin) in pins {
        pin.validate()?;
        if key != &pin.key() {
            return Err(Error::Invalid("invalid repository pin identity".into()));
        }
        let Some(current) = lock.resources.get(key) else {
            return Err(Error::Operation("repository source pin disappeared".into()));
        };
        if current.commit != pin.commit
            || current.api != pin.api
            || pin
                .files
                .iter()
                .any(|(path, sha)| current.files.get(path) != Some(sha))
        {
            return Err(Error::Operation(
                "repository source changed after review".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extension_metadata_writes_preserve_new_source_pins_and_stale_inputs_fail() {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("lenso.toml"), "[cli]\nschema_version=1\n").unwrap();
        let options = LoadOptions {
            cache_dir: Some(cache.path().into()),
            ..Default::default()
        };
        crate::prepare_project(root.path(), &options).unwrap();
        let stale = read_lock(root.path()).unwrap().unwrap();
        let mut lock = stale.clone();
        let pin = ResourcePin {
            owner: "code".into(),
            source: "github-repo:acme/resources".into(),
            reference: "main".into(),
            api: "https://api.github.com/".into(),
            commit: "1".repeat(40),
            authenticated: false,
            files: BTreeMap::new(),
        };
        let key = pin.key();
        lock.resources.insert(key.clone(), pin.clone());
        save_resource_lock(root.path(), &lock, &options).unwrap();
        crate::project::save_lock(root.path(), &stale, &options).unwrap();
        assert_eq!(
            read_lock(root.path()).unwrap().unwrap().resources[&key],
            pin
        );
        let observed = BTreeMap::from([(key.clone(), pin)]);
        verify_resource_pins(root.path(), &observed).unwrap();
        lock.resources.get_mut(&key).unwrap().commit = "2".repeat(40);
        save_resource_lock(root.path(), &lock, &options).unwrap();
        assert!(
            verify_resource_pins(root.path(), &observed)
                .unwrap_err()
                .to_string()
                .contains("changed after review")
        );
    }
}
