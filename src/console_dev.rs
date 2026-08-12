use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest as _, Sha256};

const LOCAL_CONSOLE_IMAGE: &str = "ghcr.io/liorael/lenso-console@sha256:17a13080be00c62126caac6ca54866ffd0ecda8fc30586d6b98fbafb9ca0a753";

#[derive(Debug, Clone)]
pub struct ConsoleDevOptions {
    pub console_root: Option<PathBuf>,
}

pub async fn run_console_dev(options: ConsoleDevOptions) -> Result<()> {
    let runtime = prepare_console_dev(options.console_root.as_deref(), None, &BTreeMap::new())?;
    eprintln!(
        "Console local environment is ready at {}.",
        runtime.url.trim_end_matches('/')
    );
    let mut process = spawn_console_dev(&runtime)?;
    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("listen for Ctrl-C")?;
                process.stop();
                return Ok(());
            }
            () = tokio::time::sleep(Duration::from_millis(500)) => {
                process.check()?;
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct ConsoleDevRuntime {
    pub(crate) root: PathBuf,
    pub(crate) env_file: PathBuf,
    pub(crate) url: String,
    backend: ConsoleDevBackend,
    launch_environment: BTreeMap<String, String>,
}

#[derive(Clone)]
enum ConsoleDevBackend {
    Source,
    Container {
        compose_file: PathBuf,
        project: String,
    },
}

#[derive(Debug)]
pub(crate) struct ConsoleDevProcess {
    process: ConsoleDevProcessKind,
}

#[derive(Debug)]
enum ConsoleDevProcessKind {
    Source(Child),
    Container {
        compose_file: PathBuf,
        env_file: PathBuf,
        project: String,
        root: PathBuf,
        stopped: bool,
        last_check: Instant,
    },
}

impl ConsoleDevProcess {
    pub(crate) fn check(&mut self) -> Result<()> {
        match &mut self.process {
            ConsoleDevProcessKind::Source(child) => {
                if let Some(status) = child.try_wait().context("check Console Service process")? {
                    bail!("Console Service exited with {status}");
                }
            }
            ConsoleDevProcessKind::Container {
                compose_file,
                env_file,
                project,
                root,
                stopped,
                last_check,
            } if !*stopped => {
                if last_check.elapsed() < Duration::from_secs(2) {
                    return Ok(());
                }
                let output = compose_output(
                    root,
                    env_file,
                    compose_file,
                    project,
                    &["ps", "--status", "running", "--services", "console"],
                )?;
                if output.trim() != "console" {
                    bail!("Console Service container is not running");
                }
                *last_check = Instant::now();
            }
            ConsoleDevProcessKind::Container { .. } => {}
        }
        Ok(())
    }

    pub(crate) fn stop(&mut self) {
        match &mut self.process {
            ConsoleDevProcessKind::Source(child) => {
                if matches!(child.try_wait(), Ok(Some(_))) {
                    return;
                }
                let _ = child.kill();
                let _ = child.wait();
            }
            ConsoleDevProcessKind::Container {
                compose_file,
                env_file,
                project,
                root,
                stopped,
                ..
            } => {
                if *stopped {
                    return;
                }
                let _ = run_compose(
                    root,
                    env_file,
                    compose_file,
                    project,
                    &["stop", "console"],
                    "Console Service",
                );
                *stopped = true;
            }
        }
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
    runtime_root: Option<&Path>,
    environment: &BTreeMap<String, String>,
) -> Result<ConsoleDevRuntime> {
    let default_root = match runtime_root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().context("read current directory")?,
    };
    if console_root.is_none() && !is_console_root(&default_root) {
        return prepare_container_console_dev(&default_root, environment);
    }
    let root = if let Some(root) = console_root {
        validate_console_root(root.to_path_buf())?
    } else {
        validate_console_root(default_root)?
    };
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
        backend: ConsoleDevBackend::Source,
        launch_environment: environment.clone(),
    })
}

