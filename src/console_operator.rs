use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, IsTerminal, Read};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::{Client, Url, redirect::Policy};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Executor, Postgres, Transaction, postgres::PgPoolOptions};
use uuid::Uuid;

const CONSOLE_ADMIN_USER_SCOPES_KEY: &str = "auth.console_admin_user_scopes";
const RUNTIME_CONFIG_SERVICE: &str = "*";
const BOOTSTRAP_ACTOR: &str = "lenso-cli:console-operator-bootstrap";
const CONFIGURE_ACTOR: &str = "lenso-cli:console-operator-configure";
const BOOTSTRAP_LOCK: &str = "lenso-console:operator-bootstrap";
const MINIMUM_OPERATOR_SCOPES: &[&str] = &[
    "auth.sessions.read",
    "auth.sessions.revoke",
    "auth.users.manage",
    "auth.users.read",
    "auth_password.credentials.write",
    "console.admin",
    "console.artifacts.manage",
    "console.module.business.read",
    "console.module.business.write",
    "console.system-registry.read",
    "console.system-registry.revoke",
    "console.system.connect",
    "console.system.read",
    "runtime.stories.read",
];

#[derive(Debug, Clone)]
pub struct BootstrapOperatorOptions {
    pub console_root: Option<PathBuf>,
    pub console_url: Option<String>,
    pub env_file: Option<PathBuf>,
    pub password_file: Option<PathBuf>,
    pub password_stdin: bool,
    pub user_id: Option<String>,
    pub identifier: Option<String>,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ConfigureOperatorOptions {
    pub console_root: Option<PathBuf>,
    pub env_file: Option<PathBuf>,
    pub user_id: Option<String>,
    pub identifier: Option<String>,
    pub scopes: Vec<String>,
}

struct PasswordRegistration {
    console_url: Url,
    identifier: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct PasswordSessionResponse {
    user_id: String,
}

/// Bootstrap the first operator in an independent Lenso Console Service.
pub async fn bootstrap_operator(options: BootstrapOperatorOptions) -> Result<()> {
    let registration = password_registration(&options)?;
    let console_root = options
        .console_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let service_root = console_service_root(console_root);
    let database_url = console_database_url(&service_root, options.env_file.as_deref())?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("connect to the Lenso Console Service Store")?;
    verify_console_service_store(&pool).await?;

    let mut tx = pool.begin().await.context("begin operator bootstrap")?;
    lock_operator_bootstrap(&mut tx).await?;
    let old_value = load_operator_grants(&mut tx).await?;
    ensure_no_existing_operator(&decode_operator_grants(old_value.clone())?)?;

    let user_id = if let Some(registration) = registration {
        let user_id = register_password_user(registration).await?;
        resolve_operator_user_id(&mut tx, Some(user_id), None).await?
    } else {
        resolve_operator_user_id(&mut tx, options.user_id, options.identifier).await?
    };
    let scopes = operator_scopes(options.scopes);
    let stored = write_initial_operator(&mut tx, old_value, &user_id, &scopes).await?;
    tx.commit().await.context("commit operator bootstrap")?;

    eprintln!("Bootstrapped Lenso Console operator {user_id}.");
    eprintln!("Stored {CONSOLE_ADMIN_USER_SCOPES_KEY}: {stored}");
    eprintln!("Restart the Console API and Worker for the operator grant to apply.");
    Ok(())
}

/// Idempotently ensure that an existing Console user has the complete current
/// Operator scope set while preserving unrelated users and extra scopes.
pub async fn configure_operator(options: ConfigureOperatorOptions) -> Result<()> {
    let console_root = options
        .console_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let service_root = console_service_root(console_root);
    let database_url = console_database_url(&service_root, options.env_file.as_deref())?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("connect to the Lenso Console Service Store")?;
    verify_console_service_store(&pool).await?;

    let mut tx = pool.begin().await.context("begin operator configuration")?;
    lock_operator_bootstrap(&mut tx).await?;
    let old_value = load_operator_grants(&mut tx).await?;
    let mut grants = decode_operator_grants(old_value.clone())?;
    let user_id = resolve_operator_user_id(&mut tx, options.user_id, options.identifier).await?;
    let mut scopes = grants.remove(&user_id).unwrap_or_default();
    scopes.extend(operator_scopes(options.scopes));
    scopes.sort();
    scopes.dedup();
    grants.insert(user_id.clone(), scopes);
    let stored = write_operator_grants(&mut tx, old_value, grants, CONFIGURE_ACTOR).await?;
    tx.commit().await.context("commit operator configuration")?;

    eprintln!("Configured Lenso Console operator {user_id}.");
    eprintln!("Stored {CONSOLE_ADMIN_USER_SCOPES_KEY}: {stored}");
    eprintln!("Restart the Console API and Worker for the operator grant to apply.");
    Ok(())
}

fn console_database_url(service_root: &Path, env_file: Option<&Path>) -> Result<String> {
    if let Some(env_file) = env_file {
        return crate::host::database_url_from_path(env_file);
    }
    let service_env = service_root.join(".env");
    if service_env.is_file() {
        return crate::host::database_url_from_path(&service_env);
    }
    crate::host::database_url(service_root, None)
}

async fn verify_console_service_store(pool: &sqlx::PgPool) -> Result<()> {
    let registry = sqlx::query_scalar::<_, Option<String>>(
        "select to_regclass('console.managed_services')::text",
    )
    .fetch_one(pool)
    .await
    .context("inspect Console Service Store identity")?;
    if registry.as_deref() != Some("console.managed_services") {
        bail!(
            "target database is not a Lenso Console Service Store: mandatory System Registry state is missing"
        );
    }
    Ok(())
}

fn console_service_root(console_root: &Path) -> PathBuf {
    let service = console_root.join("service");
    if service.join("Cargo.toml").is_file() {
        service
    } else {
        console_root.to_path_buf()
    }
}

fn password_registration(
    options: &BootstrapOperatorOptions,
) -> Result<Option<PasswordRegistration>> {
    if options.password_file.is_some() && options.password_stdin {
        bail!("pass either --password-file or --password-stdin, not both");
    }
    if options.password_file.is_none() && !options.password_stdin && options.console_url.is_none() {
        return Ok(None);
    }
    if options.user_id.is_some() {
        bail!("--user-id cannot be combined with password-user creation");
    }
    let identifier = options
        .identifier
        .as_deref()
        .context("--identifier is required when creating the password user")?;
    let identifier = normalize_identifier(identifier)?;
    let console_url = secure_console_url(
        options
            .console_url
            .as_deref()
            .context("--console-url is required when creating the password user")?,
    )?;
    let password = read_password(options.password_file.as_deref(), options.password_stdin)?;
    Ok(Some(PasswordRegistration {
        console_url,
        identifier,
        password,
    }))
}

fn read_password(password_file: Option<&Path>, password_stdin: bool) -> Result<String> {
    if let Some(path) = password_file {
        return read_password_file(path);
    }
    if password_stdin {
        return read_password_stdin();
    }
    prompt_password()
}

fn prompt_password() -> Result<String> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        bail!(
            "interactive password input requires a terminal; use --password-stdin or --password-file"
        );
    }
    let password = rpassword::prompt_password("New Console operator password: ")
        .context("read Console operator password")?;
    let confirmation = rpassword::prompt_password("Confirm Console operator password: ")
        .context("confirm Console operator password")?;
    if password != confirmation {
        bail!("Console operator passwords do not match");
    }
    validate_password(password)
}

