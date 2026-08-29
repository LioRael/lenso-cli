use std::{collections::BTreeSet, fs, io::Read, path::Path, time::Duration};

use anyhow::{Context, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use lenso_app_authoring::identity::{validate_plugin_id_v1, validate_release_version};

pub(crate) const DEFAULT_CATALOG_URL: &str = "https://catalog.lenso.dev/v1/plugins.json";
const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct CatalogPluginRelease {
    pub(crate) plugin_id: String,
    pub(crate) version: String,
    pub(crate) summary: String,
    pub(crate) bundle_url: String,
    pub(crate) bundle_digest: String,
    pub(crate) manifest_digest: String,
    pub(crate) host_targets: Vec<String>,
    pub(crate) execution_classes: Vec<String>,
    pub(crate) capabilities: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginCatalog {
    schema: String,
    plugins: Vec<CatalogPluginRelease>,
}

pub(crate) fn fetch_catalog(url: &str) -> anyhow::Result<Vec<CatalogPluginRelease>> {
    validate_url(url, "Plugin catalog")?;
    let bytes = fetch(url, MAX_CATALOG_BYTES)?;
    parse_catalog(&bytes)
}

pub(crate) fn search_catalog(url: &str, query: &str) -> anyhow::Result<Vec<CatalogPluginRelease>> {
    let url = catalog_search_url(url, query)?;
    fetch_catalog(url.as_str())
}

pub(crate) fn fetch_catalog_release(
    url: &str,
    plugin_id: &str,
    version: &str,
) -> anyhow::Result<Vec<CatalogPluginRelease>> {
    validate_plugin_id_v1(plugin_id)?;
    validate_release_version(version)?;
    let url = catalog_release_url(url, plugin_id, version)?;
    fetch_catalog(url.as_str())
}

fn catalog_search_url(url: &str, query: &str) -> anyhow::Result<Url> {
    let mut url = Url::parse(url).context("Plugin catalog URL is invalid")?;
    if !query.is_empty() {
        url.query_pairs_mut().append_pair("q", query);
    }
    Ok(url)
}

fn catalog_release_url(url: &str, plugin_id: &str, version: &str) -> anyhow::Result<Url> {
    let mut url = Url::parse(url).context("Plugin catalog URL is invalid")?;
    url.query_pairs_mut()
        .append_pair("pluginId", plugin_id)
        .append_pair("version", version);
    Ok(url)
}

fn parse_catalog(bytes: &[u8]) -> anyhow::Result<Vec<CatalogPluginRelease>> {
    let catalog: PluginCatalog = serde_json::from_slice(bytes).context("decode Plugin catalog")?;
    if catalog.schema != "lenso.plugin-catalog.v1" {
        bail!("unsupported Plugin catalog schema `{}`", catalog.schema);
    }
    let mut identities = BTreeSet::new();
    for release in &catalog.plugins {
        validate_plugin_id_v1(&release.plugin_id)
            .with_context(|| format!("catalog Plugin id `{}` is invalid", release.plugin_id))?;
        validate_release_version(&release.version).with_context(|| {
            format!(
                "catalog Release version `{}@{}` is invalid",
                release.plugin_id, release.version
            )
        })?;
        if release.summary.trim().is_empty() {
            bail!("Plugin catalog contains an incomplete Release identity");
        }
        if !identities.insert((&release.plugin_id, &release.version)) {
            bail!(
                "Plugin catalog contains duplicate `{}@{}` Releases",
                release.plugin_id,
                release.version
            );
        }
        validate_digest(&release.bundle_digest, "Bundle")?;
        validate_digest(&release.manifest_digest, "Manifest")?;
        validate_url(&release.bundle_url, "Plugin Bundle")?;
        if release.host_targets.is_empty()
            || release.execution_classes.is_empty()
            || release.capabilities.is_empty()
        {
            bail!(
                "Plugin catalog Release `{}@{}` has incomplete compatibility metadata",
                release.plugin_id,
                release.version
            );
        }
    }
    Ok(catalog.plugins)
}

pub(crate) fn download_bundle(release: &CatalogPluginRelease, output: &Path) -> anyhow::Result<()> {
    let bytes = fetch(&release.bundle_url, MAX_BUNDLE_BYTES)
        .with_context(|| format!("download {}@{}", release.plugin_id, release.version))?;
    let digest = sha256_digest(&bytes);
    if digest != release.bundle_digest {
        bail!(
            "downloaded Plugin Bundle digest `{digest}` does not match catalog `{}`",
            release.bundle_digest
        );
    }
    fs::write(output, bytes).with_context(|| format!("write {}", output.display()))
}

fn sha256_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(71);
    output.push_str("sha256:");
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn fetch(url: &str, limit: u64) -> anyhow::Result<Vec<u8>> {
    let response = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(30))
        .redirects(3)
        .build()
        .get(url)
        .call()
        .map_err(|error| anyhow::anyhow!("GET {url} failed: {error}"))?;
    validate_url(response.get_url(), "Redirected response")?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {url}"))?;
    if u64::try_from(bytes.len())? > limit {
        bail!("response from {url} exceeds {} MiB", limit / 1024 / 1024);
    }
    Ok(bytes)
}

fn validate_digest(value: &str, label: &str) -> anyhow::Result<()> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
        || value[7..].bytes().any(|byte| byte.is_ascii_uppercase())
    {
        bail!("{label} digest must be one lowercase SHA-256 digest");
    }
    Ok(())
}