pub(crate) fn spawn_console_dev(runtime: &ConsoleDevRuntime) -> Result<ConsoleDevProcess> {
    if let ConsoleDevBackend::Container {
        compose_file,
        project,
    } = &runtime.backend
    {
        run_compose(
            &runtime.root,
            &runtime.env_file,
            compose_file,
            project,
            &["up", "--detach", "--wait", "console"],
            "Console Service",
        )?;
        return Ok(ConsoleDevProcess {
            process: ConsoleDevProcessKind::Container {
                compose_file: compose_file.clone(),
                env_file: runtime.env_file.clone(),
                project: project.clone(),
                root: runtime.root.clone(),
                stopped: false,
                last_check: Instant::now(),
            },
        });
    }
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
    Ok(ConsoleDevProcess {
        process: ConsoleDevProcessKind::Source(child),
    })
}

fn prepare_container_console_dev(
    host_root: &Path,
    environment: &BTreeMap<String, String>,
) -> Result<ConsoleDevRuntime> {
    let root = host_root.join(".lenso/console-service");
    fs::create_dir_all(&root)
        .with_context(|| format!("create local Console runtime {}", root.display()))?;
    let env_file = root.join(".env");
    ensure_container_environment(&env_file, environment)?;
    let compose_file = root.join("compose.yml");
    fs::write(
        &compose_file,
        container_compose_document(
            environment.contains_key("LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST"),
        ),
    )
    .with_context(|| {
        format!(
            "write local Console Compose file {}",
            compose_file.display()
        )
    })?;
    let project = console_compose_project(&root);
    run_compose(
        &root,
        &env_file,
        &compose_file,
        &project,
        &["up", "--detach", "--wait", "postgres"],
        "Console Postgres",
    )?;
    run_compose(
        &root,
        &env_file,
        &compose_file,
        &project,
        &["run", "--rm", "migrate"],
        "Console migrations",
    )?;
    eprintln!("Pinned Console image ready: {LOCAL_CONSOLE_IMAGE}");
    Ok(ConsoleDevRuntime {
        root,
        url: console_url(&env_file)?,
        env_file,
        backend: ConsoleDevBackend::Container {
            compose_file,
            project,
        },
        launch_environment: BTreeMap::new(),
    })
}

fn ensure_container_environment(path: &Path, environment: &BTreeMap<String, String>) -> Result<()> {
    let mut source = if path.is_file() {
        fs::read_to_string(path)
            .with_context(|| format!("read local Console environment {}", path.display()))?
    } else {
        let postgres = crate::host::reserve_loopback_port(55_433)?;
        let http = crate::host::reserve_loopback_port(3_030)?;
        let postgres_port = postgres.local_addr()?.port();
        let http_port = http.local_addr()?.port();
        let password = random_secret()?;
        format!(
            "CONSOLE_DATABASE_URL=postgres://lenso_console:{password}@127.0.0.1:{postgres_port}/lenso_console\nDATABASE_URL=postgres://lenso_console:{password}@127.0.0.1:{postgres_port}/lenso_console\nPOSTGRES_PASSWORD={password}\nPOSTGRES_HOST_PORT={postgres_port}\nCONSOLE_HTTP_PORT={http_port}\nHTTP_PORT={http_port}\nCONSOLE_PUBLIC_ORIGIN=http://127.0.0.1:{http_port}\nCONSOLE_RECOVERY_MODE=normal\n"
        )
    };
    for (key, value) in environment {
        source = upsert_env_value(&source, key, value);
    }
    fs::write(path, source)
        .with_context(|| format!("write local Console environment {}", path.display()))?;
    make_private(path)
}

fn random_secret() -> Result<String> {
    let mut bytes = [0_u8; 24];
    getrandom::fill(&mut bytes).context("generate local Console database secret")?;
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(encoded)
}

