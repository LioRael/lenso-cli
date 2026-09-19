//! Atomic source-to-Host authoring; runtime startup remains a separate operation.
use crate::archive::{archive_bundle, with_bundle_directory};
use anyhow::{Context, bail};
use clap::Args;
use lenso_app_authoring::{
    discovery::{SourceRole, discover},
    host_authoring::{GeneratedHostBuild, LocalPluginInput},
};
use lenso_app_plan::ExecutionClassId;
use lenso_plugin_bundle::{
    ImplementationPolicy, RuntimeAdmission, read_bundle_manifest, resolve_implementation,
    verify_bundle_directory,
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Args)]
pub(crate) struct AssembleArgs {
    /// Source App root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Host identity recorded in the generated build.
    #[arg(long, default_value = "local.app")]
    id: String,
    /// New output directory; existing output is never overwritten.
    #[arg(long)]
    out: PathBuf,
    /// Emit a machine-readable receipt.
    #[arg(long)]
    json: bool,
}

pub(crate) fn assemble(args: AssembleArgs) -> anyhow::Result<()> {
    let root = crate::plugins::project_root(args.root)?;
    let report = discover(&root)?;
    let destination = std::path::absolute(&args.out)?;
    if fs::symlink_metadata(&destination).is_ok() {
        bail!("Host output already exists: {}", destination.display());
    }
    let parent = destination
        .parent()
        .context("Host output needs a parent directory")?;
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".lenso-local-host-")
        .tempdir_in(parent)?;
    fs::create_dir(stage.path().join(".lenso"))?;
    fs::write(stage.path().join(".lenso/plugin-root-authoring.lock"), [])?;
    fs::create_dir(stage.path().join("bundles"))?;
    let root_intent = root.join("plugins");
    if root_intent.try_exists()? {
        copy_root(&root_intent, &stage.path().join("plugins"), 0, &mut 0)?;
    }
    let mut inputs = Vec::new();
    let mut inventory = Vec::new();
    let mut sources = Vec::new();
    for candidate in report.candidates {
        // Shared sources are not built merely because they can be discovered.
        // A Root directory is intent to validate, not implicit enablement.
        if candidate.role == SourceRole::Shared
            && !stage
                .path()
                .join("plugins")
                .join(&candidate.plugin_id)
                .try_exists()?
        {
            continue;
        }
        if candidate
            .implementations
            .iter()
            .any(|item| item.runtime == "native-linked")
        {
            bail!(
                "{}: native-linked Host generation is not implemented by app assemble yet; use the existing Web Plugin dev/custom Host path",
                candidate.project.display()
            );
        }
        if inputs.len() >= 256 {
            bail!("local Host accepts at most 256 selected Plugin sources");
        }
        let archive_path = format!("bundles/{}.lenso-plugin", candidate.plugin_id);
        let archive = stage.path().join(&archive_path);
        if candidate.format == "bundle" {
            with_bundle_directory(&candidate.project, |directory| {
                archive_bundle(directory, &archive)
            })?;
        } else {
            let build = tempfile::tempdir().context("stage local Plugin build")?;
            let bundle = build.path().join("bundle");
            crate::plugin::materialize(
                &candidate.project,
                &bundle,
                crate::plugin::BuildProfile::Release,
            )
            .with_context(|| format!("build local Plugin {}", candidate.plugin_id))?;
            archive_bundle(&bundle, &archive)?;
        }
        let (verified, selected) = with_bundle_directory(&archive, |directory| {
            Ok((
                verify_bundle_directory(directory)?,
                resolve_implementation(
                    &read_bundle_manifest(directory)?,
                    &local_implementation_policy(),
                )?,
            ))
        })?;
        if verified.plugin_id != candidate.plugin_id
            || verified.release_version != candidate.release_version
        {
            bail!(
                "local source identity changed during build: {}",
                candidate.project.display()
            );
        }
        let descriptor = selected.descriptor;
        inventory.push(json!({
            "path": archive_path, "plugin_id": verified.plugin_id,
            "release_version": verified.release_version, "manifest_digest": verified.manifest_digest,
            "execution_class": descriptor.execution_class().as_str(), "runtime_profile": descriptor.runtime_profile(),
            "target": lenso_app_authoring::native_host_target(), "implementation_id": selected.implementation_id,
            "artifact_path": selected.artifact.path, "artifact_digest": selected.artifact.digest,
            "artifact_size": selected.artifact.size, "artifact_media_type": selected.artifact.media_type,
            "artifact_target": selected.artifact.target,
        }));
        sources.push(candidate.clone());
        inputs.push(LocalPluginInput {
            descriptor,
            manifest_digest: verified.manifest_digest,
            app_owned: candidate.role == SourceRole::AppOwned,
            source: candidate.project.display().to_string(),
        });
    }
    let (authority, proposed) =
        GeneratedHostBuild::lower_local(&args.id, inputs)?.with_local_root(stage.path())?;
    if !proposed.dependency_choices().is_empty() {
        fs::create_dir_all(stage.path().join("plugins"))?;
        let legacy = stage.path().join("plugins/dependencies.json");
        if legacy.try_exists()? {
            fs::remove_file(legacy)?;
        }
        let document = lenso_app_authoring::DependencySelectionsDocument {
            schema_version: lenso_app_authoring::DEPENDENCY_SELECTIONS_SCHEMA_VERSION,
            choices: proposed.dependency_choices().to_vec(),
        };
        fs::write(
            stage.path().join("plugins/.dependencies.json"),
            serde_json::to_vec_pretty(&document)?,
        )?;
    }
    fs::write(
        stage.path().join(".lenso/host-build.json"),
        serde_json::to_vec_pretty(&authority)?,
    )?;
    fs::write(
        stage.path().join("bundles.json"),
        serde_json::to_vec_pretty(&inventory)?,
    )?;
    let resolved = lenso_app_authoring::load_resolved_app(stage.path())
        .context("resolve local Host with Plugin Root intent")?;
    fs::write(
        stage.path().join("local-sources.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": "lenso.local-sources.v1", "template": "lenso.local-host@1",
            "cli_version": env!("CARGO_PKG_VERSION"), "target": lenso_app_authoring::native_host_target(),
            "sources": sources,
        }))?,
    )?;
    super::build::publish_new_output(stage.path(), &destination)?;
    if args.json {
        println!(
            "{}",
            json!({"schema_version":1, "kind":"lenso.app-assemble", "out":destination,
            "plugin_instances": resolved.instances().len(), "capability_bindings": resolved.plan().capability_bindings().len()})
        );
    } else {
        println!(
            "Assembled {} Plugin Instances at {}. Runtime distribution/startup is a separate step.",
            resolved.instances().len(),
            destination.display()
        );
    }
    Ok(())
}

