use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use include_dir::{Dir, DirEntry, include_dir};
use sqlx::postgres::PgPoolOptions;
use std::collections::BTreeMap;

/// Embedded starter-host template. This is the single source of truth for the
/// project that `lenso host init` writes out.
const TEMPLATE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates/starter-host");

/// Template-wide rewrite values applied when scaffolding a named project.
#[derive(Debug, Clone)]
struct Rewrites {
    package_name: String,
    lib_name: String,
}

/// Scaffold a new Lenso host application into `dir`.
pub fn init(dir: &str, name: Option<&str>, force: bool) -> Result<()> {
    init_with_output(dir, name, force, true)
}

pub(crate) fn init_quiet(dir: &str, name: Option<&str>, force: bool) -> Result<()> {
    init_with_output(dir, name, force, false)
}

fn init_with_output(
    dir: &str,
    name: Option<&str>,
    force: bool,
    print_guidance: bool,
) -> Result<()> {
    let target = PathBuf::from(dir);
    let default_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("lenso-app");
    let package_name = name.unwrap_or(default_name).to_owned();
    validate_package_name(&package_name)?;

    let lib_name = lib_name_from(&package_name);
    let rewrites = Rewrites {
        package_name: package_name.clone(),
        lib_name,
    };

    prepare_target(&target, force)?;
    extract(&TEMPLATE_DIR, &target, PathBuf::new(), &rewrites)?;

    if print_guidance {
        print_next_steps(&target, &package_name);
    }
    Ok(())
}

/// Start the local services used by a generated Lenso host project.
pub async fn serve(
    repo_root: Option<&Path>,
    skip_db: bool,
    skip_migrate: bool,
    separate_worker: bool,
) -> Result<()> {
    let repo_root = repo_root.unwrap_or_else(|| Path::new("."));
    ensure_host_root(repo_root)?;

    if !skip_db {
        run(repo_root, "docker", &["compose", "up", "-d", "postgres"])?;
        wait_for_database(repo_root, Duration::from_secs(30)).await?;
    }
    if !skip_migrate {
        run(repo_root, "cargo", &cargo_run_args("migrate"))?;
    }

    let embedded_worker = !separate_worker && has_bin(repo_root, "serve");
    let api_label = if embedded_worker { "api+worker" } else { "api" };
    let mut api = spawn_cargo_bin(repo_root, if embedded_worker { "serve" } else { "api" })?;
    let mut worker = if embedded_worker {
        None
    } else {
        Some(spawn_cargo_bin(repo_root, "worker")?)
    };
    print_serve_ready(repo_root);

    loop {
        tokio::select! {
            signal = tokio::signal::ctrl_c() => {
                signal.context("listen for Ctrl-C")?;
                stop_child(api_label, &mut api);
                if let Some(worker) = worker.as_mut() {
                    stop_child("worker", worker);
                }
                return Ok(());
            }
            () = tokio::time::sleep(Duration::from_millis(500)) => {
                if let Some(status) = api.try_wait().with_context(|| format!("check {api_label} process"))? {
                    if let Some(worker) = worker.as_mut() {
                        stop_child("worker", worker);
                    }
                    bail!("{api_label} exited with {status}");
                }
                if let Some(worker) = worker.as_mut() {
                    if let Some(status) = worker.try_wait().context("check worker process")? {
                        stop_child(api_label, &mut api);
                        bail!("worker exited with {status}");
                    }
                }
            }
        }
    }
}

async fn wait_for_database(repo_root: &Path, timeout: Duration) -> Result<()> {
    let database_url = database_url(repo_root, None)?;
    let deadline = Instant::now() + timeout;
    loop {
        match PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(1))
            .connect(&database_url)
            .await
        {
            Ok(pool) => {
                pool.close().await;
                return Ok(());
            }
            Err(error) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(500)).await;
                tracing_retry_database_wait(&error);
            }
            Err(error) => {
                return Err(error).context("wait for generated host database readiness");
            }
        }
    }
}

fn tracing_retry_database_wait(error: &sqlx::Error) {
    eprintln!("Waiting for database readiness: {error}");
}

/// Reject names that cannot be a Cargo package name.
fn validate_package_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => bail!("package name must start with an ASCII letter: {name}"),
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("package name may only contain ASCII letters, digits, '_' and '-': {name}");
    }
    Ok(())
}

