use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
pub(crate) use lenso_app_authoring::load_resolved_app;
use lenso_app_authoring::{
    BundleMutation, add_bundle, configure_instance, prepare_bundle_mutation,
    remove_instance_difference, remove_plugin, set_instance_disabled,
};
use lenso_app_plan::authoring::PluginInstanceId;
use lenso_plugin_bundle::verify_bundle_directory;

use crate::archive::with_bundle_directory;
use crate::{
    archive::archive_bundle,
    catalog::{
        CatalogPluginRelease, DEFAULT_CATALOG_URL, download_bundle, fetch_catalog_release,
        search_catalog,
    },
};
use lenso_app_authoring::identity::{validate_plugin_id_v1, validate_release_version};

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
    let mut releases = search_catalog(&args.catalog, &query)?
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
    validate_plugin_id_v1(&args.plugin_id)?;
    validate_release_version(&args.version)?;
    let root = project_root(args.root)?;
    let matches = fetch_catalog_release(&args.catalog, &args.plugin_id, &args.version)?
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
    let mutation = if replace {
        BundleMutation::Replace
    } else {
        BundleMutation::Add
    };
    let prepared = prepare_bundle_mutation(root, bundle, mutation)?;
    let verified = prepared.verified();
    if verified.plugin_id != release.plugin_id || verified.release_version != release.version {
        bail!("catalog identity does not match the verified Plugin Bundle");
    }
    if verified.manifest_digest != release.manifest_digest {
        bail!("catalog manifest digest does not match the verified Plugin Bundle");
    }
    if replace {
        retain_current_bundle(root, &release.plugin_id, prepared.destination())?;
    }
    retain_archive(root, &release.plugin_id, &release.version, archive)?;
    prepared.commit()?;
    Ok(())
}

fn history(args: HistoryArgs) -> anyhow::Result<()> {
    validate_existing_cli_plugin_id(&args.plugin_id)?;
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
    validate_existing_cli_plugin_id(&args.plugin_id)?;
    validate_release_version(&args.version)?;
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
        let prepared = prepare_bundle_mutation(&root, bundle, BundleMutation::Restore)?;
        let verified = prepared.verified();
        if verified.plugin_id != args.plugin_id || verified.release_version != args.version {
            bail!("retained Plugin Release identity is invalid");
        }
        retain_current_bundle(&root, &args.plugin_id, prepared.destination())?;
        prepared.commit()?;
        Ok(())
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

fn validate_existing_cli_plugin_id(plugin_id: &str) -> anyhow::Result<()> {
    lenso_app_authoring::identity::classify_existing_plugin_id(plugin_id).map(|_| ())
}
