use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Args, Subcommand};
use lenso_plugin_bundle::{
    ArtifactSource, BundleBuild, VerifiedBundle, build_bundle, verify_bundle_directory,
};

#[derive(Clone, Debug, Subcommand)]
pub enum PluginCommand {
    /// Materialize source artifacts and write one immutable Plugin Bundle.
    Build(PluginBuildArgs),
    /// Verify a materialized Plugin Bundle without admitting or running it.
    Verify(PluginVerifyArgs),
}

#[derive(Args, Clone, Debug)]
pub struct PluginBuildArgs {
    /// Publisher Manifest template containing stable Plugin metadata and placeholder digests.
    #[arg(long, default_value = "lenso-plugin.template.json")]
    manifest: PathBuf,

    /// New Bundle directory. Existing paths are never overwritten.
    #[arg(long)]
    output: PathBuf,

    /// Source override in `ARTIFACT_ID=PATH` form. Repeat for multiple Artifacts.
    #[arg(long = "artifact", value_name = "ARTIFACT_ID=PATH")]
    artifacts: Vec<String>,

    /// Emit the build result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args, Clone, Debug)]
pub struct PluginVerifyArgs {
    /// Materialized Bundle directory containing lenso-plugin.json.
    #[arg(long)]
    bundle: PathBuf,

    /// Emit the verification result as JSON.
    #[arg(long)]
    json: bool,
}

pub fn plugin(command: PluginCommand) -> anyhow::Result<()> {
    match command {
        PluginCommand::Build(args) => build(args),
        PluginCommand::Verify(args) => verify(&args),
    }
}

fn build(args: PluginBuildArgs) -> anyhow::Result<()> {
    let artifact_sources = args
        .artifacts
        .iter()
        .map(|value| parse_artifact_source(value))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let verified = build_bundle(&BundleBuild {
        template: args.manifest,
        output: args.output.clone(),
        artifact_sources,
    })
    .with_context(|| format!("failed to build Plugin Bundle `{}`", args.output.display()))?;
    print_result(&verified, args.json)?;
    Ok(())
}

fn verify(args: &PluginVerifyArgs) -> anyhow::Result<()> {
    let verified = verify_bundle_directory(&args.bundle)
        .with_context(|| format!("failed to verify Plugin Bundle `{}`", args.bundle.display()))?;
    print_result(&verified, args.json)?;
    Ok(())
}

fn parse_artifact_source(value: &str) -> anyhow::Result<ArtifactSource> {
    let Some((artifact_id, path)) = value.split_once('=') else {
        bail!("Artifact source `{value}` must use ARTIFACT_ID=PATH");
    };
    if artifact_id.is_empty() || path.is_empty() {
        bail!("Artifact source `{value}` must include a non-empty ID and path");
    }
    Ok(ArtifactSource {
        artifact_id: artifact_id.to_owned(),
        path: PathBuf::from(path),
    })
}

fn print_result(verified: &VerifiedBundle, json: bool) -> anyhow::Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "plugin_id": verified.plugin_id,
                "release_version": verified.release_version,
                "manifest_digest": verified.manifest_digest,
                "artifact_digests": verified.artifact_digests,
                "product_metadata_digests": verified.product_metadata_digests,
            }))?
        );
    } else {
        println!(
            "verified {}@{}",
            verified.plugin_id, verified.release_version
        );
        println!("manifest {}", verified.manifest_digest);
        for digest in &verified.artifact_digests {
            println!("artifact {digest}");
        }
        for digest in &verified.product_metadata_digests {
            println!("product-metadata {digest}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO_DIGEST: &str =
        "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn artifact_source_requires_an_id_and_path() {
        assert_eq!(
            parse_artifact_source("guest=target/guest.wasm").unwrap(),
            ArtifactSource {
                artifact_id: "guest".to_owned(),
                path: PathBuf::from("target/guest.wasm"),
            }
        );
        assert!(parse_artifact_source("guest").is_err());
        assert!(parse_artifact_source("=guest.wasm").is_err());
        assert!(parse_artifact_source("guest=").is_err());
    }

    #[test]
    fn build_and_verify_share_the_exact_bundle_contract() {
        let root = tempfile::tempdir().unwrap();
        let manifest = root.path().join("lenso-plugin.template.json");
        let source = root.path().join("plugin.mjs");
        let output = root.path().join("dist/example-plugin");
        std::fs::write(&source, b"export const ready = true;\n").unwrap();
        std::fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "plugin_id": "example.quickjs",
                "release_version": "1.0.0",
                "artifacts": [{
                    "id": "script",
                    "kind": "quick_js_module",
                    "digest": ZERO_DIGEST,
                    "size": 0,
                    "media_type": "text/javascript",
                    "path": "plugin.mjs",
                    "targets": ["aarch64-macos"]
                }],
                "module_contributions": []
            }))
            .unwrap(),
        )
        .unwrap();

        build(PluginBuildArgs {
            manifest,
            output: output.clone(),
            artifacts: vec![format!("script={}", source.display())],
            json: false,
        })
        .unwrap();
        verify(&PluginVerifyArgs {
            bundle: output,
            json: false,
        })
        .unwrap();
    }
}
