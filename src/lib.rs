//! Validated, atomic authoring operations for one Lenso App Plugin Root.
//!
//! Every mutation resolves the complete candidate against the Host Catalog
//! before changing visible App-owned files. Runtime Generation staging and
//! switching remain the responsibility of the running Host.

use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use lenso_app_plan::ExecutionClassId;
use lenso_app_plan::authoring::{
    HostCatalog, PluginDescriptor, PluginInstanceId, PluginRootInstance, PluginRootSnapshot,
    ResolvedApp, resolve_plugin_root,
};
use lenso_plugin_bundle::{
    ImplementationPolicy, read_bundle_manifest, resolve_implementation, verify_bundle_directory,
};

const PLUGIN_ROOT: &str = "plugins";
const HOST_CATALOG: &str = ".lenso/host-catalog.json";
const BUNDLE_NAME: &str = "plugin.lenso-plugin";
const MAX_CONFIGURATION_BYTES: u64 = 256 * 1024;
const MAX_RESOURCE_FILES: usize = 4_096;
const MAX_RESOURCE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_RESOURCE_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESOURCE_DEPTH: usize = 32;

/// Resolves the App selected by one project root's Host Catalog and Plugin Root.
pub fn load_resolved_app(root: &Path) -> anyhow::Result<ResolvedApp> {
    let host = load_host_catalog(root)?;
    let snapshot = snapshot_plugin_root(root)?;
    resolve_plugin_root(&host, &snapshot).map_err(anyhow::Error::msg)
}

fn load_host_catalog(root: &Path) -> anyhow::Result<HostCatalog> {
    let path = root.join(HOST_CATALOG);
    let metadata = fs::symlink_metadata(&path).with_context(|| {
        format!(
            "Host Catalog is unavailable at {}; build or install the current Host first",
            path.display()
        )
    })?;
    if !metadata.file_type().is_file() {
        bail!("Host Catalog must be a regular file: {}", path.display());
    }
    let bytes = fs::read(&path).with_context(|| format!("read Host Catalog {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("Host Catalog is invalid: {}", path.display()))
}

fn snapshot_plugin_root(root: &Path) -> anyhow::Result<PluginRootSnapshot> {
    let plugin_root = root.join(PLUGIN_ROOT);
    match fs::symlink_metadata(&plugin_root) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => bail!(
            "Plugin Root must be a regular directory: {}",
            plugin_root.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PluginRootSnapshot::default());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("inspect {}", plugin_root.display()));
        }
    }

    let mut releases = Vec::new();
    let mut instances = Vec::new();
    let mut disabled = Vec::new();
    let mut plugin_names = BTreeMap::<String, String>::new();
    let mut directories = read_entries(&plugin_root)?;
    directories.sort_by_key(fs::DirEntry::file_name);
    for entry in directories {
        let name = utf8_name(&entry.path(), &entry.file_name())?;
        if is_ignored_os_metadata(&name) {
            continue;
        }
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            bail!("unknown Plugin Root entry: {}", entry.path().display());
        }
        let plugin_id = name;
        validate_path_identity(&plugin_id, "Plugin ID")?;
        reject_case_collision(&mut plugin_names, &plugin_id, "Plugin ID")?;
        scan_plugin_directory(
            &entry.path(),
            &plugin_id,
            &mut releases,
            &mut instances,
            &mut disabled,
        )?;
    }
    Ok(PluginRootSnapshot::new(releases, instances, disabled))
}

