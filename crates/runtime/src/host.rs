use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use extism::host_fn;
use lenso_cli::{Context, Error};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Host-recorded inputs; Wasm cannot provide or overwrite this evidence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// Exact project-relative reads, including observed absence.
    pub reads: BTreeMap<PathBuf, Option<String>>,
    /// HTTPS response digests keyed by their requested URL.
    pub sources: BTreeMap<String, String>,
    /// Executed module identities, versions, exports, and content digests.
    pub modules: Vec<serde_json::Value>,
    /// Host-owned immutable repository inputs, checked before application.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resources: BTreeMap<String, crate::resources::ResourcePin>,
}

pub(crate) const MAX_RESOURCE_BYTES: usize = 1024 * 1024;

pub(crate) struct Host {
    pub(crate) project: PathBuf,
    pub(crate) reads: Vec<PathBuf>,
    pub(crate) read_roots: Vec<PathBuf>,
    pub(crate) env: Vec<String>,
    pub(crate) credentials: BTreeMap<String, crate::http::Credential>,
    pub(crate) discover: bool,
    http: Vec<String>,
    pub(crate) client: reqwest::blocking::Client,
    requests: usize,
    pub evidence: Evidence,
    pub(crate) owner: String,
    pub(crate) source_grants: Vec<String>,
    pub(crate) source_options: crate::LoadOptions,
    pub(crate) source_handles: BTreeMap<String, crate::resources::ResourcePin>,
}

impl Host {
    pub fn new(ctx: &Context) -> Result<Self, Error> {
        let reads = ctx
            .capabilities
            .get("host.reads")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let http = ctx
            .capabilities
            .get("host.http")
            .cloned()
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .user_agent("lenso-cli/0.1")
            .build()
            .map_err(|e| Error::Operation(e.to_string()))?;
        Ok(Self {
            project: ctx.project.clone(),
            reads,
            read_roots: ctx
                .capabilities
                .get("host.read_roots")
                .cloned()
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default(),
            env: ctx
                .capabilities
                .get("host.env")
                .cloned()
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default(),
            credentials: BTreeMap::new(),
            discover: ctx
                .capabilities
                .get("host.discover")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            http,
            client,
            requests: 0,
            evidence: Evidence::default(),
            owner: ctx
                .capabilities
                .get("host.credential_owner")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .into(),
            source_grants: ctx
                .capabilities
                .get("host.sources")
                .cloned()
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default(),
            source_options: Default::default(),
            source_handles: Default::default(),
        })
    }

