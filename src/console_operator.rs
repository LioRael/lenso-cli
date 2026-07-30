use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

const CONSOLE_ADMIN_USER_SCOPES_KEY: &str = "auth.console_admin_user_scopes";
const RUNTIME_CONFIG_SERVICE: &str = "*";
const BOOTSTRAP_ACTOR: &str = "lenso-cli:console-operator-bootstrap";
const BOOTSTRAP_LOCK: &str = "lenso-console:operator-bootstrap";
const MINIMUM_OPERATOR_SCOPES: &[&str] = &[
    "auth.users.read",
    "auth_password.credentials.write",
    "console.admin",
    "console.system-registry.read",
    "console.system-registry.revoke",
];

#[derive(Debug, Clone)]
pub struct BootstrapOperatorOptions {
    pub console_root: Option<PathBuf>,
    pub env_file: Option<PathBuf>,
    pub user_id: Option<String>,
    pub identifier: Option<String>,
    pub scopes: Vec<String>,
}

/// Bootstrap the first operator in an independent Lenso Console Service.
pub async fn bootstrap_operator(options: BootstrapOperatorOptions) -> Result<()> {
    let console_root = options
        .console_root
        .as_deref()
        .unwrap_or_else(|| Path::new("."));
    let service_root = console_service_root(console_root);
    let database_url = crate::host::database_url(&service_root, options.env_file.as_deref())?;
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("connect to the Lenso Console Service Store")?;
    verify_console_service_store(&pool).await?;
    let user_id = resolve_operator_user_id(&pool, options.user_id, options.identifier).await?;
    let scopes = operator_scopes(options.scopes);
    let stored = store_initial_operator(&pool, &user_id, &scopes).await?;

    eprintln!("Bootstrapped Lenso Console operator {user_id}.");
    eprintln!("Stored {CONSOLE_ADMIN_USER_SCOPES_KEY}: {stored}");
    eprintln!("Restart the Console API and Worker for the operator grant to apply.");
    Ok(())
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

async fn resolve_operator_user_id(
    pool: &sqlx::PgPool,
    user_id: Option<String>,
    identifier: Option<String>,
) -> Result<String> {
    match (user_id, identifier) {
        (Some(_), Some(_)) => bail!("pass either --user-id or --identifier, not both"),
        (Some(user_id), None) => {
            let user_id = user_id.trim();
            let exists = sqlx::query_scalar::<_, String>("select id from auth.users where id = $1")
                .bind(user_id)
                .fetch_optional(pool)
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
            .fetch_optional(pool)
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

async fn store_initial_operator(
    pool: &sqlx::PgPool,
    user_id: &str,
    scopes: &[String],
) -> Result<Value> {
    let mut tx = pool.begin().await.context("begin operator bootstrap")?;
    sqlx::query("select pg_advisory_xact_lock(hashtext($1))")
        .bind(BOOTSTRAP_LOCK)
        .execute(&mut *tx)
        .await
        .context("fence concurrent operator bootstrap")?;

    let old_value = sqlx::query_scalar::<_, Value>(
        "select value from config.setting_values where service = $1 and key = $2",
    )
    .bind(RUNTIME_CONFIG_SERVICE)
    .bind(CONSOLE_ADMIN_USER_SCOPES_KEY)
    .fetch_optional(&mut *tx)
    .await
    .context("load current Console operator grants")?;
    let existing = decode_operator_grants(old_value.clone())?;
    ensure_no_existing_operator(&existing)?;

    let grants = BTreeMap::from([(user_id.to_owned(), scopes.to_vec())]);
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
    .bind(BOOTSTRAP_ACTOR)
    .execute(&mut *tx)
    .await
    .context("write initial Console operator grant")?;
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
    .context("audit initial Console operator grant")?;
    tx.commit().await.context("commit operator bootstrap")?;
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

    #[test]
    fn operator_scope_set_contains_console_minimum_and_extra_scopes() {
        assert_eq!(
            operator_scopes(vec![
                "console.system-registry.read".to_owned(),
                "runtime.stories.read".to_owned(),
            ]),
            [
                "auth.users.read",
                "auth_password.credentials.write",
                "console.admin",
                "console.system-registry.read",
                "console.system-registry.revoke",
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
}