fn scan_plugin_directory(
    directory: &Path,
    plugin_id: &str,
    releases: &mut Vec<PluginDescriptor>,
    instances: &mut Vec<PluginRootInstance>,
    disabled: &mut Vec<PluginInstanceId>,
) -> anyhow::Result<()> {
    let mut normalized = BTreeMap::<String, String>::new();
    let mut configured_instances = BTreeSet::new();
    let mut resource_directories = BTreeMap::<String, PathBuf>::new();
    let mut entries = read_entries(directory)?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = utf8_name(&entry.path(), &entry.file_name())?;
        if is_ignored_os_metadata(&name) {
            continue;
        }
        reject_case_collision(&mut normalized, &name, "Plugin filename")?;
        let file_type = entry.file_type()?;
        if name == BUNDLE_NAME {
            if !file_type.is_dir() {
                bail!(
                    "Plugin Bundle must be a regular directory: {}",
                    entry.path().display()
                );
            }
            releases.push(read_bundle_descriptor(&entry.path(), plugin_id)?);
            continue;
        }
        if file_type.is_dir() {
            validate_instance_filename(&name)?;
            resource_directories.insert(name, entry.path());
            continue;
        }
        if !file_type.is_file() {
            bail!(
                "Plugin entries cannot be symlinks or special files: {}",
                entry.path().display()
            );
        }
        if let Some(instance) = name.strip_suffix(".toml") {
            validate_instance_filename(instance)?;
            configured_instances.insert(instance.to_owned());
            instances.push(
                PluginRootInstance::new(plugin_id, instance)
                    .with_configuration(read_configuration(&entry.path())?),
            );
        } else if let Some(instance) = name.strip_suffix(".disabled") {
            validate_instance_filename(instance)?;
            if fs::metadata(entry.path())?.len() != 0 {
                bail!("disabled marker must be empty: {}", entry.path().display());
            }
            disabled.push(PluginInstanceId::new(plugin_id, instance));
        } else {
            bail!("unknown Plugin file: {}", entry.path().display());
        }
    }
    for (instance, resource_directory) in resource_directories {
        if !configured_instances.contains(&instance) {
            bail!(
                "orphan Plugin resource directory without `{instance}.toml`: {}",
                resource_directory.display()
            );
        }
        validate_resource_directory(&resource_directory)?;
    }
    Ok(())
}

fn validate_resource_directory(path: &Path) -> anyhow::Result<()> {
    let mut file_count = 0_usize;
    let mut total_size = 0_u64;
    let mut pending = vec![(path.to_path_buf(), 0_usize)];
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_RESOURCE_DEPTH {
            bail!(
                "Plugin resource directory exceeds {MAX_RESOURCE_DEPTH} levels: {}",
                directory.display()
            );
        }
        let mut entries = read_entries(&directory)?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let entry_path = entry.path();
            let name = utf8_name(&entry_path, &entry.file_name())?;
            if is_ignored_os_metadata(&name) {
                continue;
            }
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                pending.push((entry_path, depth + 1));
                continue;
            }
            if !file_type.is_file() {
                bail!(
                    "Plugin resources cannot contain symlinks or special files: {}",
                    entry_path.display()
                );
            }
            if file_count == MAX_RESOURCE_FILES {
                bail!(
                    "Plugin resources exceed {MAX_RESOURCE_FILES} files: {}",
                    path.display()
                );
            }
            let metadata = fs::symlink_metadata(&entry_path)?;
            if metadata.file_type().is_symlink() || !metadata.file_type().is_file() {
                bail!(
                    "Plugin resources must be regular files: {}",
                    entry_path.display()
                );
            }
            if metadata.len() > MAX_RESOURCE_FILE_BYTES {
                bail!("Plugin resource exceeds 1 MiB: {}", entry_path.display());
            }
            let bytes = fs::read(&entry_path)?;
            let byte_count = u64::try_from(bytes.len()).with_context(|| {
                format!("Plugin resource is too large: {}", entry_path.display())
            })?;
            if byte_count > MAX_RESOURCE_FILE_BYTES {
                bail!("Plugin resource exceeds 1 MiB: {}", entry_path.display());
            }
            total_size = total_size
                .checked_add(byte_count)
                .with_context(|| format!("Plugin resource size overflow: {}", path.display()))?;
            if total_size > MAX_RESOURCE_TOTAL_BYTES {
                bail!("Plugin resources exceed 16 MiB: {}", path.display());
            }
            file_count += 1;
        }
    }
    Ok(())
}

fn is_ignored_os_metadata(name: &str) -> bool {
    name == ".DS_Store"
}

fn read_bundle_descriptor(path: &Path, plugin_id: &str) -> anyhow::Result<PluginDescriptor> {
    let verified = verify_bundle_directory(path)
        .with_context(|| format!("verify Plugin Bundle {}", path.display()))?;
    if verified.plugin_id != plugin_id {
        bail!(
            "Plugin Bundle ID `{}` does not match directory `{plugin_id}`",
            verified.plugin_id
        );
    }
    let manifest = read_bundle_manifest(path)
        .with_context(|| format!("read Plugin Manifest {}", path.display()))?;
    let descriptor = resolve_implementation(
        &manifest,
        &ImplementationPolicy {
            host_target: format!("{}-unknown-{}", env::consts::ARCH, env::consts::OS),
            execution_classes: vec![
                ExecutionClassId::new("lenso.quickjs@1"),
                ExecutionClassId::new("lenso.process@1"),
                ExecutionClassId::new("lenso.wasm-component@1"),
                ExecutionClassId::new("lenso.bun-process@1"),
            ],
        },
    )?
    .descriptor;
    if descriptor.plugin_id() != plugin_id
        || descriptor.release_version() != verified.release_version
    {
        bail!("Plugin Descriptor identity does not match the verified Bundle");
    }
    Ok(descriptor)
}