    pub fn read(&mut self, relative: &Path) -> Result<Option<String>, Error> {
        if !path_allowed(relative, &self.reads, &self.read_roots) {
            return Err(Error::Permission(format!(
                "file read {}",
                relative.display()
            )));
        }
        let path = project_path(&self.project, relative)?;
        let content = match std::fs::File::open(path) {
            Ok(file) => {
                let mut bytes = Vec::new();
                file.take(MAX_RESOURCE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > MAX_RESOURCE_BYTES {
                    return Err(Error::Invalid("file read exceeds 1 MiB".into()));
                }
                Some(
                    String::from_utf8(bytes)
                        .map_err(|_| Error::Invalid("file is not UTF-8".into()))?,
                )
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let digest = content.as_deref().map(sha256);
        if let Some(previous) = self
            .evidence
            .reads
            .insert(relative.to_owned(), digest.clone())
            && previous != digest
        {
            return Err(Error::Operation(format!(
                "file changed during Wasm call: {}",
                relative.display()
            )));
        }
        Ok(content)
    }

    pub fn get(&mut self, url: &str) -> Result<String, Error> {
        self.request(url, self.client.get(url), &public_url(url))
    }

    pub(crate) fn request(
        &mut self,
        permission_url: &str,
        request: reqwest::blocking::RequestBuilder,
        evidence_key: &str,
    ) -> Result<String, Error> {
        self.request_bounded(permission_url, request, evidence_key, MAX_RESOURCE_BYTES)
    }

    pub(crate) fn request_bounded(
        &mut self,
        permission_url: &str,
        request: reqwest::blocking::RequestBuilder,
        evidence_key: &str,
        max: usize,
    ) -> Result<String, Error> {
        let safe_url = public_url(permission_url);
        if !http_allowed(permission_url, &self.http) {
            return Err(Error::Permission(format!("HTTP GET {safe_url}")));
        }
        self.requests += 1;
        if self.requests > 256 {
            return Err(Error::Invalid("Wasm HTTP request limit exceeded".into()));
        }
        let response = request
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(|e| Error::Operation(format!("HTTP GET {safe_url}: {}", e.without_url())))?;
        if response.status().is_redirection() {
            return Err(Error::Permission("HTTP redirects are not followed".into()));
        }
        let mut bytes = Vec::new();
        response.take(max as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > max {
            return Err(Error::Invalid(
                "HTTP response exceeds the resource size limit".into(),
            ));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| Error::Invalid("HTTP response is not UTF-8".into()))?;
        let digest = sha256(&content);
        let evidence_key = evidence_key.to_owned();
        if let Some(previous) = self
            .evidence
            .sources
            .insert(evidence_key.clone(), digest.clone())
            && previous != digest
        {
            return Err(Error::Operation(format!(
                "HTTP source changed during Wasm call: {evidence_key}"
            )));
        }
        Ok(content)
    }
}

pub(crate) fn sha256(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

/// Resolve a project-relative path, rejecting traversal, symlinks, and Git metadata.
///
/// # Errors
/// Rejects unsafe paths or inaccessible filesystem entries.
pub fn project_path(project: &Path, relative: &Path) -> Result<PathBuf, Error> {
    if relative.as_os_str().is_empty() {
        return Err(Error::Invalid("empty project path".into()));
    }
    let mut path = project.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(Error::Permission(
                "project paths must be relative without traversal".into(),
            ));
        };
        if name == ".git" {
            return Err(Error::Permission("Git metadata is inaccessible".into()));
        }
        path.push(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::Permission(format!(
                    "symlink in project path: {}",
                    relative.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

pub(crate) fn http_allowed(url: &str, grants: &[String]) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    if !transport_allowed(&url)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    grants
        .iter()
        .filter_map(|grant| reqwest::Url::parse(grant).ok())
        .any(|grant| {
            transport_allowed(&grant)
                && url.origin() == grant.origin()
                && grant.path().ends_with('/')
                && url.path().starts_with(grant.path())
                && grant.query().is_none()
        })
}

/// Check exact paths and descendant-directory grants without permitting traversal.
#[must_use]
pub fn path_allowed(path: &Path, exact: &[PathBuf], roots: &[PathBuf]) -> bool {
    let normal = |path: &Path| {
        !path.as_os_str().is_empty() && path.components().all(|part| matches!(part, Component::Normal(name) if name != ".git" && name != ".lenso" && name != "node_modules"))
    };
    normal(path)
        && (exact.iter().any(|grant| grant == path)
            || roots
                .iter()
                .any(|root| normal(root) && path != root && path.starts_with(root)))
}

fn transport_allowed(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        || (url.scheme() == "http"
            && url
                .host_str()
                .and_then(|host| {
                    host.trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .ok()
                })
                .is_some_and(|ip| ip.is_loopback()))
}

pub(crate) fn public_url(value: &str) -> String {
    match reqwest::Url::parse(value) {
        Ok(mut url) => {
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => "HTTP request".into(),
    }
}

host_fn!(pub(crate) source_resolve(host: Host; request: String) -> String {
    let host=host.get()?;
    Ok(host.lock().map_err(|_|extism::Error::msg("host state poisoned"))?.source_resolve(&request)?)
});
host_fn!(pub(crate) source_read(host: Host; request: String) -> String {
    let host=host.get()?;
    Ok(host.lock().map_err(|_|extism::Error::msg("host state poisoned"))?.source_read(&request)?)
});

host_fn!(pub(crate) http_request(host: Host; request: String) -> String {
    let host = host.get()?;
    let result = host.lock().map_err(|_| extism::Error::msg("host state poisoned"))?.http_request(&request)?;
    Ok(result)
});

host_fn!(pub(crate) list_files(host: Host; request: String) -> String {
    let host = host.get()?;
    let result = host.lock().map_err(|_| extism::Error::msg("host state poisoned"))?.list_files(&request)?;
    Ok(serde_json::to_string(&result)?)
});

host_fn!(pub(crate) read_file(host: Host; path: String) -> String {
    let host = host.get()?;
    let result = host.lock().map_err(|_| extism::Error::msg("host state poisoned"))?.read(Path::new(&path))?;
    Ok(serde_json::to_string(&result)?)
});

host_fn!(pub(crate) http_get(host: Host; url: String) -> String {
    let host = host.get()?;
    let result = host.lock().map_err(|_| extism::Error::msg("host state poisoned"))?.get(&url)?;
    Ok(result)
});

#[cfg(test)]
mod tests {
    use super::Host;
    use super::http_allowed;
    use lenso_cli::{Context, serde_json::json};
    use std::collections::BTreeMap;

    fn context(project: &std::path::Path) -> Context {
        Context {
            events: Default::default(),
            abi_version: 1,
            project: project.to_owned(),
            input: json!({}),
            answers: BTreeMap::new(),
            steps: BTreeMap::new(),
            hooks: BTreeMap::new(),
            capabilities: BTreeMap::from([(
                "host.reads".into(),
                json!(["config.json", "../escape"]),
            )]),
        }
    }

    #[test]
    fn file_host_checks_grants_paths_and_observed_absence() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let mut host = Host::new(&context(root.path()))?;
        assert!(host.read(std::path::Path::new("secret.txt")).is_err());
        assert!(host.read(std::path::Path::new("../escape")).is_err());
        assert_eq!(host.read(std::path::Path::new("config.json"))?, None);
        assert_eq!(
            host.evidence.reads[std::path::Path::new("config.json")],
            None
        );
        std::fs::write(root.path().join("config.json"), "changed during invocation")?;
        assert!(host.read(std::path::Path::new("config.json")).is_err());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn file_host_rejects_symlinks_even_when_the_path_is_granted()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let outside = tempfile::NamedTempFile::new()?;
        std::os::unix::fs::symlink(outside.path(), root.path().join("config.json"))?;
        let mut host = Host::new(&context(root.path()))?;
        assert!(host.read(std::path::Path::new("config.json")).is_err());
        Ok(())
    }
    #[test]
    fn url_grants_check_origin_and_path_boundary() {
        let grants = vec!["https://assets.example.test/r/".into()];
        assert!(http_allowed(
            "https://assets.example.test/r/index.json",
            &grants
        ));
        for url in [
            "http://assets.example.test/r/index.json",
            "https://assets.example.test.evil/r/index.json",
            "https://assets.example.test/r/../secret",
            "https://assets.example.test/registry.json",
            "https://user@assets.example.test/r/index.json",
            "https://assets.example.test:444/r/index.json",
        ] {
            assert!(!http_allowed(url, &grants), "must reject {url}");
        }
    }
    #[test]
    fn directory_grants_respect_component_boundaries_and_reserved_paths() {
        let roots = vec![std::path::PathBuf::from("src/components")];
        assert!(super::path_allowed(
            std::path::Path::new("src/components/ui/card.tsx"),
            &[],
            &roots
        ));
        for path in [
            "src/components-old/card.tsx",
            "src/components/../secret",
            "src/components/.git/config",
            "src/components/node_modules/pkg/file",
            "src/components",
        ] {
            assert!(
                !super::path_allowed(std::path::Path::new(path), &[], &roots),
                "must reject {path}"
            );
        }
    }
}
