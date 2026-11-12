//! Lenso extension distribution metadata; independent of extension resource protocols.
use crate::{Cache, LoadOptions, cache::check_digest, source::Source};
use lenso_cli::Error;
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema_version: u32,
    name: String,
    releases: Vec<Release>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    version: semver::Version,
    manifest: String,
    sha256: String,
    #[serde(default)]
    yanked: bool,
}

pub(crate) struct Package {
    pub manifest: String,
    pub sha256: String,
    pub version: semver::Version,
}

pub(crate) fn reference(source: &str) -> Result<Option<(&str, &str)>, Error> {
    let Some(name) = source.strip_prefix("registry:") else {
        return Ok(None);
    };
    let Some((namespace, package)) = name.split_once('/') else {
        return Err(Error::Invalid(
            "Registry source must be registry:@namespace/package".into(),
        ));
    };
    let valid = |value: &str| {
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    };
    if !namespace.strip_prefix('@').is_some_and(valid) || !valid(package) {
        return Err(Error::Invalid("invalid extension Registry name".into()));
    }
    Ok(Some((namespace, package)))
}

pub(crate) fn route<'a>(
    source: &str,
    registries: &'a std::collections::BTreeMap<String, String>,
) -> Result<Option<&'a str>, Error> {
    let Some((namespace, _)) = reference(source)? else {
        return Ok(None);
    };
    registries
        .get(namespace)
        .map(|url| Some(url.as_str()))
        .ok_or_else(|| {
            Error::Invalid(format!(
                "extension Registry {namespace} is not configured in cli.registries"
            ))
        })
}

pub(crate) fn validate_base(project: &Path, base: &str) -> Result<(), Error> {
    let Source::Remote(url) = Source::parse(project, base)? else {
        return Err(Error::Invalid(
            "extension Registry requires an HTTPS base URL".into(),
        ));
    };
    if !url.path().ends_with('/') || url.query().is_some() {
        return Err(Error::Invalid(
            "extension Registry base URL must end in / without a query".into(),
        ));
    }
    Ok(())
}

pub(crate) fn resolve(
    project: &Path,
    source: &str,
    base: &str,
    requirement: Option<&str>,
    cache: &Cache,
    options: &LoadOptions,
) -> Result<Package, Error> {
    validate_base(project, base)?;
    let (_, package) =
        reference(source)?.ok_or_else(|| Error::Invalid("expected a Registry reference".into()))?;
    let base = reqwest::Url::parse(base)
        .map_err(|_| Error::Invalid("invalid extension Registry URL".into()))?;
    let index_url = base
        .join(&format!("{package}.json"))
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let index_source = Source::parse(project, index_url.as_str())?;
    let requirement = semver::VersionReq::parse(requirement.unwrap_or("*"))
        .map_err(|e| Error::Invalid(e.to_string()))?;
    index_source.manifest(cache, options, |bytes| {
        let index: Index = serde_json::from_slice(bytes)?;
        if index.schema_version != 1
            || index.name != source.trim_start_matches("registry:")
            || index.releases.len() > 4096
        {
            return Err(Error::Invalid(
                "invalid extension Registry index identity, schema, or release count".into(),
            ));
        }
        let mut versions = BTreeSet::new();
        for release in &index.releases {
            check_digest(&release.sha256)?;
            if !versions.insert(&release.version) {
                return Err(Error::Invalid(
                    "duplicate extension Registry version".into(),
                ));
            }
        }
        let release = index
            .releases
            .into_iter()
            .filter(|release| !release.yanked && requirement.matches(&release.version))
            .max_by(|a, b| a.version.cmp(&b.version))
            .ok_or_else(|| {
                Error::Invalid(format!("no release of {source} satisfies {requirement}"))
            })?;
        let manifest = index_url
            .join(&release.manifest)
            .map_err(|_| Error::Invalid("invalid Registry manifest address".into()))?;
        if manifest.origin() != base.origin() {
            return Err(Error::Invalid(
                "Registry manifests must remain on the configured origin".into(),
            ));
        }
        Source::parse(project, manifest.as_str())?;
        Ok(Package {
            manifest: manifest.into(),
            sha256: release.sha256,
            version: release.version,
        })
    })
}