fn ensure_host_root(repo_root: &Path) -> Result<()> {
    if !repo_root.join("Cargo.toml").exists() {
        bail!(
            "{} does not look like a Lenso host root",
            repo_root.display()
        );
    }
    Ok(())
}

fn has_bin(repo_root: &Path, bin: &str) -> bool {
    repo_root
        .join("src")
        .join("bin")
        .join(format!("{bin}.rs"))
        .exists()
}

fn print_serve_ready(repo_root: &Path) {
    let base_url = serve_base_url(repo_root);
    eprintln!();
    eprintln!("Lenso host is serving");
    eprintln!();
    eprintln!("  API:     {base_url}");
    eprintln!("  Docs:    {base_url}/docs");
    eprintln!("  Health:  {base_url}/livez");
    eprintln!();
    eprintln!("Press Ctrl-C to stop.");
}

fn serve_base_url(repo_root: &Path) -> String {
    let env_host = std::env::var("HTTP_HOST").ok();
    let env_port = std::env::var("HTTP_PORT").ok();
    serve_base_url_with(repo_root, env_host.as_deref(), env_port.as_deref())
}

fn serve_base_url_with(repo_root: &Path, env_host: Option<&str>, env_port: Option<&str>) -> String {
    let host = env_host
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| dotenv_value(repo_root, "HTTP_HOST"))
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let port = env_port
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| dotenv_value(repo_root, "HTTP_PORT"))
        .unwrap_or_else(|| "3000".to_owned());
    format!("http://{}:{}", browser_host(&host), port.trim())
}

fn dotenv_value(repo_root: &Path, key: &str) -> Option<String> {
    dotenv_value_from_path(&repo_root.join(".env"), key)
}

pub(crate) fn database_url(repo_root: &Path, env_file: Option<&Path>) -> Result<String> {
    if let Ok(value) = std::env::var("DATABASE_URL")
        && !value.trim().is_empty()
    {
        return Ok(value);
    }
    let env_path = env_file.map_or_else(|| repo_root.join(".env"), Path::to_path_buf);
    database_url_from_path(&env_path)
}

pub(crate) fn database_url_from_path(env_path: &Path) -> Result<String> {
    dotenv_value_from_path(env_path, "DATABASE_URL")
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("DATABASE_URL is not set in {}", env_path.display()))
}

fn dotenv_value_from_path(path: &Path, key: &str) -> Option<String> {
    let values = dotenv_values(&fs::read_to_string(path).ok()?);
    let raw = values.get(key)?;
    Some(expand_env_value(raw, &values))
}

fn dotenv_values(source: &str) -> BTreeMap<String, String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_owned(), unquote_env_value(value.trim())))
        })
        .collect()
}

fn unquote_env_value(value: &str) -> String {
    value.trim_matches('"').trim_matches('\'').to_owned()
}

fn expand_env_value(value: &str, values: &BTreeMap<String, String>) -> String {
    let mut output = String::new();
    let mut rest = value;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let key = &after[..end];
        if let Ok(env_value) = std::env::var(key) {
            output.push_str(&env_value);
        } else if let Some(file_value) = values.get(key) {
            output.push_str(file_value);
        }
        rest = &after[end + 1..];
    }
    output.push_str(rest);
    output
}

fn browser_host(host: &str) -> String {
    match host.trim() {
        "0.0.0.0" => "127.0.0.1".to_owned(),
        "::" => "[::1]".to_owned(),
        host if host.contains(':') && !host.starts_with('[') => format!("[{host}]"),
        host => host.to_owned(),
    }
}

fn cargo_run_args(bin: &str) -> Vec<&str> {
    vec!["run", "--bin", bin]
}

