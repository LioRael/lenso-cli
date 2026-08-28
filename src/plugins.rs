use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
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

#[derive(Clone, Debug, Subcommand)]
pub(crate) enum PluginsCommand {
    /// List the Plugin Instances in the derived App.
    List(ProjectArgs),
    /// Add one exact Plugin Bundle after candidate validation.
    Add(AddArgs),
    /// Write direct configuration for one Plugin Instance.
    Configure(ConfigureArgs),
    /// Disable one Plugin Instance without deleting its configuration.
    Disable(InstanceArgs),
    /// Re-enable one disabled Plugin Instance.
    Enable(InstanceArgs),
    /// Remove one Instance difference or an entire root-supplied Plugin.
    Remove(RemoveArgs),
}

#[derive(Clone, Debug, Args)]
pub(crate) struct ProjectArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct AddArgs {
    /// Exact `.lenso-plugin` Bundle directory.
    bundle: PathBuf,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct ConfigureArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// App-local Instance key.
    #[arg(default_value = "default")]
    instance: String,
    /// TOML file to use. Omit to create an empty configuration.
    #[arg(long)]
    file: Option<PathBuf>,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct InstanceArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// App-local Instance key.
    #[arg(default_value = "default")]
    instance: String,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct RemoveArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// Remove only this Instance difference; omit to remove the whole Plugin directory.
    instance: Option<String>,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

pub(crate) fn plugins(command: PluginsCommand) -> anyhow::Result<()> {
    match command {
        PluginsCommand::List(args) => list(args),
        PluginsCommand::Add(args) => add(args),
        PluginsCommand::Configure(args) => configure(args),
        PluginsCommand::Disable(args) => disable(args),
        PluginsCommand::Enable(args) => enable(args),
        PluginsCommand::Remove(args) => remove(args),
    }
}

pub(crate) fn project_root(root: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    root.map_or_else(
        || env::current_dir().context("read current directory"),
        |root| {
            Ok(if root.is_absolute() {
                root
            } else {
                env::current_dir()?.join(root)
            })
        },
    )
}

pub(crate) fn load_resolved_app(root: &Path) -> anyhow::Result<ResolvedApp> {
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
        if !file_type.is_file() {
            bail!(
                "Plugin entries cannot be symlinks or special files: {}",
                entry.path().display()
            );
        }
        if let Some(instance) = name.strip_suffix(".toml") {
            validate_instance_filename(instance)?;
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

fn list(args: ProjectArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let resolved = load_resolved_app(&root)?;
    for instance in resolved.instances() {
        println!("{}\t{:?}", instance.id(), instance.source());
    }
    Ok(())
}

fn add(args: AddArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let verified = verify_bundle_directory(&args.bundle)
        .with_context(|| format!("verify Plugin Bundle {}", args.bundle.display()))?;
    let descriptor = read_bundle_descriptor(&args.bundle, &verified.plugin_id)?;
    let host = load_host_catalog(&root)?;
    let current = snapshot_plugin_root(&root)?;
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
    resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let plugin_directory = root.join(PLUGIN_ROOT).join(&verified.plugin_id);
    fs::create_dir_all(&plugin_directory)?;
    copy_bundle(&args.bundle, &plugin_directory.join(BUNDLE_NAME))?;
    println!(
        "Added Plugin `{}` {}.",
        verified.plugin_id, verified.release_version
    );
    Ok(())
}

fn configure(args: ConfigureArgs) -> anyhow::Result<()> {
    validate_path_identity(&args.plugin_id, "Plugin ID")?;
    validate_instance_filename(&args.instance)?;
    let root = project_root(args.root)?;
    let bytes = args.file.map_or_else(
        || Ok(Vec::new()),
        |path| fs::read(&path).with_context(|| format!("read {}", path.display())),
    )?;
    let temporary = tempfile::NamedTempFile::new()?;
    fs::write(temporary.path(), &bytes)?;
    let configuration = read_configuration(temporary.path())?;
    let host = load_host_catalog(&root)?;
    let current = snapshot_plugin_root(&root)?;
    let id = PluginInstanceId::new(&args.plugin_id, &args.instance);
    let mut instances = current
        .instances()
        .iter()
        .filter(|instance| instance.id() != &id)
        .cloned()
        .collect::<Vec<_>>();
    instances.push(
        PluginRootInstance::new(&args.plugin_id, &args.instance).with_configuration(configuration),
    );
    let candidate = PluginRootSnapshot::new(
        current.releases().iter().cloned(),
        instances,
        current.disabled().iter().cloned(),
    );
    resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let path = root
        .join(PLUGIN_ROOT)
        .join(&args.plugin_id)
        .join(format!("{}.toml", args.instance));
    atomic_write(&path, &bytes)?;
    println!("Configured Plugin Instance `{id}`.");
    Ok(())
}

fn disable(args: InstanceArgs) -> anyhow::Result<()> {
    set_disabled(args, true)
}

fn enable(args: InstanceArgs) -> anyhow::Result<()> {
    set_disabled(args, false)
}

fn set_disabled(args: InstanceArgs, disabled_state: bool) -> anyhow::Result<()> {
    validate_path_identity(&args.plugin_id, "Plugin ID")?;
    validate_instance_filename(&args.instance)?;
    let root = project_root(args.root)?;
    let host = load_host_catalog(&root)?;
    let current = snapshot_plugin_root(&root)?;
    let id = PluginInstanceId::new(&args.plugin_id, &args.instance);
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
    resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let marker = root
        .join(PLUGIN_ROOT)
        .join(&args.plugin_id)
        .join(format!("{}.disabled", args.instance));
    if disabled_state {
        atomic_write(&marker, &[])?;
        println!("Disabled Plugin Instance `{id}`.");
    } else {
        fs::remove_file(&marker)
            .with_context(|| format!("remove disabled marker {}", marker.display()))?;
        println!("Enabled Plugin Instance `{id}`.");
    }
    Ok(())
}

fn remove(args: RemoveArgs) -> anyhow::Result<()> {
    validate_path_identity(&args.plugin_id, "Plugin ID")?;
    let root = project_root(args.root)?;
    let host = load_host_catalog(&root)?;
    let current = snapshot_plugin_root(&root)?;
    let plugin_directory = root.join(PLUGIN_ROOT).join(&args.plugin_id);
    let candidate = if let Some(instance) = &args.instance {
        validate_instance_filename(instance)?;
        let id = PluginInstanceId::new(&args.plugin_id, instance);
        PluginRootSnapshot::new(
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
        )
    } else {
        PluginRootSnapshot::new(
            current
                .releases()
                .iter()
                .filter(|release| release.plugin_id() != args.plugin_id)
                .cloned(),
            current
                .instances()
                .iter()
                .filter(|instance| instance.id().plugin_id() != args.plugin_id)
                .cloned(),
            current
                .disabled()
                .iter()
                .filter(|instance| instance.plugin_id() != args.plugin_id)
                .cloned(),
        )
    };
    resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    if let Some(instance) = args.instance {
        remove_if_exists(&plugin_directory.join(format!("{instance}.toml")))?;
        remove_if_exists(&plugin_directory.join(format!("{instance}.disabled")))?;
        println!(
            "Removed Plugin Instance difference `{}/{instance}`.",
            args.plugin_id
        );
    } else {
        if !plugin_directory.exists() {
            bail!("Plugin `{}` has no Plugin Root directory", args.plugin_id);
        }
        let trash =
            root.join(".lenso/trash")
                .join(format!("{}-{}", args.plugin_id, uuid::Uuid::now_v7()));
        fs::create_dir_all(trash.parent().expect("trash has a parent"))?;
        fs::rename(&plugin_directory, &trash)?;
        println!(
            "Removed Plugin `{}`; recoverable at {}.",
            args.plugin_id,
            trash.display()
        );
    }
    Ok(())
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
    fn failed_configuration_candidate_does_not_write_the_plugin_root() {
        let root = fixture_root();
        let input = root.path().join("invalid.toml");
        fs::write(&input, "unexpected = true\n").unwrap();

        let error = configure(ConfigureArgs {
            plugin_id: "example.agent".to_owned(),
            instance: "default".to_owned(),
            file: Some(input),
            root: Some(root.path().to_path_buf()),
        })
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

        let error = disable(InstanceArgs {
            plugin_id: "example.agent".to_owned(),
            instance: "default".to_owned(),
            root: Some(root.path().to_path_buf()),
        })
        .unwrap_err();

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