fn local_implementation_policy() -> ImplementationPolicy {
    ImplementationPolicy {
        host_target: lenso_app_authoring::native_host_target().to_owned(),
        runtimes: [
            ("lenso.process@1", "lenso.process-stdio@2"),
            ("lenso.process@1", "lenso.process@1"),
            ("lenso.wasm-component@1", "lenso.wasm-component@1"),
            (
                "lenso.bun-process@1",
                lenso_bun_adapter::BUN_AUTHORING_RUNTIME_PROFILE,
            ),
            (
                "lenso.bun-process@1",
                lenso_app_plan::PLUGIN_AUTHORING_V2_RUNTIME_PROFILE,
            ),
            ("lenso.bun-process@1", "lenso.bun-process@1"),
        ]
        .into_iter()
        .map(|(class, profile)| RuntimeAdmission {
            execution_class: ExecutionClassId::new(class),
            runtime_profile: profile.to_owned(),
        })
        .collect(),
    }
}

fn copy_root(
    source: &Path,
    destination: &Path,
    depth: usize,
    bytes: &mut u64,
) -> anyhow::Result<()> {
    if depth > 32 {
        bail!("Plugin Root exceeds 32 directory levels");
    }
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_root(
                &entry.path(),
                &destination.join(entry.file_name()),
                depth + 1,
                bytes,
            )?;
        }
    } else if metadata.is_file() {
        *bytes += metadata.len();
        if *bytes > 256 * 1024 * 1024 {
            bail!("Plugin Root snapshot exceeds 256 MiB");
        }
        fs::copy(source, destination)?;
    } else {
        bail!(
            "Plugin Root contains a symlink or special file: {}",
            source.display()
        );
    }
    Ok(())
}
