use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct ConsoleDevOptions {
    pub cwd: Option<PathBuf>,
    pub host: Option<String>,
    pub open: bool,
    pub package: Option<PathBuf>,
    pub port: u16,
    pub runtime_console_root: Option<PathBuf>,
}

pub fn run_console_dev(options: ConsoleDevOptions) -> Result<()> {
    let runtime_console_root =
        resolve_runtime_console_root(options.runtime_console_root.as_deref())?;
    let script = runtime_console_root.join("scripts/console-package-dev.mjs");
    if !script.exists() {
        bail!("Runtime Console dev runner not found: {}", script.display());
    }

    let mut command = Command::new("node");
    command.arg(script);
    if let Some(cwd) = options.cwd {
        command.arg("--cwd").arg(cwd);
    }
    if let Some(host) = options.host {
        command.arg("--host").arg(host);
    }
    if let Some(package) = options.package {
        command.arg("--package").arg(package);
    }
    command.arg("--port").arg(options.port.to_string());
    if options.open {
        command.env("LENSO_CONSOLE_DEV_OPEN", "1");
    }

    let status = command.status().with_context(|| {
        format!(
            "run Runtime Console dev runner in {}",
            runtime_console_root.display()
        )
    })?;
    if !status.success() {
        bail!("Runtime Console dev runner exited with {status}");
    }
    Ok(())
}

fn resolve_runtime_console_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit {
        return Ok(root.to_path_buf());
    }
    if let Ok(root) = std::env::var("LENSO_RUNTIME_CONSOLE_ROOT") {
        return Ok(PathBuf::from(root));
    }

    let cwd = std::env::current_dir().context("read current directory")?;
    if let Some(root) = find_runtime_console_root_from(&cwd) {
        return Ok(root);
    }

    let sibling = cwd
        .join("../lenso-runtime-console")
        .canonicalize()
        .context(
            "resolve sibling lenso-runtime-console; set LENSO_RUNTIME_CONSOLE_ROOT if it is elsewhere",
        )?;
    Ok(sibling)
}

fn find_runtime_console_root_from(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|candidate| is_runtime_console_root(candidate))
        .map(Path::to_path_buf)
}

fn is_runtime_console_root(candidate: &Path) -> bool {
    candidate
        .join("scripts")
        .join("console-package-dev.mjs")
        .is_file()
}
