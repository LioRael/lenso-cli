use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
pub(crate) use lenso_app_authoring::load_resolved_app;
use lenso_app_authoring::{
    add_bundle, configure_instance, remove_instance_difference, remove_plugin,
    set_instance_disabled,
};
use lenso_app_plan::ExecutionClassId;
use lenso_app_plan::authoring::{
    HostCatalog, PluginDescriptor, PluginInstanceId, PluginRootInstance, PluginRootSnapshot,
    resolve_plugin_root,
};
use lenso_plugin_bundle::{
    ImplementationPolicy, read_bundle_manifest, resolve_implementation, verify_bundle_directory,
};

use crate::archive::with_bundle_directory;
use crate::{
    archive::archive_bundle,
    catalog::{CatalogPluginRelease, DEFAULT_CATALOG_URL, download_bundle, fetch_catalog},
};

const PLUGIN_ROOT: &str = "plugins";
const HOST_CATALOG: &str = ".lenso/host-catalog.json";
const BUNDLE_NAME: &str = "plugin.lenso-plugin";
const MAX_CONFIGURATION_BYTES: u64 = 256 * 1024;
const MAX_RESOURCE_FILES: usize = 4_096;
const MAX_RESOURCE_FILE_BYTES: u64 = 1024 * 1024;
const MAX_RESOURCE_TOTAL_BYTES: u64 = 16 * 1024 * 1024;
const MAX_RESOURCE_DEPTH: usize = 32;

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
    /// Search immutable Plugin Releases in a catalog.
    Search(SearchArgs),
    /// Install one exact catalog Release.
    Install(CatalogMutationArgs),
    /// Replace a root Bundle with one exact catalog Release.
    Update(CatalogMutationArgs),
    /// List retained exact Releases available for rollback.
    History(HistoryArgs),
    /// Restore one retained exact Release.
    Rollback(RollbackArgs),
}

#[derive(Clone, Debug, Args)]
pub(crate) struct ProjectArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct AddArgs {
    /// Exact `.lenso-plugin` Bundle archive or legacy Bundle directory.
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

#[derive(Clone, Debug, Args)]
pub(crate) struct SearchArgs {
    /// Text matched against Plugin ID, summary, and Capability IDs.
    query: Option<String>,
    /// Plugin catalog endpoint.
    #[arg(long, default_value = DEFAULT_CATALOG_URL)]
    catalog: String,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct CatalogMutationArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// Exact immutable Release version; no implicit latest selection is performed.
    #[arg(long)]
    version: String,
    /// Plugin catalog endpoint.
    #[arg(long, default_value = DEFAULT_CATALOG_URL)]
    catalog: String,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct HistoryArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Debug, Args)]
pub(crate) struct RollbackArgs {
    /// Exact Plugin ID.
    plugin_id: String,
    /// Exact retained Release version.
    #[arg(long)]
    version: String,
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
        PluginsCommand::Search(args) => search(args),
        PluginsCommand::Install(args) => install(args, false),
        PluginsCommand::Update(args) => install(args, true),
        PluginsCommand::History(args) => history(args),
        PluginsCommand::Rollback(args) => rollback(args),
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

fn list(args: ProjectArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let resolved = load_resolved_app(&root)?;
    if args.json {
        let instances = resolved
            .instances()
            .iter()
            .map(|instance| {
                serde_json::json!({
                    "id": instance.id().to_string(),
                    "source": format!("{:?}", instance.source()),
                    "plan_key": instance.plan_key(),
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.plugins-list",
                "instances": instances,
            }))?
        );
    } else {
        for instance in resolved.instances() {
            println!("{}\t{:?}", instance.id(), instance.source());
        }
    }
    Ok(())
}

fn add(args: AddArgs) -> anyhow::Result<()> {
    let bundle = args.bundle.clone();
    with_bundle_directory(&bundle, |directory| add_from_directory(args, directory))
}

fn add_from_directory(args: AddArgs, bundle: &Path) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let (plugin_id, release_version, _) = add_bundle(&root, bundle)?;
    println!("Added Plugin `{plugin_id}` {release_version}.");
    Ok(())
}

fn configure(args: ConfigureArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let bytes = args.file.map_or_else(
        || Ok(Vec::new()),
        |path| fs::read(&path).with_context(|| format!("read {}", path.display())),
    )?;
    configure_instance(&root, &args.plugin_id, &args.instance, &bytes)?;
    let id = PluginInstanceId::new(&args.plugin_id, &args.instance);
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
    let root = project_root(args.root)?;
    set_instance_disabled(&root, &args.plugin_id, &args.instance, disabled_state)?;
    let id = PluginInstanceId::new(&args.plugin_id, &args.instance);
    if disabled_state {
        println!("Disabled Plugin Instance `{id}`.");
    } else {
        println!("Enabled Plugin Instance `{id}`.");
    }
    Ok(())
}

