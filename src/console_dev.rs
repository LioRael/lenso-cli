use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};
use sha2::{Digest as _, Sha256};

#[derive(Debug, Clone)]
pub struct ConsoleDevOptions {
    pub console_root: Option<PathBuf>,
}

pub fn run_console_dev(options: ConsoleDevOptions) -> Result<()> {
    let runtime = prepare_console_dev(options.console_root.as_deref(), &BTreeMap::new())?;
    eprintln!(
        "Console local environment is ready at {}.",
        runtime.url.trim_end_matches('/')
    );
    let mut process = spawn_console_dev(&runtime)?;
    loop {
        if process
            .child
            .try_wait()
            .context("check Console Service process")?
            .is_some()
        {
            return process.check();
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

#[derive(Clone)]
pub(crate) struct ConsoleDevRuntime {
    pub(crate) root: PathBuf,
    pub(crate) env_file: PathBuf,
    pub(crate) url: String,
    launch_environment: BTreeMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct ConsoleDevProcess {
    child: Child,
}

impl ConsoleDevProcess {
    pub(crate) fn check(&mut self) -> Result<()> {
        if let Some(status) = self
            .child
            .try_wait()
            .context("check Console Service process")?
        {
            bail!("Console Service exited with {status}");
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        eprintln!("Stopped Console Service.");
    }
}

impl Drop for ConsoleDevProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn prepare_console_dev(
    console_root: Option<&Path>,
    environment: &BTreeMap<String, String>,
) -> Result<ConsoleDevRuntime> {
    let root = resolve_console_root(console_root)?;
    let service_root = root.join("service");
    let env_file = ensure_local_environment(&service_root)?;
    update_local_environment(&env_file, environment)?;
    ensure_console_dependencies(&root)?;
    start_console_database(&service_root, &env_file)?;
    run_pnpm(&root, "service:migrate", "Console migrations")?;
    run_pnpm(&root, "service:web-build", "Console web build")?;
    build_console_service(&root)?;
    Ok(ConsoleDevRuntime {
        root,
        url: console_url(&env_file)?,
        env_file,
        launch_environment: environment.clone(),
    })
}

pub(crate) fn spawn_console_dev(runtime: &ConsoleDevRuntime) -> Result<ConsoleDevProcess> {
    let executable = runtime.root.join("service/target/debug").join(format!(
        "lenso-console-serve{}",
        std::env::consts::EXE_SUFFIX
    ));
    eprintln!("$ {}", executable.display());
    let child = Command::new(&executable)
        .current_dir(&runtime.root)
        .envs(&runtime.launch_environment)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("start Console Service in {}", runtime.root.display()))?;
    Ok(ConsoleDevProcess { child })
}

fn build_console_service(root: &Path) -> Result<()> {
    run_command(
        root,
        "cargo",
        &[
            "build",
            "--locked",
            "--manifest-path",
            "service/Cargo.toml",
            "--bin",
            "lenso-console-serve",
        ],
        &[],
        "Console Service",
    )
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

fn ensure_console_dependencies(root: &Path) -> Result<()> {
    if root.join("node_modules/.pnpm/lock.yaml").is_file()
        || root.join("node_modules/.modules.yaml").is_file()
    {
        return Ok(());
    }
    run_command(
        root,
        "pnpm",
        &["install", "--frozen-lockfile"],
        &[],
        "Console dependencies",
    )
}

fn ensure_local_environment(service_root: &Path) -> Result<PathBuf> {
    let env_file = service_root.join(".env");
    if env_file.is_file() {
        make_private(&env_file)?;
        return Ok(env_file);
    }
    let example = service_root.join(".env.example");
    let source = fs::read_to_string(&example)
        .with_context(|| format!("read Console environment template {}", example.display()))?;
    let postgres = crate::host::reserve_loopback_port(55_433)?;
    let http = crate::host::reserve_loopback_port(3_030)?;
    let postgres_port = postgres.local_addr()?.port();
    let http_port = http.local_addr()?.port();
    let rendered = render_local_environment(&source, postgres_port, http_port);
    fs::write(&env_file, rendered)
        .with_context(|| format!("write Console local environment {}", env_file.display()))?;
    make_private(&env_file)?;
    eprintln!("Created Console local environment {}.", env_file.display());
    Ok(env_file)
}

fn update_local_environment(path: &Path, values: &BTreeMap<String, String>) -> Result<()> {
    if values.is_empty() {
        return Ok(());
    }
    let mut source = fs::read_to_string(path)
        .with_context(|| format!("read Console local environment {}", path.display()))?;
    for (key, value) in values {
        source = upsert_env_value(&source, key, value);
    }
    fs::write(path, source)
        .with_context(|| format!("write Console local environment {}", path.display()))?;
    make_private(path)
}

#[cfg(unix)]
fn make_private(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("protect Console environment {}", path.display()))
}

#[cfg(not(unix))]
fn make_private(_path: &Path) -> Result<()> {
    Ok(())
}

fn render_local_environment(source: &str, postgres_port: u16, http_port: u16) -> String {
    let database_url =
        format!("postgres://lenso_console:lenso_console@localhost:{postgres_port}/lenso_console");
    let mut rendered = source.to_owned();
    rendered = upsert_env_value(&rendered, "DATABASE_URL", &database_url);
    rendered = upsert_env_value(&rendered, "POSTGRES_HOST_PORT", &postgres_port.to_string());
    rendered = upsert_env_value(&rendered, "HTTP_PORT", &http_port.to_string());
    rendered = upsert_env_value(
        &rendered,
        "CORS_ALLOWED_ORIGINS",
        &format!("http://localhost:{http_port}"),
    );
    rendered
}

fn upsert_env_value(source: &str, key: &str, value: &str) -> String {
    let prefix = format!("{key}=");
    let mut found = false;
    let mut lines = source
        .lines()
        .map(|line| {
            if line.starts_with(&prefix) {
                found = true;
                format!("{prefix}{value}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>();
    if !found {
        lines.push(format!("{prefix}{value}"));
    }
    format!("{}\n", lines.join("\n"))
}

fn start_console_database(service_root: &Path, env_file: &Path) -> Result<()> {
    let database_url = crate::host::database_url_from_path(env_file)?;
    let postgres_port = reqwest::Url::parse(&database_url)
        .context("parse Console DATABASE_URL")?
        .port()
        .context("Console DATABASE_URL must declare a Postgres host port")?;
    let project = console_compose_project(service_root);
    let postgres_port = postgres_port.to_string();
    let environment = [("POSTGRES_HOST_PORT", postgres_port.as_str())];
    run_command(
        service_root,
        "docker",
        &[
            "compose",
            "--project-name",
            &project,
            "--env-file",
            ".env",
            "-f",
            "docker-compose.yml",
            "up",
            "-d",
            "--wait",
            "postgres",
        ],
        &environment,
        "Console Postgres",
    )
}

fn console_compose_project(service_root: &Path) -> String {
    let mut digest = Sha256::new();
    digest.update(service_root.to_string_lossy().as_bytes());
    let mut suffix = String::with_capacity(12);
    for byte in digest.finalize().iter().take(6) {
        use std::fmt::Write as _;
        write!(&mut suffix, "{byte:02x}").expect("writing to a String cannot fail");
    }
    format!("lenso-console-{suffix}")
}

fn console_url(env_file: &Path) -> Result<String> {
    let source = fs::read_to_string(env_file)
        .with_context(|| format!("read Console environment {}", env_file.display()))?;
    let values = source
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<std::collections::BTreeMap<_, _>>();
    let port = values
        .get("HTTP_PORT")
        .copied()
        .context("Console HTTP_PORT is missing")?;
    Ok(format!("http://127.0.0.1:{port}/"))
}

fn run_command(
    root: &Path,
    program: &str,
    args: &[&str],
    environment: &[(&str, &str)],
    subject: &str,
) -> Result<()> {
    eprintln!("$ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .envs(environment.iter().copied())
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("start {subject} in {}", root.display()))?;
    if !status.success() {
        bail!("{subject} exited with {status}");
    }
    Ok(())
}

pub(crate) fn resolve_console_root(explicit: Option<&Path>) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_environment_is_isolated_and_preserves_console_composition() {
        let source = "DATABASE_URL=postgres://old\nHTTP_PORT=3030\nCORS_ALLOWED_ORIGINS=http://localhost:3030\nLENSO_MODULE_PLATFORM_STORY_ENABLED=false\n";
        let rendered = render_local_environment(source, 55_499, 3_099);

        assert!(rendered.contains(
            "DATABASE_URL=postgres://lenso_console:lenso_console@localhost:55499/lenso_console"
        ));
        assert!(rendered.contains("POSTGRES_HOST_PORT=55499"));
        assert!(rendered.contains("HTTP_PORT=3099"));
        assert!(rendered.contains("CORS_ALLOWED_ORIGINS=http://localhost:3099"));
        assert!(rendered.contains("LENSO_MODULE_PLATFORM_STORY_ENABLED=false"));
    }

    #[test]
    fn compose_project_is_stable_per_console_checkout() {
        assert_eq!(
            console_compose_project(Path::new("/tmp/console")),
            console_compose_project(Path::new("/tmp/console"))
        );
        assert_ne!(
            console_compose_project(Path::new("/tmp/console-a")),
            console_compose_project(Path::new("/tmp/console-b"))
        );
    }
}
