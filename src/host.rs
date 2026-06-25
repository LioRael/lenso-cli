use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use include_dir::{Dir, DirEntry, include_dir};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Embedded starter-host template. This is the single source of truth for the
/// project that `lenso host init` writes out.
const TEMPLATE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates/starter-host");

/// Optional prebuilt Runtime Console payload. Release builds populate this
/// directory before packaging `lenso-cli`; development builds may only contain
/// the marker file.
const CONSOLE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/console");
const CONSOLE_ADMIN_SCOPE: &str = "console.admin";
const CONSOLE_ADMIN_USER_SCOPES_KEY: &str = "auth.console_admin_user_scopes";
const RUNTIME_CONFIG_SERVICE: &str = "*";
const BOOTSTRAP_ACTOR: &str = "lenso-cli:bootstrap-admin";

/// Template-wide rewrite values applied when scaffolding a named project.
#[derive(Debug, Clone)]
struct Rewrites {
    package_name: String,
    lib_name: String,
}

#[derive(Debug, Clone)]
pub struct BootstrapAdminOptions {
    pub repo_root: Option<PathBuf>,
    pub env_file: Option<PathBuf>,
    pub user_id: Option<String>,
    pub identifier: Option<String>,
    pub scopes: Vec<String>,
}

/// Scaffold a new Lenso host application into `dir`.
pub fn init(dir: &str, name: Option<&str>, force: bool) -> Result<()> {
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
    let console_status = install_embedded_console(&target)?;

    print_next_steps(&target, &package_name, console_status);
    Ok(())
}

/// Refresh hosted Runtime Console assets in an existing Lenso host project.
pub fn update_console(repo_root: Option<&Path>) -> Result<()> {
    let target = repo_root.unwrap_or_else(|| Path::new("."));
    match install_embedded_console(target)? {
        ConsoleInstallStatus::Installed => {
            eprintln!(
                "Updated bundled Runtime Console in {}",
                target.join(".lenso").join("console").display()
            );
            Ok(())
        }
        ConsoleInstallStatus::NotPackaged => bail!(
            "Runtime Console assets were not embedded in this lenso-cli build; install a release build that includes the console"
        ),
    }
}

