use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::postgres::PgPoolOptions;

const PLAN_PROTOCOL: &str = "lenso.console-composition-plan.v1";
const COMPOSITION_PROTOCOL: &str = "lenso.console-service-composition.v2";

#[derive(Debug, Clone)]
pub struct PlanOptions {
    pub composition_file: PathBuf,
    pub env_file: PathBuf,
    pub json: bool,
    pub output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    pub approve_plan_digest: String,
    pub env_file: PathBuf,
    pub plan_file: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompositionPlan {
    protocol: String,
    current_revision: i64,
    target_revision: i64,
    composition_digest: String,
    composition: Value,
    approval_boundaries: Vec<String>,
    plan_digest: String,
}

pub async fn plan(options: PlanOptions) -> Result<()> {
    let print_json = options.json || options.output.is_none();
    let composition = read_json(&options.composition_file)?;
    validate_composition(&composition)?;
    let pool = connect(&options.env_file).await?;
    let current_revision = current_revision(&pool).await?;
    let composition_digest = digest(&composition)?;
    let mut plan = CompositionPlan {
        protocol: PLAN_PROTOCOL.to_owned(),
        current_revision,
        target_revision: current_revision
            .checked_add(1)
            .context("Console composition revision overflow")?,
        composition_digest,
        approval_boundaries: approval_boundaries(&composition),
        composition,
        plan_digest: String::new(),
    };
    plan.plan_digest = plan_digest(&plan)?;
    let rendered = serde_json::to_string_pretty(&plan)? + "\n";
    if let Some(output) = options.output {
        write_new(&output, &rendered)?;
        eprintln!("Wrote Console composition plan to {}.", output.display());
    }
    if print_json {
        print!("{rendered}");
    } else {
        eprintln!("Console composition plan: {}", plan.plan_digest);
        eprintln!("Current revision: {}", plan.current_revision);
        eprintln!("Target revision: {}", plan.target_revision);
        for boundary in &plan.approval_boundaries {
            eprintln!("Approval boundary: {boundary}");
        }
    }
    Ok(())
}

pub async fn apply(options: ApplyOptions) -> Result<()> {
    let plan: CompositionPlan = serde_json::from_value(read_json(&options.plan_file)?)
        .context("decode Console composition plan")?;
    if plan.protocol != PLAN_PROTOCOL {
        bail!(
            "unsupported Console composition plan protocol: {}",
            plan.protocol
        );
    }
    validate_composition(&plan.composition)?;
    let calculated = plan_digest(&plan)?;
    if calculated != plan.plan_digest || calculated != options.approve_plan_digest {
        bail!("Console composition plan digest was not approved exactly");
    }
    if digest(&plan.composition)? != plan.composition_digest {
        bail!("Console composition payload digest does not match the plan");
    }

    let pool = connect(&options.env_file).await?;
    let mut tx = pool.begin().await.context("begin composition apply")?;
    sqlx::query("select pg_advisory_xact_lock(hashtext('lenso-console:composition'))")
        .execute(&mut *tx)
        .await
        .context("lock Console composition")?;
    let observed = sqlx::query_scalar::<_, i64>(
        "select revision from console.service_composition where singleton = true for update",
    )
    .fetch_optional(&mut *tx)
    .await
    .context("read Console composition revision")?
    .unwrap_or(0);
    if observed != plan.current_revision {
        bail!(
            "Console composition changed after planning: expected revision {}, observed {observed}",
            plan.current_revision
        );
    }
    sqlx::query(
        "insert into console.service_composition \
         (singleton, revision, composition_digest, document, updated_at) \
         values (true, $1, $2, $3, now()) \
         on conflict (singleton) do update set revision = excluded.revision, \
         composition_digest = excluded.composition_digest, document = excluded.document, \
         updated_at = excluded.updated_at",
    )
    .bind(plan.target_revision)
    .bind(&plan.composition_digest)
    .bind(&plan.composition)
    .execute(&mut *tx)
    .await
    .context("persist Console composition")?;
    sqlx::query(
        "insert into console.composition_history \
         (revision, plan_digest, composition_digest, document) values ($1, $2, $3, $4)",
    )
    .bind(plan.target_revision)
    .bind(&plan.plan_digest)
    .bind(&plan.composition_digest)
    .bind(&plan.composition)
    .execute(&mut *tx)
    .await
    .context("append Console composition history")?;
    tx.commit().await.context("commit Console composition")?;
    eprintln!(
        "Applied Console composition revision {} ({})",
        plan.target_revision, plan.composition_digest
    );
    Ok(())
}

async fn connect(env_file: &Path) -> Result<sqlx::PgPool> {
    let database_url = crate::host::database_url_from_path(env_file)?;
    PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await
        .context("connect to the Lenso Console Service Store")
}

async fn current_revision(pool: &sqlx::PgPool) -> Result<i64> {
    sqlx::query_scalar("select revision from console.service_composition where singleton = true")
        .fetch_optional(pool)
        .await
        .context("read Console composition; run current Console migrations first")
        .map(|revision| revision.unwrap_or(0))
}

fn validate_composition(value: &Value) -> Result<()> {
    if value.get("schema").and_then(Value::as_str) != Some(COMPOSITION_PROTOCOL) {
        bail!("composition.schema must be {COMPOSITION_PROTOCOL}");
    }
    if value.get("serviceId").and_then(Value::as_str) != Some("lenso-console") {
        bail!("composition.serviceId must be lenso-console");
    }
    if value.get("status").and_then(Value::as_str) != Some("ready") {
        bail!("composition.status must be ready before it can be applied");
    }
    if value.get("workloadMode").and_then(Value::as_str) != Some("normal") {
        bail!("composition.workloadMode must be normal");
    }
    if value
        .get("issues")
        .and_then(Value::as_array)
        .is_none_or(|issues| !issues.is_empty())
    {
        bail!("composition.issues must be an empty array");
    }
    let modules = value
        .get("modules")
        .and_then(Value::as_array)
        .context("composition.modules must be an array")?;
    for role in ["identity", "system_registry"] {
        let count = modules
            .iter()
            .filter(|module| module.get("role").and_then(Value::as_str) == Some(role))
            .count();
        if count != 1 {
            bail!("mandatory Console role {role} must have exactly one Module binding");
        }
    }
    for module in modules {
        let module_id = module
            .get("moduleId")
            .and_then(Value::as_str)
            .context("every composition Module requires moduleId")?;
        if module_id.trim().is_empty() {
            bail!("composition Module id cannot be empty");
        }
        let kind = module
            .get("kind")
            .and_then(Value::as_str)
            .context("every composition Module requires kind")?;
        if !matches!(kind, "shell" | "mandatory" | "optional") {
            bail!("composition Module kind must be shell, mandatory, or optional");
        }
        if module.get("role").is_some() && kind != "mandatory" {
            bail!("only mandatory composition Modules may bind a mandatory role");
        }
        let release_digest = module
            .get("moduleReleaseDigest")
            .and_then(Value::as_str)
            .context("every composition Module requires moduleReleaseDigest")?;
        require_sha256("moduleReleaseDigest", release_digest)?;
        if let Some(ui_digest) = module.get("uiArtifactDigest").and_then(Value::as_str) {
            require_sha256("uiArtifactDigest", ui_digest)?;
        }
        if let Some(delivery) = module.get("delivery").and_then(Value::as_str)
            && !matches!(delivery, "linked" | "service")
        {
            bail!("composition Module delivery must be linked or service");
        }
        if module
            .get("grantedPermissions")
            .and_then(Value::as_array)
            .is_none_or(|permissions| !permissions.iter().all(Value::is_string))
        {
            bail!("grantedPermissions must be an array of permission ids");
        }
        let ui_base_url = module.get("uiArtifactBaseUrl").and_then(Value::as_str);
        let ui_entries = module.get("uiEntries").and_then(Value::as_array);
        if ui_base_url.is_some() || ui_entries.is_some() {
            if module
                .get("uiArtifactDigest")
                .and_then(Value::as_str)
                .is_none()
            {
                bail!("Console UI entries require uiArtifactDigest");
            }
            let base_url = ui_base_url.context("Console UI entries require uiArtifactBaseUrl")?;
            let parsed =
                reqwest::Url::parse(base_url).context("uiArtifactBaseUrl must be a URL")?;
            if parsed.scheme() != "https"
                && !(parsed.scheme() == "http"
                    && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1")))
            {
                bail!("uiArtifactBaseUrl must use HTTPS outside loopback development");
            }
            let entries = ui_entries.context("uiArtifactBaseUrl requires uiEntries")?;
            if entries.is_empty() {
                bail!("uiEntries cannot be empty when a Console UI artifact is configured");
            }
            for entry in entries {
                for field in ["name", "label", "route", "path"] {
                    if entry
                        .get(field)
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                    {
                        bail!("every Console UI entry requires {field}");
                    }
                }
                let entry_path = entry["path"].as_str().expect("validated entry path");
                if entry_path.starts_with('/') || entry_path.split('/').any(|part| part == "..") {
                    bail!("Console UI entry path must stay inside the UI artifact");
                }
            }
        }
    }
    Ok(())
}

fn approval_boundaries(value: &Value) -> Vec<String> {
    let mut boundaries = Vec::new();
    if let Some(modules) = value.get("modules").and_then(Value::as_array) {
        if modules.iter().any(|module| {
            module
                .get("grantedPermissions")
                .and_then(Value::as_array)
                .is_some_and(|permissions| !permissions.is_empty())
        }) {
            boundaries.push("console_permission_grant".to_owned());
        }
        if modules
            .iter()
            .any(|module| module.get("delivery").and_then(Value::as_str) == Some("linked"))
        {
            boundaries.push("linked_module_requires_console_service_release".to_owned());
        }
    }
    boundaries
}

fn require_sha256(field: &str, value: &str) -> Result<()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        bail!("{field} must be a sha256 digest");
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{field} must be a sha256 digest");
    }
    Ok(())
}