fn remove(args: RemoveArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    if let Some(instance) = args.instance {
        remove_instance_difference(&root, &args.plugin_id, &instance)?;
        println!(
            "Removed Plugin Instance difference `{}/{instance}`.",
            args.plugin_id
        );
    } else {
        let (_, trash) = remove_plugin(&root, &args.plugin_id)?;
        println!(
            "Removed Plugin `{}`; recoverable at {}.",
            args.plugin_id,
            trash.display()
        );
    }
    Ok(())
}

fn search(args: SearchArgs) -> anyhow::Result<()> {
    let query = args.query.unwrap_or_default().to_lowercase();
    let mut releases = fetch_catalog(&args.catalog)?
        .into_iter()
        .filter(|release| {
            query.is_empty()
                || release.plugin_id.to_lowercase().contains(&query)
                || release.summary.to_lowercase().contains(&query)
                || release
                    .capabilities
                    .iter()
                    .any(|capability| capability.to_lowercase().contains(&query))
        })
        .collect::<Vec<_>>();
    releases.sort_by(|left, right| {
        (&left.plugin_id, &left.version).cmp(&(&right.plugin_id, &right.version))
    });
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.plugins-search",
                "catalog": args.catalog,
                "releases": releases.iter().map(catalog_release_json).collect::<Vec<_>>(),
            }))?
        );
    } else if releases.is_empty() {
        println!("No installable Plugin Releases matched.");
    } else {
        for release in releases {
            println!(
                "{}@{}\t{}",
                release.plugin_id, release.version, release.summary
            );
        }
    }
    Ok(())
}

fn install(args: CatalogMutationArgs, replace: bool) -> anyhow::Result<()> {
    validate_path_identity(&args.plugin_id, "Plugin ID")?;
    validate_path_identity(&args.version, "Release version")?;
    let root = project_root(args.root)?;
    let matches = fetch_catalog(&args.catalog)?
        .into_iter()
        .filter(|release| release.plugin_id == args.plugin_id && release.version == args.version)
        .collect::<Vec<_>>();
    let [release] = matches.as_slice() else {
        bail!(
            "catalog must contain exactly one `{}` Release at version `{}`; found {}",
            args.plugin_id,
            args.version,
            matches.len()
        );
    };
    let temporary = tempfile::NamedTempFile::new().context("stage catalog Plugin Bundle")?;
    download_bundle(release, temporary.path())?;
    with_bundle_directory(temporary.path(), |bundle| {
        install_catalog_bundle(&root, release, bundle, temporary.path(), replace)
    })?;
    println!(
        "{} Plugin `{}` {}.",
        if replace { "Updated" } else { "Installed" },
        release.plugin_id,
        release.version
    );
    Ok(())
}

fn install_catalog_bundle(
    root: &Path,
    release: &CatalogPluginRelease,
    bundle: &Path,
    archive: &Path,
    replace: bool,
) -> anyhow::Result<()> {
    let verified = verify_bundle_directory(bundle)?;
    if verified.plugin_id != release.plugin_id || verified.release_version != release.version {
        bail!("catalog identity does not match the verified Plugin Bundle");
    }
    if verified.manifest_digest != release.manifest_digest {
        bail!("catalog manifest digest does not match the verified Plugin Bundle");
    }
    let descriptor = read_bundle_descriptor(bundle, &release.plugin_id)?;
    let host = load_host_catalog(root)?;
    let current = snapshot_plugin_root(root)?;
    let has_current = current
        .releases()
        .iter()
        .any(|item| item.plugin_id() == release.plugin_id);
    if replace != has_current {
        if replace {
            bail!(
                "Plugin `{}` has no root Bundle to update",
                release.plugin_id
            );
        }
        bail!(
            "Plugin `{}` already has a root Bundle; use `plugins update`",
            release.plugin_id
        );
    }
    let candidate = PluginRootSnapshot::new(
        current
            .releases()
            .iter()
            .filter(|item| item.plugin_id() != release.plugin_id)
            .cloned()
            .chain([descriptor]),
        current.instances().iter().cloned(),
        current.disabled().iter().cloned(),
    );
    resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
    let destination = root
        .join(PLUGIN_ROOT)
        .join(&release.plugin_id)
        .join(BUNDLE_NAME);
    if replace {
        retain_current_bundle(root, &release.plugin_id, &destination)?;
    }
    retain_archive(root, &release.plugin_id, &release.version, archive)?;
    replace_bundle_directory(bundle, &destination, replace)
}

