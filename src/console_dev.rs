use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct ConsoleDevOptions {
    pub console_root: Option<PathBuf>,
}

pub fn run_console_dev(options: ConsoleDevOptions) -> Result<()> {
    let root = resolve_console_root(options.console_root.as_deref())?;
    run_pnpm(&root, "service:serve", "Console Service")
}

pub fn run_module_console_ui_dev(repo_root: Option<&Path>) -> Result<()> {
    let root = repo_root
        .map(Path::to_path_buf)
        .unwrap_or(std::env::current_dir().context("read current directory")?);
    let ui_root = root.join("console-ui");
    if !ui_root.join("package.json").is_file() {
        bail!(
            "Module Console UI artifact not found: {}",
            ui_root.join("package.json").display()
        );
    }
    run_pnpm(&ui_root, "dev", "Module Console UI artifact")
}

fn run_pnpm(root: &Path, script: &str, subject: &str) -> Result<()> {
    let status = Command::new("pnpm")
        .args(["run", script])
        .current_dir(root)
        .status()
        .with_context(|| format!("start {subject} in {}", root.display()))?;
    if !status.success() {
        bail!("{subject} dev process exited with {status}");
    }
    Ok(())
}

fn resolve_console_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit {
        return validate_console_root(root.to_path_buf());
    }
    let cwd = std::env::current_dir().context("read current directory")?;
    if is_console_root(&cwd) {
        return Ok(cwd);
    }
    bail!("could not find the Console repository; pass --console-root")
}

fn validate_console_root(root: PathBuf) -> Result<PathBuf> {
    if is_console_root(&root) {
        Ok(root)
    } else {
        bail!("Console repository not found at {}", root.display())
    }
}

fn is_console_root(candidate: &Path) -> bool {
    candidate.join("service/Cargo.toml").is_file() && candidate.join("package.json").is_file()
}
