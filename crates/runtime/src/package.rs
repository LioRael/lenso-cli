//! Deterministic author packaging and GitHub Release package resolution.
use crate::{
    Cache, Extension, HandlerSpec, LoadOptions,
    cache::{check_digest, digest},
    github::{Github, Repo, relative},
    project::{atomic_write, read_bounded},
};
use lenso_cli::Error;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
const MAX_ARCHIVE: usize = 32 * 1024 * 1024;
const MAX_EXPANDED: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageConfig {
    pub id: String,
    #[serde(default = "root_path")]
    pub path: String,
    #[serde(default = "tag_pattern")]
    pub tag: String,
    #[serde(default = "asset_pattern")]
    pub asset: String,
    #[serde(default)]
    pub include: Vec<String>,
}
fn root_path() -> String {
    ".".into()
}
fn tag_pattern() -> String {
    "v{version}".into()
}
fn asset_pattern() -> String {
    "lenso-{id}-{version}.tar.gz".into()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Descriptor {
    schema_version: u32,
    extensions: Vec<PackageConfig>,
}
fn descriptor(bytes: &[u8], id: &str) -> Result<PackageConfig, Error> {
    let value: Descriptor = toml::from_str(
        std::str::from_utf8(bytes)
            .map_err(|_| Error::Invalid("distribution descriptor is not UTF-8".into()))?,
    )
    .map_err(|e| Error::Invalid(format!("lenso.registry.toml: {e}")))?;
    if value.schema_version != 1 || value.extensions.len() > 128 {
        return Err(Error::Invalid(
            "invalid distribution descriptor schema or package count".into(),
        ));
    }
    let mut ids = BTreeSet::new();
    for entry in &value.extensions {
        if !ids.insert(&entry.id) {
            return Err(Error::Invalid("duplicate distribution package ID".into()));
        }
    }
    value
        .extensions
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| Error::Invalid(format!("package {id} is not declared by the author")))
}
fn pattern(pattern: &str, id: &str, version: &str) -> Result<String, Error> {
    let value = pattern.replace("{id}", id).replace("{version}", version);
    if value.contains(['{', '}']) {
        return Err(Error::Invalid(
            "unknown distribution pattern placeholder".into(),
        ));
    }
    relative(&value)?;
    Ok(value)
}
pub(crate) fn publication_conventions(
    bytes: Option<&[u8]>,
    id: &str,
    version: &str,
) -> Result<(String, String), Error> {
    let config = match bytes {
        Some(bytes) => descriptor(bytes, id)?,
        None => PackageConfig {
            id: id.into(),
            path: root_path(),
            tag: tag_pattern(),
            asset: asset_pattern(),
            include: Vec::new(),
        },
    };
    Ok((
        pattern(&config.tag, id, version)?,
        pattern(&config.asset, id, version)?,
    ))
}

