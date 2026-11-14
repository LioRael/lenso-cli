use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lenso_cli::{Context, Diagnostic, DiagnosticCode, Error};
use lenso_runtime::ProcessSpec;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Configuration reads remain within the ordinary Host resource budget.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
/// Bound the number/size of explicitly inherited environment values and redaction patterns.
const MAX_ENV_VALUE_BYTES: usize = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    schema_version: u32,
    pub workflows: BTreeMap<String, Workflow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Workflow {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: PathBuf,
    #[serde(default)]
    pub env: Vec<String>,
    pub timeout_ms: u64,
    pub readiness_timeout_ms: u64,
    pub readiness: Readiness,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Readiness {
    Http { url: String, status: u16 },
    Exit { code: u8 },
}

#[derive(Debug, Serialize)]
pub(crate) struct Health {
    pub endpoint: String,
    pub state: &'static str,
    pub checked_at_ms: u128,
    pub status: Option<u16>,
    pub reason: Option<&'static str>,
}

pub(crate) fn invalid(field: &str, reason: &str) -> Error {
    Diagnostic::new(DiagnosticCode::InvalidInput)
        .with_details(json!({"field":field,"reason":reason}))
        .with_retryable(false)
        .into()
}

pub(crate) fn selection(ctx: &Context) -> Result<Option<&str>, Error> {
    ctx.input
        .get("workflow")
        .or_else(|| ctx.answers.get("ops:workflow"))
        .map(|value| {
            value
                .as_str()
                .filter(|value| !value.is_empty())
                .ok_or_else(|| invalid("workflow", "expected_nonempty_name"))
        })
        .transpose()
}

pub(crate) fn load(ctx: &Context) -> Result<Config, Error> {
    let path = Path::new("lenso.ops.toml");
    let exact: Vec<PathBuf> = grant(ctx, "host.reads")?;
    let roots: Vec<PathBuf> = grant(ctx, "host.read_roots")?;
    if !lenso_runtime::path_allowed(path, &exact, &roots) {
        return Err(Diagnostic::missing_capability("host.reads").into());
    }
    let path = lenso_runtime::project_path(&ctx.project, path)?;
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(invalid("lenso.ops.toml", "size_limit"));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("lenso.ops.toml", "invalid_utf8"))?;
    let config: Config =
        toml::from_str(text).map_err(|_| invalid("lenso.ops.toml", "invalid_config"))?;
    if config.schema_version != 1 || config.workflows.is_empty() || config.workflows.len() > 64 {
        return Err(invalid(
            "lenso.ops.toml",
            "unsupported_schema_or_workflow_count",
        ));
    }
    for (name, workflow) in &config.workflows {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(invalid("workflows", "invalid_name"));
        }
        workflow.validate()?;
    }
    Ok(config)
}

impl Config {
    pub fn question(&self) -> lenso_cli::Outcome {
        lenso_cli::Outcome::NeedsInput {
            question: lenso_cli::Question {
                id: "ops:workflow".into(),
                message: "Choose a declared Ops workflow".into(),
                kind: lenso_cli::QuestionKind::Select {
                    options: self.workflows.keys().cloned().collect(),
                },
            },
        }
    }
    pub fn workflow(&self, name: &str) -> Result<&Workflow, Error> {
        self.workflows
            .get(name)
            .ok_or_else(|| invalid("workflow", "unknown_workflow"))
    }
}