fn plan_digest(plan: &CompositionPlan) -> Result<String> {
    let mut unsigned = serde_json::to_value(plan)?;
    unsigned["planDigest"] = Value::String(String::new());
    digest(&unsigned)
}

fn digest(value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec(&canonical_json(value)).context("canonicalize JSON")?;
    let digest = Sha256::digest(bytes);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("sha256:{hex}"))
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let ordered = object
                .iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>();
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        _ => value.clone(),
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let source =
        fs::read_to_string(path).with_context(|| format!("read JSON from {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("parse JSON from {}", path.display()))
}

fn write_new(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        bail!("refusing to overwrite {}", path.display());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create directory {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn composition_requires_exact_mandatory_roles_and_release_digests() {
        let digest = format!("sha256:{}", "a".repeat(64));
        let composition = json!({
            "schema": COMPOSITION_PROTOCOL,
            "serviceId": "lenso-console",
            "status": "ready",
            "workloadMode": "normal",
            "issues": [],
            "modules": [
                {"moduleId": "auth", "kind": "mandatory", "moduleReleaseDigest": digest, "role": "identity", "grantedPermissions": []},
                {"moduleId": "registry", "kind": "mandatory", "moduleReleaseDigest": digest, "role": "system_registry", "grantedPermissions": []}
            ]
        });
        validate_composition(&composition).expect("valid composition");
    }
}