fn validate_url(value: &str, label: &str) -> anyhow::Result<()> {
    let url = Url::parse(value).with_context(|| format!("{label} URL is invalid"))?;
    let is_https = url.scheme() == "https";
    let is_loopback_http = url.scheme() == "http"
        && url.host().is_some_and(|host| match host {
            url::Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
            url::Host::Ipv4(address) => address.is_loopback(),
            url::Host::Ipv6(address) => address.is_loopback(),
        });
    if is_https || is_loopback_http {
        return Ok(());
    }
    bail!("{label} URL must use HTTPS (loopback HTTP is allowed for development)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_catalog_schema() {
        let error = parse_catalog(br#"{"schema":"old","plugins":[]}"#).unwrap_err();
        assert!(error.to_string().contains("unsupported"));
    }

    #[test]
    fn rejects_duplicate_releases() {
        let release = r#"{"pluginId":"example.echo","version":"1.0.0","summary":"Echo","bundleUrl":"https://example.com/echo.lenso-plugin","bundleDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","manifestDigest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","hostTargets":["*"],"executionClasses":["lenso.wasm-component@1"],"capabilities":["example.echo@1"]}"#;
        let document =
            format!(r#"{{"schema":"lenso.plugin-catalog.v1","plugins":[{release},{release}]}}"#);
        let error = parse_catalog(document.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("duplicate"));
    }

    #[test]
    fn search_url_preserves_custom_parameters_and_encodes_the_query() {
        let url = catalog_search_url(
            "https://example.com/v1/plugins.json?channel=preview",
            "agent tools/zh",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://example.com/v1/plugins.json?channel=preview&q=agent+tools%2Fzh"
        );
    }

    #[test]
    fn exact_release_url_preserves_custom_parameters_and_encodes_identity() {
        let url = catalog_release_url(
            "https://example.com/v1/plugins.json?channel=preview",
            "company.support-bot",
            "1.2.3-rc.1+build.7",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://example.com/v1/plugins.json?channel=preview&pluginId=company.support-bot&version=1.2.3-rc.1%2Bbuild.7"
        );
    }

    #[test]
    fn rejects_noncanonical_catalog_identity() {
        let release = r#"{"pluginId":"uppercase","version":"1.0","summary":"Echo","bundleUrl":"https://example.com/echo.lenso-plugin","bundleDigest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","manifestDigest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","hostTargets":["*"],"executionClasses":["lenso.wasm-component@1"],"capabilities":["example.echo@1"]}"#;
        let document = format!(r#"{{"schema":"lenso.plugin-catalog.v1","plugins":[{release}]}}"#);
        let error = parse_catalog(document.as_bytes()).unwrap_err();
        assert!(error.to_string().contains("catalog Plugin id"));
    }

    #[test]
    fn loopback_http_validation_uses_the_parsed_host() {
        assert!(validate_url("http://localhost:8787/catalog", "Catalog").is_ok());
        assert!(validate_url("http://127.0.0.2:8787/catalog", "Catalog").is_ok());
        assert!(validate_url("http://[::1]:8787/catalog", "Catalog").is_ok());
        let deceptive_user_info = format!("http://localhost:8787{}evil.example/catalog", '@');
        assert!(validate_url(&deceptive_user_info, "Catalog").is_err());
        assert!(validate_url("http://127.0.0.1.evil.example/catalog", "Catalog").is_err());
    }
}