fn container_compose_document(include_enrollment_trust: bool) -> String {
    let enrollment_trust = if include_enrollment_trust {
        "      LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST: ${LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST:?set enrollment trust}\n"
    } else {
        ""
    };
    format!(
        r#"services:
  postgres:
    image: postgres:18-alpine
    environment:
      - POSTGRES_DB=lenso_console
      - POSTGRES_PASSWORD
      - POSTGRES_USER=lenso_console
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U lenso_console -d lenso_console"]
      interval: 1s
      timeout: 3s
      retries: 60
    ports:
      - "127.0.0.1:${{POSTGRES_HOST_PORT:?set POSTGRES_HOST_PORT}}:5432"
    volumes:
      - console-database:/var/lib/postgresql
  migrate:
    image: {LOCAL_CONSOLE_IMAGE}
    command: ["/usr/local/bin/lenso-console-migrate"]
    depends_on:
      postgres:
        condition: service_healthy
    environment: &console-environment
      APP_ENV: production
      CORS_ALLOWED_ORIGINS: ${{CONSOLE_PUBLIC_ORIGIN:?set CONSOLE_PUBLIC_ORIGIN}}
      CONSOLE_RECOVERY_MODE: normal
      DATABASE_URL: postgres://lenso_console:${{POSTGRES_PASSWORD}}@postgres:5432/lenso_console
      LENSO_COMPOSITION_PROFILE: core
      LENSO_MODULE_PLATFORM_STORY_ENABLED: "false"
{enrollment_trust}
      LOG_FORMAT: json
      SERVICE_NAME: lenso-console
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    tmpfs:
      - /tmp
  console:
    image: {LOCAL_CONSOLE_IMAGE}
    depends_on:
      postgres:
        condition: service_healthy
    environment: *console-environment
    healthcheck:
      test: ["CMD", "curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:3030/health/ready"]
      interval: 2s
      timeout: 3s
      retries: 60
    ports:
      - "127.0.0.1:${{CONSOLE_HTTP_PORT:?set CONSOLE_HTTP_PORT}}:3030"
    read_only: true
    security_opt:
      - no-new-privileges:true
    cap_drop:
      - ALL
    tmpfs:
      - /tmp
    volumes:
      - console-artifacts:/opt/lenso-console/artifacts
volumes:
  console-artifacts:
  console-database:
"#
    )
}

fn run_compose(
    root: &Path,
    env_file: &Path,
    compose_file: &Path,
    project: &str,
    args: &[&str],
    subject: &str,
) -> Result<()> {
    eprintln!(
        "$ docker compose --project-name {project} {}",
        args.join(" ")
    );
    let status = Command::new("docker")
        .args(["compose", "--project-name", project, "--env-file"])
        .arg(env_file)
        .args(["--file"])
        .arg(compose_file)
        .args(args)
        .current_dir(root)
        .status()
        .with_context(|| {
            format!(
                "run Docker Compose for {subject} in {}; start a Docker-compatible runtime or use --console-root with a Console source checkout",
                root.display()
            )
        })?;
    if !status.success() {
        bail!("{subject} exited with {status}");
    }
    Ok(())
}

fn compose_output(
    root: &Path,
    env_file: &Path,
    compose_file: &Path,
    project: &str,
    args: &[&str],
) -> Result<String> {
    let output = Command::new("docker")
        .args(["compose", "--project-name", project, "--env-file"])
        .arg(env_file)
        .args(["--file"])
        .arg(compose_file)
        .args(args)
        .current_dir(root)
        .output()
        .context("inspect local Console Service container")?;
    if !output.status.success() {
        bail!(
            "inspect local Console Service container exited with {}",
            output.status
        );
    }
    String::from_utf8(output.stdout).context("decode local Console Service status")
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

    #[test]
    fn container_runtime_pins_the_console_release_and_isolates_state() {
        let compose = container_compose_document(true);

        assert!(compose.contains(LOCAL_CONSOLE_IMAGE));
        assert!(compose.contains("postgres:18-alpine"));
        assert!(compose.contains("/usr/local/bin/lenso-console-migrate"));
        assert!(compose.contains("console-database:/var/lib/postgresql"));
        assert!(compose.contains("console-artifacts:/opt/lenso-console/artifacts"));
        assert!(compose.contains("127.0.0.1:${POSTGRES_HOST_PORT"));
        assert!(compose.contains("127.0.0.1:${CONSOLE_HTTP_PORT"));
        assert!(compose.contains("127.0.0.1:3030/health/ready"));
        assert!(compose.contains("LENSO_MODULE_PLATFORM_STORY_ENABLED: \"false\""));
        assert!(!compose.contains(":latest"));
        assert!(compose.contains("LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST"));

        let standalone = container_compose_document(false);
        assert!(!standalone.contains("LENSO_MODULE_LENSO_SYSTEM_REGISTRY__ENROLLMENT_TRUST"));
    }
}