fn version_of(tag: &str, pattern: &str, id: &str) -> Result<Option<semver::Version>, Error> {
    let pattern = pattern.replace("{id}", id);
    let Some((before, after)) = pattern.split_once("{version}") else {
        return Err(Error::Invalid(
            "tag pattern must contain one {version}".into(),
        ));
    };
    if after.contains('{') || before.contains('{') {
        return Err(Error::Invalid("invalid tag pattern".into()));
    }
    Ok(tag
        .strip_prefix(before)
        .and_then(|s| s.strip_suffix(after))
        .and_then(|s| s.parse().ok()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FilePin {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleaseMetadata {
    pub schema_version: u32,
    pub id: String,
    pub version: String,
    pub manifest: FilePin,
    pub archive: FilePin,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReleasePin {
    pub api: String,
    pub repository: Repo,
    pub discovery_commit: String,
    pub release_id: u64,
    pub tag: String,
    pub metadata_sha256: String,
    pub manifest_asset: u64,
    pub archive_asset: u64,
    pub authenticated: bool,
    pub metadata: ReleaseMetadata,
}
impl ReleasePin {
    pub fn validate(&self) -> Result<(), Error> {
        check_digest(&self.metadata_sha256)?;
        check_digest(&self.metadata.manifest.sha256)?;
        check_digest(&self.metadata.archive.sha256)?;
        crate::github::commit(&self.discovery_commit)?;
        relative(&self.metadata.manifest.name)?;
        relative(&self.metadata.archive.name)?;
        if self.metadata.schema_version != 1
            || self.metadata.manifest.size > 1024 * 1024
            || self.metadata.archive.size > MAX_ARCHIVE as u64
        {
            return Err(Error::Invalid(
                "invalid release package limits or schema".into(),
            ));
        }
        Ok(())
    }
}
pub(crate) fn reference(source: &str) -> Result<Option<(Repo, Option<String>)>, Error> {
    let Some(value) = source.strip_prefix("github-release:") else {
        return Ok(None);
    };
    let parts: Vec<_> = value.split('/').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(Error::Invalid(
            "GitHub Release source requires owner/repo[/package]".into(),
        ));
    }
    let repo = Repo::parse(&parts[..2].join("/"))?;
    let selector = parts.get(2).map(|s| s.to_string());
    if let Some(selector) = &selector {
        relative(selector)?;
    }
    Ok(Some((repo, selector)))
}
fn asset_id(release: &serde_json::Value, name: &str) -> Result<u64, Error> {
    let matches: Vec<_> = release["assets"]
        .as_array()
        .ok_or_else(|| Error::Invalid("GitHub release assets are missing".into()))?
        .iter()
        .filter(|a| a["name"] == name)
        .collect();
    if matches.len() != 1 {
        return Err(Error::Invalid(format!(
            "release needs exactly one {name} asset"
        )));
    }
    matches[0]["id"]
        .as_u64()
        .ok_or_else(|| Error::Invalid("GitHub asset ID is missing".into()))
}
pub(crate) fn resolve(
    source: &str,
    id: &str,
    requirement: Option<&str>,
    selected_version: Option<&semver::Version>,
    github: &Github,
    cache: &Cache,
    options: &LoadOptions,
) -> Result<(ReleasePin, Vec<u8>), Error> {
    let (repo, selector) = reference(source)?
        .ok_or_else(|| Error::Invalid("expected GitHub Release source".into()))?;
    let package = selector.as_deref().unwrap_or(id);
    let revision = github.revision(&repo, "HEAD", cache, options)?;
    let config = match github.file(&repo, &revision, "lenso.registry.toml", cache, options)? {
        Some(bytes) => descriptor(&bytes, package)?,
        None if selector.is_none() => PackageConfig {
            id: id.into(),
            path: root_path(),
            tag: tag_pattern(),
            asset: asset_pattern(),
            include: Vec::new(),
        },
        None => {
            return Err(Error::Invalid(
                "a selected package needs lenso.registry.toml".into(),
            ));
        }
    };
    let requirement: semver::VersionReq = requirement
        .unwrap_or("*")
        .parse()
        .map_err(|e: semver::Error| Error::Invalid(e.to_string()))?;
    let mut selected: Option<(semver::Version, serde_json::Value)> = None;
    for page in 1..=128 {
        let mut url = github.url(&repo, &["releases"])?;
        url.query_pairs_mut()
            .append_pair("per_page", "30")
            .append_pair("page", &page.to_string());
        let value = github.metadata(url, cache, options)?;
        let entries = value
            .as_array()
            .ok_or_else(|| Error::Invalid("invalid GitHub release list".into()))?;
        for entry in entries {
            if entry["draft"] == true {
                continue;
            }
            let tag = entry["tag_name"]
                .as_str()
                .ok_or_else(|| Error::Invalid("GitHub release tag is missing".into()))?;
            if let Some(version) = version_of(tag, &config.tag, &config.id)?
                && requirement.matches(&version)
                && selected_version.is_none_or(|selected| selected == &version)
                && (entry["prerelease"] != true || !version.pre.is_empty())
                && selected.as_ref().is_none_or(|(old, _)| version > *old)
            {
                selected = Some((version, entry.clone()));
            }
        }
        if entries.len() < 30 {
            break;
        }
        if page == 128 {
            return Err(Error::Invalid(
                "GitHub release pagination limit exceeded".into(),
            ));
        }
    }
    let (version, release) = selected.ok_or_else(|| {
        Error::Invalid("no GitHub release satisfies the version requirement".into())
    })?;
    let metadata_id = asset_id(&release, "lenso-release.json")?;
    let key = github.cache_key(&format!("release-metadata:{}:{}", repo.name(), metadata_id));
    let cached = if options.mode != crate::LoadMode::Update || options.offline {
        cache.metadata(&key)?
    } else {
        None
    };
    let bytes = match cached {
        Some(bytes) => bytes,
        None => github.asset(&repo, metadata_id, 1024 * 1024, options)?,
    };
    let metadata: ReleaseMetadata = serde_json::from_slice(&bytes)?;
    if metadata.id != id
        || metadata.version != version.to_string()
        || metadata.archive.name != pattern(&config.asset, &config.id, &metadata.version)?
    {
        return Err(Error::Invalid(
            "release package identity/version/asset mismatch".into(),
        ));
    }
    let pin = ReleasePin {
        api: github.config.api.clone(),
        repository: repo,
        discovery_commit: revision,
        release_id: release["id"]
            .as_u64()
            .ok_or_else(|| Error::Invalid("release ID is missing".into()))?,
        tag: release["tag_name"].as_str().unwrap_or_default().into(),
        metadata_sha256: digest(&bytes),
        manifest_asset: asset_id(&release, &metadata.manifest.name)?,
        archive_asset: asset_id(&release, &metadata.archive.name)?,
        authenticated: github.authenticated(),
        metadata,
    };
    pin.validate()?;
    let path = cache.ensure(&pin.metadata.manifest.sha256, 1024 * 1024, || {
        github.asset(&pin.repository, pin.manifest_asset, 1024 * 1024, options)
    })?;
    let manifest = read_bounded(&path, 1024 * 1024)?;
    if manifest.len() as u64 != pin.metadata.manifest.size {
        return Err(Error::Invalid("release manifest size mismatch".into()));
    }
    cache.save_metadata(&key, &bytes)?;
    Ok((pin, manifest))
}

pub(crate) fn archive_files(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut expanded = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_EXPANDED as u64 + 1)
        .read_to_end(&mut expanded)?;
    if expanded.len() > MAX_EXPANDED {
        return Err(Error::Invalid(
            "expanded archive stream exceeds 64 MiB".into(),
        ));
    }
    let mut archive = tar::Archive::new(expanded.as_slice());
    let mut files = BTreeMap::new();
    let mut size: usize = 0;
    for (count, entry) in archive.entries()?.enumerate() {
        if count >= 256 {
            return Err(Error::Invalid("package entry limit exceeded".into()));
        }
        let mut entry = entry?;
        let path = entry
            .path()?
            .to_str()
            .ok_or_else(|| Error::Invalid("package path is not UTF-8".into()))?
            .trim_end_matches('/')
            .to_owned();
        relative(&path)?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file() || files.contains_key(&path) {
            return Err(Error::Permission(
                "package links, special files and duplicate paths are forbidden".into(),
            ));
        }
        let length = usize::try_from(entry.size())
            .map_err(|_| Error::Invalid("package entry is too large".into()))?;
        size = size
            .checked_add(length)
            .ok_or_else(|| Error::Invalid("expanded package is too large".into()))?;
        if size > MAX_EXPANDED || files.len() >= 128 {
            return Err(Error::Invalid("expanded package limits exceeded".into()));
        }
        let mut body = Vec::new();
        entry
            .by_ref()
            .take(length as u64 + 1)
            .read_to_end(&mut body)?;
        if body.len() != length {
            return Err(Error::Invalid("truncated package entry".into()));
        }
        files.insert(path, body);
    }
    Ok(files)
}
pub(crate) fn module(
    pin: &ReleasePin,
    definition: &Extension,
    name: &str,
    github: &Github,
    cache: &Cache,
    options: &LoadOptions,
) -> Result<PathBuf, Error> {
    pin.validate()?;
    if pin.api != github.config.api || (pin.authenticated && !github.authenticated()) {
        return Err(Error::Permission(
            "locked GitHub source requires its configured API and credentials".into(),
        ));
    }
    let spec = definition
        .handlers
        .values()
        .find_map(|handler| match handler {
            HandlerSpec::Wasm(spec) if spec.module == name => Some(spec),
            _ => None,
        })
        .ok_or_else(|| Error::Invalid("package module is missing".into()))?;
    if cache.read(&spec.sha256, 16 * 1024 * 1024)?.is_some() {
        return cache.blob_path(&spec.sha256);
    }
    let path = cache.ensure(&pin.metadata.archive.sha256, MAX_ARCHIVE, || {
        github.asset(&pin.repository, pin.archive_asset, MAX_ARCHIVE, options)
    })?;
    let archive = read_bounded(&path, MAX_ARCHIVE)?;
    if archive.len() as u64 != pin.metadata.archive.size {
        return Err(Error::Invalid("archive size mismatch".into()));
    }
    let files = archive_files(&archive)?;
    let manifest = files
        .get(&pin.metadata.manifest.name)
        .ok_or_else(|| Error::Invalid("archive manifest is missing".into()))?;
    if digest(manifest) != pin.metadata.manifest.sha256 {
        return Err(Error::Permission("archive manifest digest mismatch".into()));
    }
    for handler in definition.handlers.values() {
        if let HandlerSpec::Wasm(spec) = handler {
            let bytes = files
                .get(&spec.module)
                .ok_or_else(|| Error::Invalid("archive module is missing".into()))?;
            if bytes.len() > 16 * 1024 * 1024 || digest(bytes) != spec.sha256 {
                return Err(Error::Permission("archive module digest mismatch".into()));
            }
        }
    }
    cache.ensure(&spec.sha256, 16 * 1024 * 1024, || {
        files
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Invalid("archive module is missing".into()))
    })
}

/// Build deterministic GitHub Release assets from a local, prebuilt extension.
///
/// # Errors
/// Rejects missing modules, unsafe paths, changed digests, and invalid author configuration.
pub fn package_extension(
    project: &Path,
    selector: Option<&str>,
    output: &Path,
) -> Result<serde_json::Value, Error> {
    let project = project.canonicalize()?;
    let descriptor_path = crate::project_path(&project, Path::new("lenso.registry.toml"))?;
    let config = if descriptor_path.exists() {
        let bytes = read_bounded(&descriptor_path, 1024 * 1024)?;
        let parsed: Descriptor = toml::from_str(
            std::str::from_utf8(&bytes)
                .map_err(|_| Error::Invalid("descriptor is not UTF-8".into()))?,
        )
        .map_err(|e| Error::Invalid(e.to_string()))?;
        let selected = selector
            .or_else(|| (parsed.extensions.len() == 1).then(|| parsed.extensions[0].id.as_str()))
            .ok_or_else(|| Error::Invalid("select a package with --id".into()))?;
        descriptor(&bytes, selected)?
    } else {
        PackageConfig {
            id: selector.unwrap_or("").into(),
            path: root_path(),
            tag: tag_pattern(),
            asset: asset_pattern(),
            include: Vec::new(),
        }
    };
    let base = if config.path == "." {
        project.clone()
    } else {
        crate::project_path(&project, Path::new(&config.path))?
    };
    let bytes = read_bounded(
        &crate::project_path(&base, Path::new("extension.toml"))?,
        1024 * 1024,
    )?;
    let mut definition: Extension = toml::from_str(
        std::str::from_utf8(&bytes).map_err(|_| Error::Invalid("manifest is not UTF-8".into()))?,
    )
    .map_err(|e| Error::Invalid(e.to_string()))?;
    validate_definition(&definition)?;
    if !config.id.is_empty() && config.id != definition.id {
        return Err(Error::Invalid(
            "author package ID does not match manifest".into(),
        ));
    }
    let version: semver::Version = definition
        .version
        .parse()
        .map_err(|e: semver::Error| Error::Invalid(e.to_string()))?;
    let mut files = BTreeMap::new();
    for include in &config.include {
        relative(include)?;
        if include != "extension.toml" {
            files.insert(
                include.clone(),
                read_bounded(
                    &crate::project_path(&base, Path::new(include))?,
                    16 * 1024 * 1024,
                )?,
            );
        }
    }
    for handler in definition.handlers.values_mut() {
        if let HandlerSpec::Wasm(spec) = handler {
            relative(&spec.module)?;
            let bytes = read_bounded(
                &crate::project_path(&base, Path::new(&spec.module))?,
                16 * 1024 * 1024,
            )?;
            let sha = digest(&bytes);
            if spec.sha256 != "auto" && spec.sha256 != sha {
                return Err(Error::Permission(
                    "prebuilt module does not match its manifest digest".into(),
                ));
            }
            spec.sha256 = sha;
            files.insert(spec.module.clone(), bytes);
        }
    }
    let manifest = toml::to_string_pretty(&definition)
        .map_err(|e| Error::Invalid(e.to_string()))?
        .into_bytes();
    files.insert("extension.toml".into(), manifest.clone());
    let encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for (name, bytes) in &files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();
        archive.append_data(&mut header, name, bytes.as_slice())?;
    }
    let bytes = archive.into_inner()?.finish()?;
    if bytes.len() > MAX_ARCHIVE {
        return Err(Error::Invalid("package archive exceeds 32 MiB".into()));
    }
    archive_files(&bytes)?;
    let name = pattern(&config.asset, &definition.id, &version.to_string())?;
    if matches!(name.as_str(), "extension.toml" | "lenso-release.json") {
        return Err(Error::Invalid(
            "archive asset name conflicts with release metadata".into(),
        ));
    }
    if name.contains('/') {
        return Err(Error::Invalid(
            "release asset name cannot contain directories".into(),
        ));
    }
    let tag = pattern(&config.tag, &definition.id, &version.to_string())?;
    let metadata = ReleaseMetadata {
        schema_version: 1,
        id: definition.id,
        version: version.to_string(),
        manifest: FilePin {
            name: "extension.toml".into(),
            sha256: digest(&manifest),
            size: manifest.len() as u64,
        },
        archive: FilePin {
            name: name.clone(),
            sha256: digest(&bytes),
            size: bytes.len() as u64,
        },
    };
    std::fs::create_dir_all(output)?;
    atomic_write(&output.join(&name), &bytes)?;
    atomic_write(&output.join("extension.toml"), &manifest)?;
    atomic_write(
        &output.join("lenso-release.json"),
        &serde_json::to_vec_pretty(&metadata)?,
    )?;
    Ok(
        serde_json::json!({"message":format!("Packaged {} {}",metadata.id,metadata.version),"tag":tag,"directory":output,"archive":name,"sha256":metadata.archive.sha256}),
    )
}

fn validate_definition(definition: &Extension) -> Result<(), Error> {
    crate::manifest::validate_metadata(definition)?;
    let mut check = lenso_cli::Cli::new("packaging");
    crate::register_management(&mut check)?;
    let namespace = format!("{}/", definition.id);
    let mut handlers: BTreeSet<_> = definition.handlers.keys().cloned().collect();
    for step in definition
        .workflows
        .iter()
        .flat_map(|workflow| &workflow.steps)
    {
        for id in std::iter::once(&step.handler)
            .chain(&step.hooks.prepare)
            .chain(&step.hooks.before)
            .chain(&step.hooks.validate)
            .chain(&step.hooks.after)
        {
            if !id.starts_with(&namespace) {
                handlers.insert(id.clone());
            }
        }
    }
    // Host-owned references are checked by the consuming application, without executing them here.
    for id in handlers {
        if !check.engine_mut().contains_handler(&id) {
            check.engine_mut().handler(id, |_| async {
                Err(Error::Operation("validation-only handler".into()))
            })?;
        }
    }
    for workflow in &definition.workflows {
        check.engine_mut().workflow(workflow.clone())?;
    }
    for command in &definition.commands {
        check.alias(command.clone())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(entries: &[(&str, u8, &[u8])]) -> Vec<u8> {
        let encoder = flate2::GzBuilder::new()
            .mtime(0)
            .write(Vec::new(), flate2::Compression::fast());
        let mut tar = tar::Builder::new(encoder);
        for (name, kind, body) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_path(name).unwrap();
            header.set_entry_type(tar::EntryType::new(*kind));
            header.set_mode(0o644);
            header.set_size(body.len() as u64);
            header.set_cksum();
            tar.append(&header, *body).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }
    #[test]
    fn package_archives_reject_duplicate_paths_links_and_unsafe_names() {
        assert!(
            archive_files(&archive(&[
                ("code.wasm", b'0', b"one"),
                ("code.wasm", b'0', b"two")
            ]))
            .is_err()
        );
        for kind in *b"1236" {
            assert!(archive_files(&archive(&[("code.wasm", kind, b"")])).is_err());
        }
        for name in ["../escape", "/absolute", "C:drive", "a\\b", ".git/config"] {
            assert!(relative(name).is_err());
        }
        assert_eq!(
            archive_files(&archive(&[("nested/code.wasm", b'0', b"safe")])).unwrap()["nested/code.wasm"],
            b"safe"
        );
    }
    #[test]
    fn custom_tag_patterns_select_versions_without_treating_refs_as_paths() {
        assert_eq!(
            version_of("code-v1.2.3", "{id}-v{version}", "code")
                .unwrap()
                .unwrap()
                .to_string(),
            "1.2.3"
        );
        assert!(
            version_of("other-v1.2.3", "{id}-v{version}", "code")
                .unwrap()
                .is_none()
        );
        assert!(version_of("v1", "{version}{version}", "code").is_err());
        assert!(reference("github-release:acme/repo/../escape").is_err());
    }
}

#[cfg(test)]
mod expansion_tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn compressed_stream_limit_includes_hidden_tar_headers_and_trailing_padding() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let chunk = vec![0; 1024 * 1024];
        for _ in 0..64 {
            encoder.write_all(&chunk).unwrap();
        }
        encoder.write_all(&[0]).unwrap();
        let compressed = encoder.finish().unwrap();
        assert!(compressed.len() < 1024 * 1024);
        assert!(
            archive_files(&compressed)
                .unwrap_err()
                .to_string()
                .contains("archive stream exceeds")
        );
    }
}
