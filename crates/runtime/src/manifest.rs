use std::collections::BTreeSet;
use std::path::Path;

use lenso_cli::{Cli, CommandSpec, Error, Workflow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::LoadOptions;
use crate::project::{Project, ProjectLock, read_project, resolution_guard, resolve, save_lock};
use crate::{ProcessSpec, WasmSpec};

/// Static extension metadata and executable adapter declarations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extension {
    /// Extension JSON ABI/schema version.
    pub schema_version: u32,
    /// Provider namespace, used to reject misleading command ownership.
    pub id: String,
    /// Human-readable installed version.
    pub version: String,
    /// Handler identifiers owned by this extension namespace.
    pub handlers: BTreeMap<String, HandlerSpec>,
    /// Workflow definitions, using registered native or extension handlers.
    pub workflows: Vec<Workflow>,
    /// Statically discoverable command aliases.
    pub commands: Vec<CommandSpec>,
}

/// Runtime adapters available to explicitly installed extensions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HandlerSpec {
    /// Restricted local Extism export.
    Wasm(WasmSpec),
    /// Exact invocation requiring a matching installation grant.
    Process(ProcessSpec),
}

pub(crate) fn validate_metadata(extension: &Extension) -> Result<(), Error> {
    if extension.schema_version != 1 || extension.id.is_empty() || extension.version.is_empty() {
        return Err(Error::Invalid(
            "unsupported or empty extension identity".into(),
        ));
    }
    let namespace = format!("{}/", extension.id);
    for id in extension
        .handlers
        .keys()
        .chain(extension.workflows.iter().map(|workflow| &workflow.id))
    {
        if !id.starts_with(&namespace) {
            return Err(Error::Invalid(format!(
                "identifier {id} is outside {}",
                extension.id
            )));
        }
    }
    for command in &extension.commands {
        if command.provider != extension.id || !command.workflow.starts_with(&namespace) {
            return Err(Error::Invalid(
                "command provider or workflow ownership mismatch".into(),
            ));
        }
    }
    Ok(())
}

/// Load command metadata, automatically preparing a missing project lock.
///
/// Use a blocking worker when remote sources are possible; the async CLI bootstrap
/// helper [`Cli::prepare`] provides progress without blocking the terminal event loop.
///
/// # Errors
/// Rejects invalid sources, definitions, or conflicts with this host's commands.
pub fn load_project(cli: &mut Cli, project: &Path) -> Result<(), Error> {
    load_project_with_options(cli, project, &LoadOptions::default())
}

/// Load declarations under explicit offline/locked/discovery and cache policies.
///
/// # Errors
/// Rejects unavailable dependencies, lock-policy violations, or invalid definitions.
pub fn load_project_with_options(
    cli: &mut Cli,
    project: &Path,
    options: &LoadOptions,
) -> Result<(), Error> {
    let project = project.canonicalize()?;
    let Some(config) = read_project(&project)? else {
        return Ok(());
    };
    let _guard = resolution_guard(&project, &config, options)?;
    let lock = resolve(&project, &config, options)?;
    let mut next = cli.clone();
    register_locked(&mut next, &project, &config, &lock, options)?;
    save_lock(&project, &lock, options)?;
    *cli = next;
    Ok(())
}

pub(crate) fn register_locked(
    cli: &mut Cli,
    project: &Path,
    config: &Project,
    lock: &ProjectLock,
    options: &LoadOptions,
) -> Result<(), Error> {
    let mut next = cli.clone();
    let mut extensions = Vec::new();
    let mut ids = BTreeSet::new();
    for installed in config.cli.extensions.iter().filter(|entry| entry.enabled) {
        let locked = lock
            .extensions
            .iter()
            .find(|entry| entry.id == installed.id && entry.source == installed.source)
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{} is not resolved in lenso.lock; run lenso update",
                    installed.id
                ))
            })?;
        let extension = &locked.definition;
        validate_metadata(extension)?;
        if !ids.insert(extension.id.clone()) {
            return Err(Error::Invalid("duplicate extension identity".into()));
        }
        for (id, handler) in &extension.handlers {
            match handler {
                HandlerSpec::Wasm(spec) => {
                    let spec = spec.clone();
                    let project = project.to_owned();
                    let locked = locked.clone();
                    let mut options = options.clone();
                    options.github = config.cli.github.clone();
                    let credentials = installed.credentials.clone();
                    let identity =
                        serde_json::json!({"id":extension.id,"version":extension.version});
                    next.engine_mut().handler(id, move |ctx| {
                        let spec = spec.clone();
                        let project = project.clone();
                        let locked = locked.clone();
                        let credentials = credentials.clone();
                        let identity = identity.clone();
                        let mut options = options.clone();
                        async move {
                            options.events = ctx.events.clone();
                            let module_name = spec.module.clone();
                            let source_options = options.clone();
                            let module = tokio::task::spawn_blocking(move || {
                                locked.module(&project, &module_name, &options)
                            })
                            .await
                            .map_err(|e| Error::Operation(e.to_string()))??;
                            spec.run_cached(&module, ctx, identity, credentials, source_options)
                                .await
                        }
                    })?;
                }
                HandlerSpec::Process(spec) => {
                    let spec = spec.clone();
                    let granted = installed.processes.contains(&spec);
                    next.engine_mut().handler(id, move |ctx| {
                        let spec = spec.clone();
                        async move {
                            if !granted {
                                return Err(Error::Permission(format!(
                                    "process {} {:?}",
                                    spec.program, spec.args
                                )));
                            }
                            spec.run(ctx).await
                        }
                    })?;
                }
            }
        }
        extensions.push((installed, extension));
    }
    for (installed, extension) in &extensions {
        for workflow in &extension.workflows {
            next.engine_mut().workflow(workflow.clone())?;
            for (name, value) in [
                ("project.files", serde_json::to_value(&installed.files)?),
                ("host.reads", serde_json::to_value(&installed.reads)?),
                ("host.http", serde_json::to_value(&installed.http)?),
                (
                    "host.read_roots",
                    serde_json::to_value(&installed.read_roots)?,
                ),
                (
                    "project.write_roots",
                    serde_json::to_value(&installed.write_roots)?,
                ),
                ("host.env", serde_json::to_value(&installed.env)?),
                ("host.sources", serde_json::to_value(&installed.sources)?),
                (
                    "extension.options",
                    serde_json::to_value(&installed.options)?,
                ),
                ("host.credential_owner", serde_json::json!(installed.id)),
                ("host.discover", serde_json::json!(installed.discover)),
                (
                    "host.cwd",
                    serde_json::json!(std::env::current_dir().ok().and_then(|cwd| {
                        cwd.strip_prefix(project)
                            .ok()
                            .map(|path| path.to_string_lossy().replace('\\', "/"))
                    })),
                ),
                (
                    "host.processes",
                    serde_json::to_value(&installed.processes)?,
                ),
            ] {
                next.engine_mut().grant(&workflow.id, name, value)?;
            }
        }
    }
    for (installed, extension) in extensions {
        for mut command in extension.commands.clone() {
            let original = command.path.join(" ");
            if installed.disabled_commands.contains(&original) {
                continue;
            }
            if let Some(path) = installed.aliases.get(&original) {
                command.path = path.clone();
            }
            next.alias(command)?;
        }
    }
    *cli = next;
    Ok(())
}
