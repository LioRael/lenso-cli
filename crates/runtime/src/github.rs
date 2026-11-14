//! GitHub transport and immutable repository identities, with no business catalog schema.
use crate::{Cache, LoadMode, LoadOptions, cache::digest};
use lenso_cli::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

/// Trusted GitHub transport settings. Credentials never enter Wasm contexts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct GithubConfig {
    /// GitHub REST API base; custom HTTPS and numeric loopback endpoints are supported.
    pub api: String,
    /// Explicit environment variable containing an API token; optional for public data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
}
impl Default for GithubConfig {
    fn default() -> Self {
        Self {
            api: "https://api.github.com/".into(),
            token_env: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Repo {
    pub owner: String,
    pub name: String,
}
impl Repo {
    pub fn parse(value: &str) -> Result<Self, Error> {
        let mut parts = value.split('/');
        let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(Error::Invalid(
                "GitHub repository must be owner/repo".into(),
            ));
        };
        let valid = |s: &str| {
            !s.is_empty()
                && s != "."
                && s != ".."
                && s.len() <= 100
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        };
        if !valid(owner) || !valid(name) {
            return Err(Error::Invalid("invalid GitHub repository".into()));
        }
        Ok(Self {
            owner: owner.into(),
            name: name.into(),
        })
    }
    pub fn name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

pub(crate) fn relative(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > 1024
        || value.split('/').count() > 16
        || value.contains(['\\', ':', '\0'])
        || Path::new(value)
            .components()
            .any(|p| !matches!(p, std::path::Component::Normal(_)))
        || value
            .split('/')
            .any(|s| matches!(s, ".git" | ".lenso" | ".." | "."))
    {
        return Err(Error::Permission(
            "source paths must be relative without traversal or metadata access".into(),
        ));
    }
    Ok(())
}
pub(crate) fn commit(value: &str) -> Result<(), Error> {
    if value.len() != 40 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid(
            "GitHub revision must be a full commit SHA".into(),
        ));
    }
    Ok(())
}

pub(crate) struct Github {
    pub config: GithubConfig,
    token: Option<String>,
    client: reqwest::blocking::Client,
    deadline: std::time::Instant,
    requests: std::cell::Cell<usize>,
}
impl Github {
    pub fn new(config: &GithubConfig) -> Result<Self, Error> {
        let url = reqwest::Url::parse(&config.api)
            .map_err(|_| Error::Invalid("invalid GitHub API URL".into()))?;
        crate::source::Source::parse(Path::new("/"), &config.api)?;
        if !config.api.ends_with('/') || url.query().is_some() {
            return Err(Error::Invalid(
                "GitHub API base must end in / without query".into(),
            ));
        }
        let token = config
            .token_env
            .as_ref()
            .map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| {
                        Error::Permission(format!("GitHub credential {name} is not set"))
                    })
            })
            .transpose()?;
        let mut builder = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(20))
            .user_agent("lenso-cli/0.1");
        if url
            .host_str()
            .and_then(|h| h.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback())
        {
            builder = builder.no_proxy();
        }
        Ok(Self {
            config: config.clone(),
            deadline: std::time::Instant::now() + Duration::from_secs(20),
            requests: std::cell::Cell::new(0),
            token,
            client: builder
                .build()
                .map_err(|e| Error::Operation(e.to_string()))?,
        })
    }
    pub(crate) fn mutate(
        &self,
        method: reqwest::Method,
        url: reqwest::Url,
        body: Vec<u8>,
        content_type: &str,
    ) -> Result<Value, Error> {
        let api = reqwest::Url::parse(&self.config.api)
            .map_err(|_| Error::Invalid("invalid GitHub API URL".into()))?;
        let upload = api.host_str() == Some("api.github.com")
            && url.scheme() == "https"
            && url.host_str() == Some("uploads.github.com")
            && url.port_or_known_default() == Some(443);
        if url.origin() != api.origin() && !upload {
            return Err(Error::Permission(
                "GitHub mutation destination is not allowed".into(),
            ));
        }
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| Error::Permission("publishing requires a GitHub credential".into()))?;
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| Error::Invalid("invalid GitHub credential".into()))?;
        authorization.set_sensitive(true);
        let response = self
            .client
            .request(method.clone(), url)
            .timeout(Duration::from_secs(120))
            .header("Authorization", authorization)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2026-03-10")
            .header("Content-Type", content_type)
            .body(body)
            .send()
            .map_err(|e| {
                Error::Operation(format!("GitHub {method} failed: {}", e.without_url()))
            })?;
        if !response.status().is_success() {
            return Err(lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::OperationFailed)
                .with_details(serde_json::json!({"operation":"github_mutation","status":response.status().as_u16()}))
                .with_retryable(false).into());
        }
        let mut bytes = Vec::new();
        response.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err(Error::Invalid(
                "GitHub mutation response exceeds 1 MiB".into(),
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| Error::Operation("GitHub mutation returned invalid JSON".into()))
    }

    pub fn authenticated(&self) -> bool {
        self.token.is_some()
    }
    pub(crate) fn cache_key(&self, path: &str) -> String {
        format!(
            "github:{}:{}:{}",
            self.config.api,
            digest(self.token.as_deref().unwrap_or("").as_bytes()),
            path
        )
    }
    pub fn url(&self, repo: &Repo, parts: &[&str]) -> Result<reqwest::Url, Error> {
        let mut url = reqwest::Url::parse(&self.config.api)
            .map_err(|_| Error::Invalid("invalid GitHub API URL".into()))?;
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| Error::Invalid("invalid GitHub API base".into()))?;
            segments
                .pop_if_empty()
                .push("repos")
                .push(&repo.owner)
                .push(&repo.name);
            for part in parts {
                segments.push(part);
            }
        }
        Ok(url)
    }
    pub fn fetch(
        &self,
        mut url: reqwest::Url,
        accept: &str,
        max: usize,
        options: &LoadOptions,
        missing: bool,
    ) -> Result<Option<Vec<u8>>, Error> {
        if options.offline {
            return Err(Error::Operation(
                "GitHub source is unavailable offline".into(),
            ));
        }
        let api = reqwest::Url::parse(&self.config.api)
            .map_err(|_| Error::Invalid("invalid GitHub API URL".into()))?;
        for redirect in 0..=4 {
            let remaining = self
                .deadline
                .checked_duration_since(std::time::Instant::now())
                .ok_or_else(|| Error::Operation("GitHub transfer deadline exceeded".into()))?;
            self.requests.set(self.requests.get() + 1);
            if self.requests.get() > 256 {
                return Err(Error::Invalid("GitHub request budget exceeded".into()));
            }
            let mut request = self
                .client
                .get(url.clone())
                .timeout(remaining)
                .header("Accept", accept)
                .header("X-GitHub-Api-Version", "2026-03-10");
            if redirect == 0
                && url.origin() == api.origin()
                && let Some(token) = &self.token
            {
                let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
                    .map_err(|_| Error::Invalid("invalid GitHub credential".into()))?;
                header.set_sensitive(true);
                request = request.header("Authorization", header);
            }
            let response = request.send().map_err(|e| {
                Error::Operation(format!("GitHub request failed: {}", e.without_url()))
            })?;
            if missing && response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(None);
            }
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| Error::Operation("GitHub redirect has no location".into()))?;
                let next = url
                    .join(location)
                    .map_err(|_| Error::Permission("invalid GitHub download redirect".into()))?;
                let known = matches!(
                    next.host_str(),
                    Some(
                        "github.com"
                            | "objects.githubusercontent.com"
                            | "release-assets.githubusercontent.com"
                            | "codeload.github.com"
                    )
                );
                let same = next.origin() == api.origin();
                if !next.username().is_empty()
                    || next.password().is_some()
                    || next.fragment().is_some()
                    || (!same && !(next.scheme() == "https" && known))
                {
                    return Err(Error::Permission(
                        "GitHub download redirect destination is not allowed".into(),
                    ));
                }
                url = next;
                continue;
            }
            if !response.status().is_success() {
                let rate = response
                    .headers()
                    .get("x-ratelimit-remaining")
                    .and_then(|h| h.to_str().ok())
                    == Some("0")
                    || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS;
                return Err(Error::Operation(if rate {
                    "GitHub rate limit reached; retry after the server reset".into()
                } else {
                    format!("GitHub request failed: HTTP {}", response.status())
                }));
            }
            let mut bytes = Vec::new();
            response.take(max as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > max {
                return Err(Error::Invalid("GitHub response exceeds size limit".into()));
            }
            return Ok(Some(bytes));
        }
        Err(Error::Permission(
            "GitHub download redirect limit exceeded".into(),
        ))
    }
    pub fn metadata(
        &self,
        url: reqwest::Url,
        cache: &Cache,
        options: &LoadOptions,
    ) -> Result<Value, Error> {
        let key = self.cache_key(url.as_str());
        let _guard = cache.lock(&key)?;
        if (options.mode != LoadMode::Update || options.offline)
            && let Some(bytes) = cache.metadata(&key)?
        {
            return Ok(serde_json::from_slice(&bytes)?);
        }
        let bytes = self
            .fetch(
                url,
                "application/vnd.github+json",
                1024 * 1024,
                options,
                false,
            )?
            .ok_or_else(|| Error::Invalid("GitHub metadata is missing".into()))?;
        let value = serde_json::from_slice(&bytes)?;
        cache.save_metadata(&key, &bytes)?;
        Ok(value)
    }
    pub fn revision(
        &self,
        repo: &Repo,
        reference: &str,
        cache: &Cache,
        options: &LoadOptions,
    ) -> Result<String, Error> {
        if commit(reference).is_ok() {
            return Ok(reference.to_ascii_lowercase());
        }
        if reference.is_empty() || reference.len() > 256 {
            return Err(Error::Invalid("invalid GitHub ref".into()));
        }
        let value = self.metadata(self.url(repo, &["commits", reference])?, cache, options)?;
        let sha = value["sha"]
            .as_str()
            .ok_or_else(|| Error::Invalid("GitHub commit SHA is missing".into()))?;
        commit(sha)?;
        Ok(sha.into())
    }
    pub fn file(
        &self,
        repo: &Repo,
        revision: &str,
        path: &str,
        cache: &Cache,
        options: &LoadOptions,
    ) -> Result<Option<Vec<u8>>, Error> {
        relative(path)?;
        commit(revision)?;
        let key = self.cache_key(&format!("{}@{revision}/{path}", repo.name()));
        let _guard = cache.lock(&key)?;
        if let Some(bytes) = cache.metadata(&key)? {
            return Ok(Some(bytes));
        }
        let mut parts = vec!["contents"];
        parts.extend(path.split('/'));
        let mut url = self.url(repo, &parts)?;
        url.query_pairs_mut().append_pair("ref", revision);
        let Some(bytes) = self.fetch(
            url,
            "application/vnd.github.object+json",
            2 * 1024 * 1024,
            options,
            true,
        )?
        else {
            return Ok(None);
        };
        let value: Value = serde_json::from_slice(&bytes)?;
        if value["type"] != "file"
            || value.get("submodule_git_url").is_some()
            || value.get("target").is_some()
        {
            return Err(
                lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::PermissionDenied)
                    .with_details(serde_json::json!({"restriction":"regular_source_file"}))
                    .with_retryable(false)
                    .into(),
            );
        }
        // Contents API may dereference symlinks; verify the tree mode at the same commit.
        let blob = self.regular_file(repo, revision, path, cache, options)?;
        let encoded = value["content"]
            .as_str()
            .ok_or_else(|| Error::Invalid("GitHub file content is missing or too large".into()))?;
        if value["encoding"] != "base64" {
            return Err(Error::Invalid("unsupported GitHub file encoding".into()));
        }
        use base64::Engine;
        let body = base64::engine::general_purpose::STANDARD
            .decode(
                encoded
                    .bytes()
                    .filter(|b| !b.is_ascii_whitespace())
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| Error::Invalid("invalid GitHub file encoding".into()))?;
        if body.len() > 1024 * 1024 {
            return Err(Error::Invalid("GitHub file exceeds 1 MiB".into()));
        }
        use sha1::{Digest, Sha1};
        let mut hash = Sha1::new();
        hash.update(format!("blob {}\0", body.len()).as_bytes());
        hash.update(&body);
        if value["sha"].as_str() != Some(blob.as_str()) || format!("{:x}", hash.finalize()) != blob
        {
            return Err(Error::Permission(
                "GitHub file does not match the pinned Git blob".into(),
            ));
        }
        cache.save_metadata(&key, &body)?;
        Ok(Some(body))
    }
    fn regular_file(
        &self,
        repo: &Repo,
        revision: &str,
        path: &str,
        cache: &Cache,
        options: &LoadOptions,
    ) -> Result<String, Error> {
        let mut tree = revision.to_owned();
        let pieces: Vec<_> = path.split('/').collect();
        for (index, name) in pieces.iter().enumerate() {
            let value = self.metadata(
                self.url(repo, &["git", "trees", &tree])?,
                cache,
                &LoadOptions {
                    mode: LoadMode::Execute,
                    ..options.clone()
                },
            )?;
            if value["truncated"] == true {
                return Err(Error::Invalid("GitHub tree listing is truncated".into()));
            }
            let entry = value["tree"]
                .as_array()
                .and_then(|entries| entries.iter().find(|e| e["path"] == *name))
                .ok_or_else(|| lenso_cli::Diagnostic::new(lenso_cli::DiagnosticCode::InvalidInput)
                    .with_details(serde_json::json!({"field":"source.path","reason":"not_found_in_snapshot"})))?;
            let last = index + 1 == pieces.len();
            if last {
                if entry["type"] != "blob"
                    || !matches!(entry["mode"].as_str(), Some("100644" | "100755"))
                {
                    return Err(lenso_cli::Diagnostic::new(
                        lenso_cli::DiagnosticCode::PermissionDenied,
                    )
                    .with_details(serde_json::json!({"restriction":"regular_source_file"}))
                    .with_retryable(false)
                    .into());
                }
                let blob = entry["sha"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("GitHub blob SHA is missing".into()))?;
                commit(blob)?;
                return Ok(blob.into());
            } else {
                if entry["type"] != "tree" {
                    return Err(Error::Permission(
                        "GitHub path traverses a non-directory".into(),
                    ));
                }
                tree = entry["sha"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("GitHub tree SHA is missing".into()))?
                    .into();
            }
        }
        Err(Error::Invalid("GitHub file path is empty".into()))
    }
    pub fn asset(
        &self,
        repo: &Repo,
        id: u64,
        max: usize,
        options: &LoadOptions,
    ) -> Result<Vec<u8>, Error> {
        self.fetch(
            self.url(repo, &["releases", "assets", &id.to_string()])?,
            "application/octet-stream",
            max,
            options,
            false,
        )?
        .ok_or_else(|| Error::Invalid("GitHub asset is missing".into()))
    }
}