fn secure_console_url(value: &str) -> Result<Url> {
    let mut url = Url::parse(value).context("parse --console-url")?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("--console-url must not contain credentials");
    }
    let secure = url.scheme() == "https";
    let loopback = url.host_str().is_some_and(|host| {
        let host = host.trim_start_matches('[').trim_end_matches(']');
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if !secure && !(url.scheme() == "http" && loopback) {
        bail!("--console-url must use HTTPS unless it targets loopback");
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn read_password_file(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect password file {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("password file must be a regular file and not a symbolic link");
    }
    ensure_private_password_file(&metadata, path)?;
    let password = fs::read_to_string(path)
        .with_context(|| format!("read password file {}", path.display()))?;
    validate_password(strip_terminal_newline(password))
}

#[cfg(unix)]
fn ensure_private_password_file(metadata: &fs::Metadata, path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode() & 0o077 != 0 {
        bail!(
            "password file {} must not be readable or writable by group or others",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_password_file(_metadata: &fs::Metadata, _path: &Path) -> Result<()> {
    Ok(())
}

fn read_password_stdin() -> Result<String> {
    let mut password = String::new();
    io::stdin()
        .read_to_string(&mut password)
        .context("read password from stdin")?;
    validate_password(strip_terminal_newline(password))
}

fn strip_terminal_newline(mut password: String) -> String {
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    password
}

fn validate_password(password: String) -> Result<String> {
    if password.is_empty() {
        bail!("password input is empty");
    }
    Ok(password)
}

async fn register_password_user(registration: PasswordRegistration) -> Result<String> {
    let endpoint = registration
        .console_url
        .join("/v1/auth/password/register")
        .context("build Console password registration URL")?;
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .context("build Console Auth client")?;
    let response = client
        .post(endpoint)
        .json(&json!({
            "identifier": registration.identifier,
            "password": registration.password
        }))
        .send()
        .await
        .context("register password user through the Console Auth Module")?;
    if !response.status().is_success() {
        bail!(
            "Console Auth password registration failed with HTTP {}",
            response.status()
        );
    }
    let response = response
        .json::<PasswordSessionResponse>()
        .await
        .context("decode Console Auth password registration response")?;
    if response.user_id.trim().is_empty() {
        bail!("Console Auth returned an empty user id");
    }
    Ok(response.user_id)
}

async fn lock_operator_bootstrap(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("select pg_advisory_xact_lock(hashtext($1))")
        .bind(BOOTSTRAP_LOCK)
        .execute(&mut **tx)
        .await
        .context("fence concurrent operator bootstrap")?;
    Ok(())
}

async fn load_operator_grants(tx: &mut Transaction<'_, Postgres>) -> Result<Option<Value>> {
    sqlx::query_scalar::<_, Value>(
        "select value from config.setting_values where service = $1 and key = $2",
    )
    .bind(RUNTIME_CONFIG_SERVICE)
    .bind(CONSOLE_ADMIN_USER_SCOPES_KEY)
    .fetch_optional(&mut **tx)
    .await
    .context("load current Console operator grants")
}

async fn resolve_operator_user_id(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Option<String>,
    identifier: Option<String>,
) -> Result<String> {
    match (user_id, identifier) {
        (Some(_), Some(_)) => bail!("pass either --user-id or --identifier, not both"),
        (Some(user_id), None) => {
            let user_id = user_id.trim();
            let exists = sqlx::query_scalar::<_, String>("select id from auth.users where id = $1")
                .bind(user_id)
                .fetch_optional(&mut **tx)
                .await
                .context("check Console Auth user")?;
            exists.with_context(|| format!("Console Auth user `{user_id}` was not found"))
        }
        (None, Some(identifier)) => {
            let normalized = normalize_identifier(&identifier)?;
            sqlx::query_scalar::<_, String>(
                "select user_id from auth.identities where provider = 'password' and provider_subject = $1",
            )
            .bind(&normalized)
            .fetch_optional(&mut **tx)
            .await
            .with_context(|| format!("find Console password identity `{normalized}`"))?
            .with_context(|| format!("Console password identity `{normalized}` was not found"))
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

fn operator_scopes(scopes: Vec<String>) -> Vec<String> {
    let mut set = MINIMUM_OPERATOR_SCOPES
        .iter()
        .map(|scope| (*scope).to_owned())
        .collect::<BTreeSet<_>>();
    set.extend(
        scopes
            .into_iter()
            .map(|scope| scope.trim().to_owned())
            .filter(|scope| !scope.is_empty()),
    );
    set.into_iter().collect()
}

async fn write_initial_operator(
    tx: &mut Transaction<'_, Postgres>,
    old_value: Option<Value>,
    user_id: &str,
    scopes: &[String],
) -> Result<Value> {
    let grants = BTreeMap::from([(user_id.to_owned(), scopes.to_vec())]);
    write_operator_grants(tx, old_value, grants, BOOTSTRAP_ACTOR).await
}

async fn write_operator_grants(
    tx: &mut Transaction<'_, Postgres>,
    old_value: Option<Value>,
    grants: BTreeMap<String, Vec<String>>,
    actor: &str,
) -> Result<Value> {
    let next_value = serde_json::to_value(grants).context("encode Console operator grants")?;
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
    .bind(actor)
    .execute(&mut **tx)
    .await
    .context("write initial Console operator grant")?;
    tx.execute(
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
        .bind(actor),
    )
    .await
    .context("audit initial Console operator grant")?;
    Ok(next_value)
}

fn decode_operator_grants(value: Option<Value>) -> Result<BTreeMap<String, Vec<String>>> {
    serde_json::from_value(value.unwrap_or_else(|| json!({})))
        .context("decode auth.console_admin_user_scopes")
}

fn ensure_no_existing_operator(existing: &BTreeMap<String, Vec<String>>) -> Result<()> {
    if !existing.is_empty() {
        bail!(
            "the Lenso Console already has an operator; manage additional operators through the identity Module"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> BootstrapOperatorOptions {
        BootstrapOperatorOptions {
            console_root: None,
            console_url: None,
            env_file: None,
            password_file: None,
            password_stdin: false,
            user_id: None,
            identifier: Some("admin@example.com".to_owned()),
            scopes: Vec::new(),
        }
    }

    #[test]
    fn operator_scope_set_contains_console_minimum_and_extra_scopes() {
        assert_eq!(
            operator_scopes(vec![
                "console.system-registry.read".to_owned(),
                "runtime.stories.read".to_owned(),
            ]),
            [
                "auth.sessions.read",
                "auth.sessions.revoke",
                "auth.users.manage",
                "auth.users.read",
                "auth_password.credentials.write",
                "console.admin",
                "console.artifacts.manage",
                "console.module.business.read",
                "console.module.business.write",
                "console.system-registry.read",
                "console.system-registry.revoke",
                "console.system.connect",
                "console.system.read",
                "runtime.stories.read",
            ]
        );
    }

    #[test]
    fn operator_identifier_normalizes_email_only() {
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
    fn non_checkout_path_is_used_as_service_root() {
        let root = Path::new("/tmp/lenso-console");
        assert_eq!(console_service_root(root), root);
    }

    #[test]
    fn console_environment_file_is_the_default_database_authority() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lenso-console-operator-env-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".env"),
            "DATABASE_URL=postgres://console:secret@127.0.0.1:55433/console\n",
        )
        .unwrap();

        assert_eq!(
            console_database_url(&root, None).unwrap(),
            "postgres://console:secret@127.0.0.1:55433/console"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_existing_grants_fail_closed() {
        assert!(decode_operator_grants(Some(json!([]))).is_err());
    }

    #[test]
    fn existing_operator_prevents_repeated_bootstrap() {
        let existing =
            BTreeMap::from([("usr_existing".to_owned(), vec!["console.admin".to_owned()])]);
        let error = ensure_no_existing_operator(&existing).unwrap_err();
        assert!(error.to_string().contains("already has an operator"));
        assert!(ensure_no_existing_operator(&BTreeMap::new()).is_ok());
    }

    #[test]
    fn password_user_creation_requires_a_safe_exact_input() {
        let mut input = options();
        input.console_url = Some("http://console.example.com:3030".to_owned());
        input.password_stdin = true;
        let Err(error) = password_registration(&input) else {
            panic!("remote plaintext HTTP must be rejected");
        };
        assert!(error.to_string().contains("HTTPS"));

        input.console_url = Some("https://console.example.com".to_owned());
        input.user_id = Some("usr_existing".to_owned());
        let Err(error) = password_registration(&input) else {
            panic!("password-user creation with --user-id must be rejected");
        };
        assert!(error.to_string().contains("--user-id"));
    }

    #[test]
    fn console_url_allows_https_and_loopback_only() {
        assert!(secure_console_url("https://console.example.com").is_ok());
        assert!(secure_console_url("http://127.0.0.1:3030").is_ok());
        assert!(secure_console_url("http://[::1]:3030").is_ok());
        assert!(secure_console_url("http://console.example.com").is_err());
        assert!(secure_console_url("https://user:secret@console.example.com").is_err());
    }

    #[test]
    fn password_newline_removal_preserves_other_characters() {
        assert_eq!(
            strip_terminal_newline(" secret value \r\n".to_owned()),
            " secret value "
        );
        assert_eq!(strip_terminal_newline("secret  ".to_owned()), "secret  ");
    }

    #[cfg(unix)]
    #[test]
    fn password_file_requires_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lenso-console-operator-password-{}-{nonce}",
            std::process::id()
        ));
        fs::write(&path, "strong password\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_password_file(&path).unwrap(), "strong password");

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_password_file(&path).is_err());
        fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn password_registration_uses_console_auth_without_returning_the_session_secret() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 2048];
            loop {
                let read = stream.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..read]);
                let request_text = String::from_utf8_lossy(&request);
                let Some((headers, body)) = request_text.split_once("\r\n\r\n") else {
                    continue;
                };
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(str::trim)
                            .and_then(|value| value.parse::<usize>().ok())
                    })
                    .unwrap();
                if body.len() >= content_length {
                    assert!(headers.starts_with("POST /v1/auth/password/register HTTP/1.1"));
                    assert!(body.contains("admin@example.com"));
                    assert!(body.contains("strong-password"));
                    break;
                }
            }
            let body = r#"{"user_id":"usr_console","token":"session-secret","expires_at":"2026-07-30T00:00:00Z"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });

        let user_id = register_password_user(PasswordRegistration {
            console_url: Url::parse(&format!("http://{address}")).unwrap(),
            identifier: "admin@example.com".to_owned(),
            password: "strong-password".to_owned(),
        })
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(user_id, "usr_console");
    }
}