impl Workflow {
    fn validate(&self) -> Result<(), Error> {
        if self.program.is_empty()
            || self.program.contains('\0')
            || self.args.len() > 128
            || self.args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(invalid("program", "invalid_program_or_arguments"));
        }
        if !(10..=86_400_000).contains(&self.timeout_ms)
            || self.readiness_timeout_ms == 0
            || self.readiness_timeout_ms > self.timeout_ms
        {
            return Err(invalid("timeout_ms", "invalid_deadline"));
        }
        let mut seen = BTreeSet::new();
        if self.env.len() > 32
            || self.env.iter().any(|name| {
                name.is_empty()
                    || name.len() > 128
                    || name.starts_with(|character: char| character.is_ascii_digit())
                    || !name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    || !seen.insert(name)
            })
        {
            return Err(invalid("env", "invalid_or_duplicate_name"));
        }
        if let Readiness::Http { url, status } = &self.readiness {
            endpoint(url)?;
            if !(200..300).contains(status) {
                return Err(invalid("readiness.status", "expected_2xx"));
            }
        }
        Ok(())
    }

    pub fn process(&self) -> ProcessSpec {
        ProcessSpec {
            program: self.program.clone(),
            args: self.args.clone(),
        }
    }

    pub fn directory(&self, ctx: &Context) -> Result<PathBuf, Error> {
        let path = if self.cwd == Path::new(".") {
            ctx.project.clone()
        } else {
            lenso_runtime::project_path(&ctx.project, &self.cwd)?
        };
        if !path.is_dir() {
            return Err(invalid("cwd", "directory_missing"));
        }
        Ok(path)
    }

    pub fn executable(&self, ctx: &Context) -> Result<PathBuf, Error> {
        let program = Path::new(&self.program);
        let candidates = if program.is_absolute() {
            vec![program.to_owned()]
        } else if program.components().count() > 1 {
            vec![self.directory(ctx)?.join(program)]
        } else {
            std::env::var_os("PATH")
                .map(|value| {
                    std::env::split_paths(&value)
                        .map(|path| path.join(program))
                        .collect()
                })
                .unwrap_or_default()
        };
        candidates
            .into_iter()
            .find(|path| {
                let Ok(metadata) = std::fs::metadata(path) else {
                    return false;
                };
                if !metadata.is_file() {
                    return false;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    metadata.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    true
                }
            })
            .ok_or_else(|| invalid("program", "executable_missing"))
    }

    pub fn environment(&self, ctx: &Context) -> Result<BTreeMap<String, String>, Error> {
        let allowed: Vec<String> = grant(ctx, "host.env")?;
        let mut environment = BTreeMap::new();
        for name in &self.env {
            if !allowed.contains(name) {
                return Err(Diagnostic::missing_capability("host.env").into());
            }
            let value =
                std::env::var(name).map_err(|_| invalid("env", "variable_missing_or_not_utf8"))?;
            if value.is_empty() || value.len() > MAX_ENV_VALUE_BYTES {
                return Err(invalid("env", "empty_or_oversized_value"));
            }
            environment.insert(name.clone(), value);
        }
        Ok(environment)
    }
}

pub(crate) fn grant<T: serde::de::DeserializeOwned + Default>(
    ctx: &Context,
    name: &str,
) -> Result<T, Error> {
    ctx.capabilities
        .get(name)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map(|value| value.unwrap_or_default())
        .map_err(|_| invalid("capabilities", "invalid_host_grant"))
}

pub(crate) fn endpoint(url: &str) -> Result<SocketAddr, Error> {
    let url = reqwest::Url::parse(url).map_err(|_| invalid("readiness.url", "invalid_url"))?;
    let address: IpAddr = url
        .host_str()
        .and_then(|host| host.trim_matches(['[', ']']).parse().ok())
        .filter(IpAddr::is_loopback)
        .ok_or_else(|| invalid("readiness.url", "numeric_loopback_required"))?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("readiness.url", "plain_loopback_http_required"));
    }
    let port = url
        .port_or_known_default()
        .filter(|port| *port != 0)
        .ok_or_else(|| invalid("readiness.url", "invalid_port"))?;
    Ok(SocketAddr::new(address, port))
}

fn check_http_grant(ctx: &Context, url: &str) -> Result<(), Error> {
    let allowed: Vec<String> = grant(ctx, "host.http")?;
    if !lenso_runtime::http_allowed(url, &allowed) {
        return Err(Diagnostic::missing_capability("host.http").into());
    }
    Ok(())
}