/// Grant Runtime Console admin scopes to an existing auth user.
pub async fn bootstrap_admin(options: BootstrapAdminOptions) -> Result<()> {
    let repo_root = options
        .repo_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    ensure_host_root(repo_root)?;

    let database_url = database_url(repo_root, options.env_file.as_deref())?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("connect to DATABASE_URL")?;
    let user_id = resolve_bootstrap_user_id(&pool, options.user_id, options.identifier).await?;
    let granted = bootstrap_scopes(options.scopes);
    let stored = upsert_console_admin_scopes(&pool, &user_id, &granted).await?;

    eprintln!("Bootstrapped Runtime Console admin user {user_id}.");
    eprintln!("Stored {CONSOLE_ADMIN_USER_SCOPES_KEY}: {stored}");
    eprintln!("Restart api/worker for the scope change to apply.");
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
    eprintln!("{}", console_line(repo_root, &base_url));
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

fn database_url(repo_root: &Path, env_file: Option<&Path>) -> Result<String> {
    if let Ok(value) = std::env::var("DATABASE_URL")
        && !value.trim().is_empty()
    {
        return Ok(value);
    }
    let env_path = env_file.map_or_else(|| repo_root.join(".env"), Path::to_path_buf);
    dotenv_value_from_path(&env_path, "DATABASE_URL")
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("DATABASE_URL is not set in env or {}", env_path.display()))
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

fn console_line(repo_root: &Path, base_url: &str) -> String {
    if repo_root.join(".lenso/console/dist/index.html").exists() {
        format!("  Console: {base_url}/console")
    } else {
        "  Console: not installed. Run `lenso host update-console`.".to_owned()
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

async fn resolve_bootstrap_user_id(
    pool: &sqlx::PgPool,
    user_id: Option<String>,
    identifier: Option<String>,
) -> Result<String> {
    match (user_id, identifier) {
        (Some(_), Some(_)) => bail!("pass either --user-id or --identifier, not both"),
        (Some(user_id), None) => {
            let exists = sqlx::query_scalar::<_, String>("select id from auth.users where id = $1")
                .bind(user_id.trim())
                .fetch_optional(pool)
                .await
                .context("check auth user")?;
            exists.with_context(|| format!("auth user `{}` was not found", user_id.trim()))
        }
        (None, Some(identifier)) => {
            let normalized = normalize_identifier(&identifier)?;
            sqlx::query_scalar::<_, String>(
                "select user_id from auth.identities where provider = 'password' and provider_subject = $1",
            )
            .bind(&normalized)
            .fetch_optional(pool)
            .await
            .with_context(|| format!("find password identity `{normalized}`"))?
            .with_context(|| format!("password identity `{normalized}` was not found"))
        }
        (None, None) => bail!("pass --user-id or --identifier"),
    }
}

fn normalize_identifier(identifier: &str) -> Result<String> {
    let trimmed = identifier.trim();
    if trimmed.is_empty() {
        bail!("identifier is empty");
    }
    if trimmed.contains('@') {
        Ok(trimmed.to_ascii_lowercase())
    } else {
        Ok(trimmed.to_owned())
    }
}

fn bootstrap_scopes(scopes: Vec<String>) -> Vec<String> {
    let mut set = BTreeSet::from([CONSOLE_ADMIN_SCOPE.to_owned()]);
    set.extend(
        scopes
            .into_iter()
            .map(|scope| scope.trim().to_owned())
            .filter(|scope| !scope.is_empty()),
    );
    set.into_iter().collect()
}

async fn upsert_console_admin_scopes(
    pool: &sqlx::PgPool,
    user_id: &str,
    scopes: &[String],
) -> Result<Value> {
    let mut tx = pool.begin().await.context("begin runtime config update")?;
    let old_value = sqlx::query_scalar::<_, Value>(
        "select value from config.setting_values where service = $1 and key = $2",
    )
    .bind(RUNTIME_CONFIG_SERVICE)
    .bind(CONSOLE_ADMIN_USER_SCOPES_KEY)
    .fetch_optional(&mut *tx)
    .await
    .context("load current console admin scopes")?;

    let mut grants = decode_console_admin_scopes(old_value.clone())?;
    merge_user_scopes(&mut grants, user_id, scopes);
    let next_value = serde_json::to_value(grants).context("encode console admin scopes")?;

    sqlx::query(
        r"
        insert into config.setting_values (service, key, value, updated_at, updated_by)
        values ($1, $2, $3, now(), $4)
        on conflict (service, key)
        do update set value = excluded.value, updated_at = now(), updated_by = excluded.updated_by
        ",
    )
    .bind(RUNTIME_CONFIG_SERVICE)
    .bind(CONSOLE_ADMIN_USER_SCOPES_KEY)
    .bind(&next_value)
    .bind(BOOTSTRAP_ACTOR)
    .execute(&mut *tx)
    .await
    .context("write console admin scopes")?;

    sqlx::query(
        r"
        insert into config.setting_audit (id, service, key, old_value, new_value, actor, changed_at)
        values ($1, $2, $3, $4, $5, $6, now())
        ",
    )
    .bind(Uuid::now_v7())
    .bind(RUNTIME_CONFIG_SERVICE)
    .bind(CONSOLE_ADMIN_USER_SCOPES_KEY)
    .bind(&old_value)
    .bind(&next_value)
    .bind(BOOTSTRAP_ACTOR)
    .execute(&mut *tx)
    .await
    .context("audit console admin scope update")?;

    tx.commit().await.context("commit runtime config update")?;
    Ok(next_value)
}

fn decode_console_admin_scopes(value: Option<Value>) -> Result<BTreeMap<String, Vec<String>>> {
    serde_json::from_value(value.unwrap_or_else(|| json!({})))
        .context("decode auth.console_admin_user_scopes")
}

fn merge_user_scopes(grants: &mut BTreeMap<String, Vec<String>>, user_id: &str, scopes: &[String]) {
    let entry = grants.entry(user_id.to_owned()).or_default();
    for scope in scopes {
        if !entry.iter().any(|existing| existing == scope) {
            entry.push(scope.clone());
        }
    }
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

fn install_embedded_console(target: &Path) -> Result<ConsoleInstallStatus> {
    let Some(dist_dir) = CONSOLE_DIR.get_dir("dist") else {
        return Ok(ConsoleInstallStatus::NotPackaged);
    };
    if CONSOLE_DIR.get_file("dist/index.html").is_none() {
        return Ok(ConsoleInstallStatus::NotPackaged);
    }

    let console_root = target.join(".lenso").join("console");
    copy_embedded_dir(dist_dir, &console_root.join("dist"))?;

    if let Some(extensions_dir) = CONSOLE_DIR.get_dir("extensions") {
        copy_embedded_dir(extensions_dir, &console_root.join("extensions"))?;
    } else {
        fs::create_dir_all(console_root.join("extensions"))
            .with_context(|| format!("create {}", console_root.join("extensions").display()))?;
    }

    if let Some(host_extensions_dir) = CONSOLE_DIR.get_dir("dist/extensions/host") {
        copy_embedded_dir(
            host_extensions_dir,
            &console_root.join("extensions").join("host"),
        )?;
    }

    let registry = console_root.join("extensions").join("registry.json");
    if !registry.exists() {
        fs::write(&registry, b"{\"version\":1,\"bundles\":[]}\n")
            .with_context(|| format!("write {}", registry.display()))?;
    }

    Ok(ConsoleInstallStatus::Installed)
}

fn copy_embedded_dir(dir: &Dir, target: &Path) -> Result<()> {
    if target.exists() {
        fs::remove_dir_all(target).with_context(|| format!("remove {}", target.display()))?;
    }
    fs::create_dir_all(target).with_context(|| format!("create {}", target.display()))?;

    for entry in dir.entries() {
        let name = entry_name(entry)?;
        let out_path = target.join(name);
        match entry {
            DirEntry::Dir(child) => copy_embedded_dir(child, &out_path)?,
            DirEntry::File(file) => {
                fs::write(&out_path, file.contents())
                    .with_context(|| format!("write {}", out_path.display()))?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum ConsoleInstallStatus {
    Installed,
    NotPackaged,
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

fn print_next_steps(target: &Path, package_name: &str, console_status: ConsoleInstallStatus) {
    eprintln!(
        "Created Lenso host project `{package_name}` in {}",
        target.display()
    );
    match console_status {
        ConsoleInstallStatus::Installed => {
            eprintln!("Installed the bundled Runtime Console into .lenso/console.");
        }
        ConsoleInstallStatus::NotPackaged => {
            eprintln!("Runtime Console assets were not embedded in this lenso-cli build.");
        }
    }
    eprintln!();
    eprintln!("Next steps:");
    eprintln!("  cd {}", target.display());
    eprintln!("  cp .env.example .env");
    eprintln!("  lenso serve");
    if console_status == ConsoleInstallStatus::Installed {
        eprintln!("  open http://127.0.0.1:3000/console");
    }
    eprintln!();
    eprintln!("Install a remote module with `lenso module install <manifest-url>`.");
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
            "POSTGRES_HOST_PORT=4545\nDATABASE_URL=postgres://lenso:lenso@localhost:${POSTGRES_HOST_PORT}/lenso\n",
        )
        .unwrap();

        assert_eq!(
            dotenv_value(&target, "DATABASE_URL").as_deref(),
            Some("postgres://lenso:lenso@localhost:4545/lenso")
        );

        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn bootstrap_scope_merge_preserves_existing_scopes() {
        let mut grants =
            BTreeMap::from([("usr_admin".to_owned(), vec!["auth.users.read".to_owned()])]);
        merge_user_scopes(
            &mut grants,
            "usr_admin",
            &bootstrap_scopes(vec![
                "auth.users.read".to_owned(),
                "identity.users.read".to_owned(),
            ]),
        );

        assert_eq!(
            grants["usr_admin"],
            vec!["auth.users.read", "console.admin", "identity.users.read"]
        );
    }

    #[test]
    fn bootstrap_identifier_normalizes_email_only() {
        assert_eq!(
            normalize_identifier(" Ada@Example.COM ").unwrap(),
            "ada@example.com"
        );
        assert_eq!(
            normalize_identifier(" +8613800000000 ").unwrap(),
            "+8613800000000"
        );
    }

    #[test]
    fn console_line_reports_installed_or_update_command() {
        let target = temp_dir("lenso-cli-console-line");
        fs::create_dir_all(target.join(".lenso/console/dist")).unwrap();

        assert_eq!(
            console_line(&target, "http://127.0.0.1:3000"),
            "  Console: not installed. Run `lenso host update-console`."
        );

        fs::write(target.join(".lenso/console/dist/index.html"), "").unwrap();
        assert_eq!(
            console_line(&target, "http://127.0.0.1:3000"),
            "  Console: http://127.0.0.1:3000/console"
        );

        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn copies_embedded_console_tree() {
        let target = temp_dir("lenso-cli-console-copy");
        let _ = fs::remove_dir_all(&target);

        copy_embedded_dir(&CONSOLE_DIR, &target).unwrap();
        assert!(target.join(".keep").exists());

        fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn update_console_matches_packaged_asset_state() {
        let target = temp_dir("lenso-cli-console-update");
        fs::create_dir_all(&target).unwrap();

        let result = update_console(Some(&target));

        if CONSOLE_DIR.get_file("dist/index.html").is_some() {
            result.unwrap();
            assert!(target.join(".lenso/console/dist/index.html").exists());
            if CONSOLE_DIR.get_dir("dist/extensions/host").is_some() {
                assert!(
                    target
                        .join(".lenso/console/extensions/host/runtime-console-api.js")
                        .exists()
                );
            }
        } else {
            assert!(result.is_err());
        }
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