fn run(repo_root: &Path, program: &str, args: &[&str]) -> Result<()> {
    eprintln!("$ {} {}", program, args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(repo_root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("run {program}"))?;
    if !status.success() {
        bail!("{program} exited with {status}");
    }
    Ok(())
}

fn spawn_cargo_bin(repo_root: &Path, bin: &str) -> Result<Child> {
    let args = cargo_run_args(bin);
    eprintln!("$ cargo {}", args.join(" "));
    Command::new("cargo")
        .args(args)
        .current_dir(repo_root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("start {bin}"))
}

fn stop_child(label: &str, child: &mut Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
    eprintln!("Stopped {label}.");
}

/// Convert a package name to its Cargo library crate name (`-` becomes `_`).
fn lib_name_from(package_name: &str) -> String {
    package_name.replace('-', "_")
}

/// Ensure the target directory is empty (or missing) unless `force` is set.
fn prepare_target(target: &Path, force: bool) -> Result<()> {
    if target.exists() {
        let is_empty = target
            .read_dir()
            .with_context(|| format!("read target directory {}", target.display()))?
            .next()
            .is_none();
        if !is_empty && !force {
            bail!(
                "target directory is not empty: {} (pass --force to overwrite)",
                target.display()
            );
        }
    } else {
        fs::create_dir_all(target)
            .with_context(|| format!("create target directory {}", target.display()))?;
    }
    Ok(())
}

/// Recursively copy the embedded template into `target`, applying rewrites.
fn extract(dir: &Dir, target: &Path, rel: PathBuf, rewrites: &Rewrites) -> Result<()> {
    for entry in dir.entries() {
        let name = entry_name(entry)?;
        let entry_rel = rel.join(name);
        let out_path = target.join(&entry_rel);
        match entry {
            DirEntry::Dir(child) => {
                fs::create_dir_all(&out_path)
                    .with_context(|| format!("create directory {}", out_path.display()))?;
                extract(child, target, entry_rel, rewrites)?;
            }
            DirEntry::File(file) => {
                let out_path = output_path(target, &entry_rel);
                write_file(
                    file.contents(),
                    rewrite_for(&entry_rel),
                    &out_path,
                    rewrites,
                )?;
            }
        }
    }
    Ok(())
}

/// Map a template-relative path to its rewrite kind.
///
/// The template manifest is stored as `Cargo.toml.tmpl` so the package does not
/// look like a nested Cargo project; it is written out as `Cargo.toml`.
fn rewrite_for(rel: &Path) -> RewriteKind {
    match rel.to_str() {
        Some("Cargo.toml.tmpl") => RewriteKind::Manifest,
        Some(p) if p.starts_with("src/bin/") && p.ends_with(".rs") => RewriteKind::BinSource,
        _ => RewriteKind::None,
    }
}

#[derive(Debug, Clone, Copy)]
enum RewriteKind {
    None,
    Manifest,
    BinSource,
}

/// Output path for a template file, renaming `Cargo.toml.tmpl` to `Cargo.toml`.
fn output_path(target: &Path, rel: &Path) -> PathBuf {
    let mut out = target.join(rel);
    if out.file_name().and_then(|n| n.to_str()) == Some("Cargo.toml.tmpl") {
        out.set_file_name("Cargo.toml");
    }
    out
}

/// File name for a template entry, regardless of nesting depth.
fn entry_name<'a>(entry: &DirEntry<'a>) -> Result<&'a str> {
    entry
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            anyhow!(
                "template entry without a valid file name: {}",
                entry.path().display()
            )
        })
}

/// Write one template file, rewriting the manifest and bin entrypoints.
fn write_file(contents: &[u8], kind: RewriteKind, out: &Path, rewrites: &Rewrites) -> Result<()> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }

    let bytes: Vec<u8> = match kind {
        RewriteKind::Manifest => rewrite_cargo_toml(contents, rewrites)?.into_bytes(),
        RewriteKind::BinSource => rewrite_bin_source(contents, rewrites).into_bytes(),
        RewriteKind::None => contents.to_vec(),
    };

    fs::write(out, bytes).with_context(|| format!("write {}", out.display()))?;
    Ok(())
}

/// Replace the template package name with the requested project name.
fn rewrite_cargo_toml(contents: &[u8], rewrites: &Rewrites) -> Result<String> {
    let text = std::str::from_utf8(contents).context("template Cargo.toml is not UTF-8")?;
    let original = "name = \"lenso-starter-host\"";
    let replacement = format!("name = \"{}\"", rewrites.package_name);
    if !text.contains(original) {
        bail!("template Cargo.toml no longer declares the starter package name");
    }
    Ok(text.replacen(original, &replacement, 1))
}