fn history(args: HistoryArgs) -> anyhow::Result<()> {
    validate_path_identity(&args.plugin_id, "Plugin ID")?;
    let root = project_root(args.root)?;
    let directory = plugin_store(&root, &args.plugin_id);
    let mut versions = match fs::read_dir(&directory) {
        Ok(entries) => entries
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|entry| {
                entry
                    .path()
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(str::to_owned)
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    versions.sort();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.plugins-history",
                "plugin_id": args.plugin_id,
                "versions": versions,
            }))?
        );
    } else if versions.is_empty() {
        println!("No retained Releases for `{}`.", args.plugin_id);
    } else {
        for version in versions {
            println!("{}@{}", args.plugin_id, version);
        }
    }
    Ok(())
}

fn rollback(args: RollbackArgs) -> anyhow::Result<()> {
    validate_path_identity(&args.plugin_id, "Plugin ID")?;
    validate_path_identity(&args.version, "Release version")?;
    let root = project_root(args.root)?;
    let archive =
        plugin_store(&root, &args.plugin_id).join(format!("{}.lenso-plugin", args.version));
    if !archive.is_file() {
        bail!(
            "retained Plugin Release is unavailable: {}",
            archive.display()
        );
    }
    with_bundle_directory(&archive, |bundle| {
        let verified = verify_bundle_directory(bundle)?;
        if verified.plugin_id != args.plugin_id || verified.release_version != args.version {
            bail!("retained Plugin Release identity is invalid");
        }
        let descriptor = read_bundle_descriptor(bundle, &args.plugin_id)?;
        let host = load_host_catalog(&root)?;
        let current = snapshot_plugin_root(&root)?;
        let candidate = PluginRootSnapshot::new(
            current
                .releases()
                .iter()
                .filter(|item| item.plugin_id() != args.plugin_id)
                .cloned()
                .chain([descriptor]),
            current.instances().iter().cloned(),
            current.disabled().iter().cloned(),
        );
        resolve_plugin_root(&host, &candidate).map_err(anyhow::Error::msg)?;
        let destination = root
            .join(PLUGIN_ROOT)
            .join(&args.plugin_id)
            .join(BUNDLE_NAME);
        retain_current_bundle(&root, &args.plugin_id, &destination)?;
        replace_bundle_directory(bundle, &destination, true)
    })?;
    println!(
        "Rolled back Plugin `{}` to {}.",
        args.plugin_id, args.version
    );
    Ok(())
}

fn catalog_release_json(release: &CatalogPluginRelease) -> serde_json::Value {
    serde_json::json!({
        "plugin_id": release.plugin_id,
        "version": release.version,
        "summary": release.summary,
        "bundle_url": release.bundle_url,
        "bundle_digest": release.bundle_digest,
        "manifest_digest": release.manifest_digest,
        "host_targets": release.host_targets,
        "execution_classes": release.execution_classes,
        "capabilities": release.capabilities,
    })
}

fn plugin_store(root: &Path, plugin_id: &str) -> PathBuf {
    root.join(".lenso/plugin-store").join(plugin_id)
}

fn retain_archive(
    root: &Path,
    plugin_id: &str,
    version: &str,
    archive: &Path,
) -> anyhow::Result<()> {
    let destination = plugin_store(root, plugin_id).join(format!("{version}.lenso-plugin"));
    fs::create_dir_all(destination.parent().expect("store path has a parent"))?;
    if destination.exists() {
        let current = fs::read(&destination)?;
        let candidate = fs::read(archive)?;
        if current != candidate {
            bail!("retained Plugin Release `{plugin_id}@{version}` has different immutable bytes");
        }
        return Ok(());
    }
    fs::copy(archive, &destination)?;
    Ok(())
}

fn retain_current_bundle(root: &Path, plugin_id: &str, bundle: &Path) -> anyhow::Result<()> {
    if !bundle.is_dir() {
        bail!("current Plugin Bundle is unavailable: {}", bundle.display());
    }
    let verified = verify_bundle_directory(bundle)?;
    let staging = tempfile::tempdir().context("stage retained Plugin Release")?;
    let archive = staging.path().join("current.lenso-plugin");
    archive_bundle(bundle, &archive)?;
    retain_archive(root, plugin_id, &verified.release_version, &archive)
}

fn replace_bundle_directory(
    source: &Path,
    destination: &Path,
    replace: bool,
) -> anyhow::Result<()> {
    let parent = destination
        .parent()
        .context("Bundle destination has no parent")?;
    fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new()
        .prefix(".plugin-bundle-")
        .tempdir_in(parent)?;
    copy_directory(source, staging.path())?;
    let staging_path = staging.keep();
    if !replace {
        return fs::rename(&staging_path, destination)
            .with_context(|| format!("commit Plugin Bundle {}", destination.display()));
    }
    let backup = parent.join(format!(".plugin-bundle-backup-{}", uuid::Uuid::now_v7()));
    fs::rename(destination, &backup)?;
    if let Err(error) = fs::rename(&staging_path, destination) {
        let _ = fs::rename(&backup, destination);
        return Err(error)
            .with_context(|| format!("replace Plugin Bundle {}", destination.display()));
    }
    fs::remove_dir_all(backup)?;
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
