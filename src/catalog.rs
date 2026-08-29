use std::{collections::BTreeSet, fs, io::Read, path::Path, time::Duration};

use anyhow::{Context, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

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

fn parse_catalog(bytes: &[u8]) -> anyhow::Result<Vec<CatalogPluginRelease>> {
    let catalog: PluginCatalog = serde_json::from_slice(bytes).context("decode Plugin catalog")?;
    if catalog.schema != "lenso.plugin-catalog.v1" {
        bail!("unsupported Plugin catalog schema `{}`", catalog.schema);
    }
    let mut identities = BTreeSet::new();
    for release in &catalog.plugins {
        if release.plugin_id.trim().is_empty()
            || release.version.trim().is_empty()
            || release.summary.trim().is_empty()
        {
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
    let digest = format!(
        "sha256:{}",
        Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    if digest != release.bundle_digest {
        bail!(
            "downloaded Plugin Bundle digest `{digest}` does not match catalog `{}`",
            release.bundle_digest
        );
    }
    fs::write(output, bytes).with_context(|| format!("write {}", output.display()))
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
    if value.starts_with("https://")
        || value.starts_with("http://127.0.0.1:")
        || value.starts_with("http://localhost:")
        || value.starts_with("http://[::1]:")
    {
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
}