/// Repoint bin entrypoints from the starter lib crate to the project lib crate.
fn rewrite_bin_source(contents: &[u8], rewrites: &Rewrites) -> String {
    let text = std::str::from_utf8(contents).unwrap_or_default();
    text.replace("lenso_starter_host", &rewrites.lib_name)
}

fn print_next_steps(target: &Path, package_name: &str) {
    eprintln!(
        "Created Lenso host project `{package_name}` in {}",
        target.display()
    );
    eprintln!();
    eprintln!("Next steps:");
    eprintln!("  cd {}", target.display());
    eprintln!("  cp .env.example .env");
    eprintln!("  lenso serve");
    eprintln!();
    eprintln!("Install a service with `lenso service install <service-name-or-manifest>`.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lib_name_replaces_dashes() {
        assert_eq!(lib_name_from("lenso-starter-host"), "lenso_starter_host");
        assert_eq!(lib_name_from("my-app"), "my_app");
        assert_eq!(lib_name_from("app"), "app");
    }

    #[test]
    fn validates_package_names() {
        assert!(validate_package_name("my-app").is_ok());
        assert!(validate_package_name("App2").is_ok());
        assert!(validate_package_name("2app").is_err());
        assert!(validate_package_name("my app").is_err());
        assert!(validate_package_name("-app").is_err());
    }

    #[test]
    fn rewrites_cargo_toml_package_name() {
        let rewrites = Rewrites {
            package_name: "billing-svc".to_owned(),
            lib_name: "billing_svc".to_owned(),
        };
        let input = b"[package]\nname = \"lenso-starter-host\"\nversion = \"0.1.0\"\n";
        let out = rewrite_cargo_toml(input, &rewrites).unwrap();
        assert!(out.contains("name = \"billing-svc\""));
        assert!(!out.contains("lenso-starter-host"));
    }

    #[test]
    fn rewrites_bin_source_lib_reference() {
        let rewrites = Rewrites {
            package_name: "billing-svc".to_owned(),
            lib_name: "billing_svc".to_owned(),
        };
        let input = b"lenso_starter_host::host_composition()";
        let out = rewrite_bin_source(input, &rewrites);
        assert_eq!(out, "billing_svc::host_composition()");
    }

    #[test]
    fn starter_http_routes_include_current_operation_metadata() {
        let source = TEMPLATE_DIR
            .get_file("src/modules/app/mod.rs")
            .expect("starter app module template")
            .contents_utf8()
            .expect("starter app module is UTF-8");

        assert_eq!(source.matches("ModuleHttpRoute {").count(), 6);
        assert_eq!(source.matches("operation: None,").count(), 6);
    }

    #[test]
    fn cargo_run_args_target_host_bins() {
        assert_eq!(cargo_run_args("api"), vec!["run", "--bin", "api"]);
        assert_eq!(cargo_run_args("serve"), vec!["run", "--bin", "serve"]);
        assert_eq!(cargo_run_args("worker"), vec!["run", "--bin", "worker"]);
    }

    #[test]
    fn serve_base_url_reads_env_file_and_browser_host() {
        let target = temp_dir("lenso-cli-serve-url");
        fs::create_dir_all(&target).unwrap();
        fs::write(&target.join(".env"), "HTTP_HOST=0.0.0.0\nHTTP_PORT=4242\n").unwrap();

        assert_eq!(
            serve_base_url_with(&target, None, None),
            "http://127.0.0.1:4242"
        );
        assert_eq!(
            serve_base_url_with(&target, Some("localhost"), Some("8080")),
            "http://localhost:8080"
        );

        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn dotenv_value_expands_template_variables() {
        let target = temp_dir("lenso-cli-dotenv-expand");
        fs::create_dir_all(&target).unwrap();
        fs::write(
            target.join(".env"),
            "POSTGRES_HOST_PORT=4545\nDATABASE_URL=postgres://lenso:lenso@127.0.0.1:${POSTGRES_HOST_PORT}/lenso\n",
        )
        .unwrap();

        assert_eq!(
            dotenv_value(&target, "DATABASE_URL").as_deref(),
            Some("postgres://lenso:lenso@127.0.0.1:4545/lenso")
        );

        fs::remove_dir_all(target).unwrap();
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "{}-{}",
            prefix,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