fn read_configuration(path: &Path) -> anyhow::Result<serde_json::Value> {
    let metadata = fs::metadata(path)?;
    if metadata.len() > MAX_CONFIGURATION_BYTES {
        bail!("Plugin configuration exceeds 256 KiB: {}", path.display());
    }
    let text = fs::read_to_string(path)
        .with_context(|| format!("read Plugin configuration {}", path.display()))?;
    let table: toml::Table = toml::from_str(&text)
        .with_context(|| format!("parse Plugin configuration {}", path.display()))?;
    serde_json::to_value(table).context("convert Plugin configuration to portable values")
}

fn read_entries(path: &Path) -> anyhow::Result<Vec<fs::DirEntry>> {
    fs::read_dir(path)
        .with_context(|| format!("read directory {}", path.display()))?
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("read directory entries {}", path.display()))
}

fn utf8_name(path: &Path, name: &std::ffi::OsStr) -> anyhow::Result<String> {
    name.to_str()
        .map(str::to_owned)
        .with_context(|| format!("Plugin path is not UTF-8: {}", path.display()))
}

fn validate_instance_filename(instance: &str) -> anyhow::Result<()> {
    validate_path_identity(instance, "Instance key")?;
    if instance.starts_with('.') || instance == "plugin" {
        bail!("reserved Plugin Instance key `{instance}`");
    }
    Ok(())
}

fn validate_path_identity(value: &str, label: &str) -> anyhow::Result<()> {
    if value.trim() != value
        || value.is_empty()
        || value == "."
        || value == ".."
        || value.contains(['/', '\0', '\\'])
    {
        bail!("invalid {label} `{value}`");
    }
    Ok(())
}

fn reject_case_collision(
    normalized: &mut BTreeMap<String, String>,
    value: &str,
    label: &str,
) -> anyhow::Result<()> {
    let key = value.to_lowercase();
    if let Some(previous) = normalized.insert(key, value.to_owned())
        && previous != value
    {
        bail!("case-colliding {label}s `{previous}` and `{value}`");
    }
    Ok(())
}

/// Adds one verified external Plugin Bundle after resolving the complete candidate App.
pub fn add_bundle(root: &Path, bundle: &Path) -> anyhow::Result<(String, String, ResolvedApp)> {
    let verified = verify_bundle_directory(bundle)
        .with_context(|| format!("verify Plugin Bundle {}", bundle.display()))?;
    let descriptor = read_bundle_descriptor(bundle, &verified.plugin_id)?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    if current
        .releases()
        .iter()
        .any(|release| release.plugin_id() == verified.plugin_id)
    {
        bail!("Plugin `{}` already has a root Bundle", verified.plugin_id);
    }
    let candidate = PluginRootSnapshot::new(
        current.releases().iter().cloned().chain([descriptor]),
        current.instances().iter().cloned(),
        current.disabled().iter().cloned(),
    );
    let resolved = resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let plugin_directory = root.join(PLUGIN_ROOT).join(&verified.plugin_id);
    fs::create_dir_all(&plugin_directory)?;
    copy_bundle(bundle, &plugin_directory.join(BUNDLE_NAME))?;
    Ok((
        verified.plugin_id,
        verified.release_version.clone(),
        resolved,
    ))
}

/// Atomically writes one typed Instance patch after resolving the complete candidate App.
pub fn configure_instance(
    root: &Path,
    plugin_id: &str,
    instance: &str,
    bytes: &[u8],
) -> anyhow::Result<ResolvedApp> {
    validate_path_identity(plugin_id, "Plugin ID")?;
    validate_instance_filename(instance)?;
    let temporary = tempfile::NamedTempFile::new()?;
    fs::write(temporary.path(), bytes)?;
    let configuration = read_configuration(temporary.path())?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    let id = PluginInstanceId::new(plugin_id, instance);
    let mut instances = current
        .instances()
        .iter()
        .filter(|instance| instance.id() != &id)
        .cloned()
        .collect::<Vec<_>>();
    instances.push(PluginRootInstance::new(plugin_id, instance).with_configuration(configuration));
    let candidate = PluginRootSnapshot::new(
        current.releases().iter().cloned(),
        instances,
        current.disabled().iter().cloned(),
    );
    let resolved = resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let path = root
        .join(PLUGIN_ROOT)
        .join(plugin_id)
        .join(format!("{instance}.toml"));
    atomic_write(&path, bytes)?;
    Ok(resolved)
}