pub(crate) fn is_default(config: &GithubConfig) -> bool {
    config == &GithubConfig::default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repository_and_snapshot_paths_reject_transport_and_traversal_injection() {
        for value in [
            "acme/repo/extra",
            "acme/..",
            "acme/repo?token=x",
            "user@host/repo",
        ] {
            assert!(Repo::parse(value).is_err());
        }
        for value in [
            "../secret",
            "/secret",
            "a/../../secret",
            "a\\b",
            "C:drive",
            ".git/config",
        ] {
            assert!(relative(value).is_err());
        }
        assert!(relative(&vec!["dir"; 17].join("/")).is_err());
        assert!(commit("main").is_err());
        assert!(commit(&"a".repeat(40)).is_ok());
        let github = Github::new(&GithubConfig::default()).unwrap();
        let url = github
            .url(
                &Repo::parse("acme/repo").unwrap(),
                &["commits", "feature/example"],
            )
            .unwrap();
        assert!(url.as_str().ends_with("/commits/feature%2Fexample"));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[test]
    #[ignore = "requires network and an explicitly configured LENSO_LIVE_GH_TOKEN"]
    fn live_github_release_asset_download_verifies_the_advertised_digest() {
        let github = Github::new(&GithubConfig {
            token_env: Some("LENSO_LIVE_GH_TOKEN".into()),
            ..Default::default()
        })
        .unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let cache = Cache::at(cache_dir.path().into()).unwrap();
        let options = LoadOptions::default();
        let repo = Repo::parse("cli/cli").unwrap();
        let release = github
            .metadata(
                github.url(&repo, &["releases", "latest"]).unwrap(),
                &cache,
                &options,
            )
            .unwrap();
        let asset = release["assets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|asset| {
                asset["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("_checksums.txt"))
            })
            .unwrap();
        let bytes = github
            .asset(&repo, asset["id"].as_u64().unwrap(), 1024 * 1024, &options)
            .unwrap();
        assert_eq!(
            format!("sha256:{}", digest(&bytes)),
            asset["digest"].as_str().unwrap()
        );
        assert!(
            std::str::from_utf8(&bytes)
                .unwrap()
                .contains("linux_amd64.tar.gz")
        );
    }
}
