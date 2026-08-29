use std::{path::PathBuf, process::Command};

use clap::Args;
use serde::Serialize;

use crate::plugins::{load_resolved_app, project_root};

#[derive(Clone, Debug, Args)]
pub(crate) struct DoctorArgs {
    /// App project root. Defaults to the current directory.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Emit a stable JSON report.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Serialize)]
struct Check {
    name: &'static str,
    status: &'static str,
    detail: String,
}

pub(crate) fn doctor(args: DoctorArgs) -> anyhow::Result<()> {
    let root = project_root(args.root)?;
    let mut checks = Vec::new();
    checks.push(command_check("cargo", &["--version"]));
    checks.push(command_check("rustc", &["--version"]));
    checks.push(command_check("bun", &["--version"]));
    checks.push(path_check(
        "host_catalog",
        root.join(".lenso/host-catalog.json"),
    ));
    checks.push(path_check("host_executable", root.join(".lenso/host")));
    checks.push(match load_resolved_app(&root) {
        Ok(app) => Check {
            name: "app_resolution",
            status: "passed",
            detail: format!(
                "{} Plugin Instance(s), {} Capability binding(s)",
                app.instances().len(),
                app.plan().capability_bindings().len()
            ),
        },
        Err(error) => Check {
            name: "app_resolution",
            status: "failed",
            detail: format!("{error:#}"),
        },
    });
    let failed = checks
        .iter()
        .filter(|check| check.status == "failed")
        .count();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "kind": "lenso.doctor",
                "status": if failed == 0 { "passed" } else { "failed" },
                "root": root,
                "checks": checks,
            }))?
        );
    } else {
        for check in &checks {
            println!("{:<18} {:<7} {}", check.name, check.status, check.detail);
        }
    }
    if failed > 0 {
        anyhow::bail!("{failed} doctor check(s) failed");
    }
    Ok(())
}

fn command_check(name: &'static str, arguments: &[&str]) -> Check {
    match Command::new(name).args(arguments).output() {
        Ok(output) if output.status.success() => Check {
            name,
            status: "passed",
            detail: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        },
        Ok(output) => Check {
            name,
            status: "failed",
            detail: format!("exited with {}", output.status),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Check {
            name,
            status: "skipped",
            detail: "not installed; required only for matching Plugin execution classes".to_owned(),
        },
        Err(error) => Check {
            name,
            status: "failed",
            detail: error.to_string(),
        },
    }
}

fn path_check(name: &'static str, path: PathBuf) -> Check {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_file() => Check {
            name,
            status: "passed",
            detail: path.display().to_string(),
        },
        Ok(_) => Check {
            name,
            status: "failed",
            detail: format!("{} is not a regular file", path.display()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && name == "host_executable" => {
            Check {
                name,
                status: "skipped",
                detail: format!("{} is required only by `lenso run`", path.display()),
            }
        }
        Err(error) => Check {
            name,
            status: "failed",
            detail: format!("{}: {error}", path.display()),
        },
    }
}