/// Atomically changes one Instance selection marker after candidate resolution.
pub fn set_instance_disabled(
    root: &Path,
    plugin_id: &str,
    instance: &str,
    disabled_state: bool,
) -> anyhow::Result<ResolvedApp> {
    validate_path_identity(plugin_id, "Plugin ID")?;
    validate_instance_filename(instance)?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    let id = PluginInstanceId::new(plugin_id, instance);
    let mut disabled = current.disabled().iter().cloned().collect::<BTreeSet<_>>();
    if disabled_state {
        disabled.insert(id.clone());
    } else if !disabled.remove(&id) {
        bail!("Plugin Instance `{id}` is not disabled");
    }
    let candidate = PluginRootSnapshot::new(
        current.releases().iter().cloned(),
        current.instances().iter().cloned(),
        disabled,
    );
    let resolved = resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let marker = root
        .join(PLUGIN_ROOT)
        .join(plugin_id)
        .join(format!("{instance}.disabled"));
    if disabled_state {
        atomic_write(&marker, &[])?;
    } else {
        fs::remove_file(&marker)
            .with_context(|| format!("remove disabled marker {}", marker.display()))?;
    }
    Ok(resolved)
}

/// Removes one App-owned Instance difference after validating the remaining App.
pub fn remove_instance_difference(
    root: &Path,
    plugin_id: &str,
    instance: &str,
) -> anyhow::Result<ResolvedApp> {
    validate_path_identity(plugin_id, "Plugin ID")?;
    validate_instance_filename(instance)?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    let id = PluginInstanceId::new(plugin_id, instance);
    let candidate = PluginRootSnapshot::new(
        current.releases().iter().cloned(),
        current
            .instances()
            .iter()
            .filter(|item| item.id() != &id)
            .cloned(),
        current
            .disabled()
            .iter()
            .filter(|item| *item != &id)
            .cloned(),
    );
    let resolved = resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let plugin_directory = root.join(PLUGIN_ROOT).join(plugin_id);
    remove_if_exists(&plugin_directory.join(format!("{instance}.toml")))?;
    remove_if_exists(&plugin_directory.join(format!("{instance}.disabled")))?;
    Ok(resolved)
}

/// Moves one root-supplied Plugin to recoverable trash after validating the remaining App.
pub fn remove_plugin(root: &Path, plugin_id: &str) -> anyhow::Result<(ResolvedApp, PathBuf)> {
    validate_path_identity(plugin_id, "Plugin ID")?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    let candidate = PluginRootSnapshot::new(
        current
            .releases()
            .iter()
            .filter(|release| release.plugin_id() != plugin_id)
            .cloned(),
        current
            .instances()
            .iter()
            .filter(|instance| instance.id().plugin_id() != plugin_id)
            .cloned(),
        current
            .disabled()
            .iter()
            .filter(|instance| instance.plugin_id() != plugin_id)
            .cloned(),
    );
    let resolved = resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let plugin_directory = root.join(PLUGIN_ROOT).join(plugin_id);
    if !plugin_directory.exists() {
        bail!("Plugin `{plugin_id}` has no Plugin Root directory");
    }
    let trash = root
        .join(".lenso/trash")
        .join(format!("{plugin_id}-{}", uuid::Uuid::now_v7()));
    fs::create_dir_all(trash.parent().expect("trash has a parent"))?;
    fs::rename(&plugin_directory, &trash)?;
    Ok((resolved, trash))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().context("Plugin file has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    fs::write(temporary.path(), bytes)?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("commit Plugin file {}", path.display()))?;
    Ok(())
}

fn copy_bundle(source: &Path, destination: &Path) -> anyhow::Result<()> {
    if destination.exists() {
        bail!("Plugin Bundle already exists: {}", destination.display());
    }
    let parent = destination
        .parent()
        .context("Bundle destination has no parent")?;
    let staging = tempfile::Builder::new()
        .prefix(".plugin-bundle-")
        .tempdir_in(parent)?;
    copy_directory(source, staging.path())?;
    let staging_path = staging.keep();
    fs::rename(&staging_path, destination)
        .with_context(|| format!("commit Plugin Bundle {}", destination.display()))?;
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    for entry in read_entries(source)? {
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let child = destination.join(entry.file_name());
            fs::create_dir_all(&child)?;
            copy_directory(&entry.path(), &child)?;
            continue;
        }
        if !file_type.is_file() {
            bail!(
                "Plugin Bundle contains a non-file entry: {}",
                entry.path().display()
            );
        }
        fs::copy(entry.path(), destination.join(entry.file_name()))?;
    }
    Ok(())
}