pub(crate) async fn preflight(ctx: &Context, workflow: &Workflow) -> Vec<Value> {
    let mut issues = Vec::new();
    if !cfg!(unix) {
        issues.push(json!({"reason":"unix_process_groups_required"}));
    }
    for result in [
        workflow.directory(ctx).map(|_| ()),
        workflow.executable(ctx).map(|_| ()),
        workflow.environment(ctx).map(|_| ()),
    ] {
        if let Err(error) = result {
            issues.push(json!(error.diagnostic()));
        }
    }
    match grant::<Vec<ProcessSpec>>(ctx, "host.processes") {
        Ok(allowed) if allowed.contains(&workflow.process()) => {}
        _ => issues.push(json!(Diagnostic::missing_capability("host.processes"))),
    }
    if let Readiness::Http { url, .. } = &workflow.readiness {
        if let Err(error) = check_http_grant(ctx, url) {
            issues.push(json!(error.diagnostic()));
        }
        if let Ok(address) = endpoint(url) {
            match tokio::net::TcpListener::bind(address).await {
                Ok(listener) => drop(listener),
                Err(_) => issues.push(json!({"field":"readiness.url","reason":"port_unavailable"})),
            }
        }
    }
    issues
}

pub(crate) fn health_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_millis(250))
        .build()
        .map_err(|_| invalid("readiness", "http_client_unavailable"))
}

pub(crate) async fn probe(
    ctx: &Context,
    client: &reqwest::Client,
    readiness: &Readiness,
) -> Result<Health, Error> {
    let Readiness::Http {
        url,
        status: expected,
    } = readiness
    else {
        return Err(invalid("readiness", "http_required"));
    };
    endpoint(url)?;
    check_http_grant(ctx, url)?;
    let (state, status, reason) = match client.get(url).send().await {
        Ok(response) if response.status().as_u16() == *expected => ("ready", Some(*expected), None),
        Ok(response) => (
            "not_ready",
            Some(response.status().as_u16()),
            Some("unexpected_http_status"),
        ),
        Err(error) if error.is_timeout() => ("not_ready", None, Some("http_timeout")),
        Err(_) => ("not_ready", None, Some("connection_failed")),
    };
    Ok(Health {
        endpoint: url.clone(),
        state,
        checked_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        status,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_endpoints_require_plain_numeric_loopback_without_credentials() {
        assert!(endpoint("http://127.0.0.1:8000/health").is_ok());
        assert!(endpoint("http://[::1]:8000/health").is_ok());
        for url in [
            "http://localhost/",
            "http://192.0.2.1/",
            "https://127.0.0.1/",
            "http://user:private@127.0.0.1/",
            "http://127.0.0.1/?secret=value",
            "http://127.0.0.1:0/",
        ] {
            let error = endpoint(url).unwrap_err();
            assert_eq!(error.diagnostic().code(), DiagnosticCode::InvalidInput);
            assert!(!json!(error.diagnostic()).to_string().contains("private"));
        }
    }

    #[test]
    fn config_read_grants_and_symlink_guards_precede_parsing_and_errors_are_public() {
        let root = tempfile::tempdir().unwrap();
        let mut ctx = Context {
            events: Default::default(),
            abi_version: 1,
            project: root.path().canonicalize().unwrap(),
            input: json!({}),
            answers: Default::default(),
            steps: Default::default(),
            hooks: Default::default(),
            capabilities: Default::default(),
        };
        std::fs::write(
            root.path().join("lenso.ops.toml"),
            "private-secret = broken syntax",
        )
        .unwrap();
        assert_eq!(
            load(&ctx).unwrap_err().diagnostic().code(),
            DiagnosticCode::PermissionDenied
        );
        ctx.capabilities
            .insert("host.reads".into(), json!(["lenso.ops.toml"]));
        let error = load(&ctx).unwrap_err();
        assert_eq!(error.diagnostic().code(), DiagnosticCode::InvalidInput);
        assert!(
            !json!(error.diagnostic())
                .to_string()
                .contains("private-secret")
        );
        #[cfg(unix)]
        {
            std::fs::remove_file(root.path().join("lenso.ops.toml")).unwrap();
            let outside = tempfile::NamedTempFile::new().unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join("lenso.ops.toml")).unwrap();
            assert!(load(&ctx).is_err());
        }
    }
}