fn remove_if_exists(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lenso_app_plan::authoring::{HostDefaultPlugin, HostPluginRelease, HostSlot};

    fn fixture_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".lenso")).unwrap();
        let host = HostCatalog::new(
            [HostSlot::one("agent")],
            [HostPluginRelease::new(PluginDescriptor::new(
                "example.agent",
                "1.0.0",
                "agent",
            ))],
            [HostDefaultPlugin::new("example.agent", "default")],
        );
        fs::write(
            root.path().join(HOST_CATALOG),
            serde_json::to_vec(&host).unwrap(),
        )
        .unwrap();
        root
    }

    #[test]
    fn missing_plugin_root_resolves_the_host_default_app() {
        let root = fixture_root();
        let resolved = load_resolved_app(root.path()).unwrap();

        assert_eq!(resolved.instances().len(), 1);
        assert_eq!(
            resolved.instances()[0].id().to_string(),
            "example.agent/default"
        );
    }

    #[test]
    fn macos_metadata_at_plugin_root_is_ignored() {
        let root = fixture_root();
        fs::create_dir(root.path().join("plugins")).unwrap();
        fs::write(root.path().join("plugins/.DS_Store"), b"Finder metadata").unwrap();

        let resolved = load_resolved_app(root.path()).unwrap();

        assert_eq!(resolved.instances().len(), 1);
    }

    #[test]
    fn macos_metadata_inside_plugin_directory_is_ignored() {
        let root = fixture_root();
        let plugin = root.path().join("plugins/example.agent");
        fs::create_dir_all(&plugin).unwrap();
        fs::write(plugin.join(".DS_Store"), b"Finder metadata").unwrap();

        let resolved = load_resolved_app(root.path()).unwrap();

        assert_eq!(resolved.instances().len(), 1);
    }

    #[test]
    fn accepts_a_bounded_resource_directory_paired_with_an_instance() {
        let root = fixture_root();
        let plugin = root.path().join("plugins/example.agent");
        fs::create_dir_all(plugin.join("default/prompts")).unwrap();
        fs::write(plugin.join("default.toml"), "").unwrap();
        fs::write(plugin.join("default/prompts/system.md"), "hello").unwrap();
        fs::write(plugin.join("default/prompts/.DS_Store"), "metadata").unwrap();

        let resolved = load_resolved_app(root.path()).unwrap();

        assert!(
            resolved
                .instances()
                .iter()
                .any(|instance| instance.id().to_string() == "example.agent/default")
        );
    }

    #[test]
    fn rejects_an_orphan_resource_directory() {
        let root = fixture_root();
        let resources = root.path().join("plugins/example.agent/custom");
        fs::create_dir_all(&resources).unwrap();
        fs::write(resources.join("prompt.md"), "orphan").unwrap();

        let error = load_resolved_app(root.path()).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("orphan Plugin resource directory")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_resource_symlink() {
        use std::os::unix::fs::symlink;

        let root = fixture_root();
        let plugin = root.path().join("plugins/example.agent");
        fs::create_dir_all(plugin.join("custom")).unwrap();
        fs::write(plugin.join("custom.toml"), "").unwrap();
        fs::write(root.path().join("secret"), "not admitted").unwrap();
        symlink(root.path().join("secret"), plugin.join("custom/secret")).unwrap();

        let error = load_resolved_app(root.path()).unwrap_err();

        assert!(error.to_string().contains("cannot contain symlinks"));
    }

    #[test]
    fn failed_configuration_candidate_does_not_write_the_plugin_root() {
        let root = fixture_root();

        let error = configure_instance(
            root.path(),
            "example.agent",
            "default",
            b"unexpected = true\n",
        )
        .unwrap_err();

        assert!(error.to_string().contains("non-empty configuration"));
        assert!(
            !root
                .path()
                .join("plugins/example.agent/default.toml")
                .exists()
        );
    }

    #[test]
    fn required_default_disable_fails_before_writing_a_marker() {
        let root = fixture_root();

        let error =
            set_instance_disabled(root.path(), "example.agent", "default", true).unwrap_err();

        assert!(error.to_string().contains("cannot be disabled"));
        assert!(
            !root
                .path()
                .join("plugins/example.agent/default.disabled")
                .exists()
        );
    }

    #[test]
    fn case_colliding_plugin_identities_fail_closed() {
        let mut normalized = BTreeMap::new();
        reject_case_collision(&mut normalized, "Example.Agent", "Plugin ID").unwrap();
        let error =
            reject_case_collision(&mut normalized, "example.agent", "Plugin ID").unwrap_err();

        assert!(error.to_string().contains("case-colliding Plugin IDs"));
    }
}
