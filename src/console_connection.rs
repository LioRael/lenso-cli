use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, Url, redirect::Policy};
use serde::Deserialize;
use serde_json::Value;

const CONNECT_PROTOCOL: &str = "lenso.console-connect.v1";

#[derive(Debug, Clone)]
pub struct ConnectOptions {
    pub bundle: PathBuf,
    pub console_url: String,
    pub json: bool,
    pub token_env: String,
    pub token_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ConnectBundle {
    protocol: String,
    #[serde(default)]
    enrollment_receipts: Vec<Value>,
    #[serde(default)]
    artifact_composition: Option<Value>,
    system_connection: Value,
}

pub async fn connect(options: ConnectOptions) -> Result<()> {
    let bundle: ConnectBundle =
        serde_json::from_slice(&fs::read(&options.bundle).with_context(|| {
            format!(
                "read Console connection bundle {}",
                options.bundle.display()
            )
        })?)
        .context("decode Console connection bundle")?;
    validate_bundle(&bundle)?;
    let console_url = secure_console_url(&options.console_url)?;
    let token = operator_token(options.token_file.as_deref(), &options.token_env)?;
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .context("build Console connection client")?;

    let inventory = request_json(
        &client,
        &console_url,
        "/api/console/v1/services",
        &token,
        None,
    )
    .await?;
    let enrolled = inventory
        .as_array()
        .context("Console Service inventory is not an array")?
        .iter()
        .filter_map(|service| service.get("serviceId")?.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for receipt in &bundle.enrollment_receipts {
        let service_id = receipt
            .pointer("/receipt/managedServiceId")
            .and_then(Value::as_str)
            .context("enrollment receipt managedServiceId is required")?;
        if enrolled.contains(service_id) {
            eprintln!("Enrollment reused: {service_id}");
            continue;
        }
        request_json(
            &client,
            &console_url,
            "/api/console/v1/enrollment-receipts",
            &token,
            Some(receipt),
        )
        .await
        .with_context(|| format!("register signed enrollment for {service_id}"))?;
        eprintln!("Enrollment registered: {service_id}");
    }
    if let Some(composition) = &bundle.artifact_composition {
        request_json(
            &client,
            &console_url,
            "/api/console/v1/artifacts/reconcile",
            &token,
            Some(composition),
        )
        .await
        .context("reconcile exact Console UI artifacts")?;
        eprintln!("Console UI artifacts reconciled.");
    }
    let connection = request_json(
        &client,
        &console_url,
        "/api/console/v1/system/connect",
        &token,
        Some(&bundle.system_connection),
    )
    .await
    .context("connect exact System topology")?;
    if connection.get("status").and_then(Value::as_str) != Some("connected") {
        bail!("Console returned a non-connected System projection");
    }
    if options.json {
        println!("{}", serde_json::to_string_pretty(&connection)?);
    } else {
        eprintln!(
            "System connected: {}",
            connection
                .get("systemId")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    Ok(())
}

fn validate_bundle(bundle: &ConnectBundle) -> Result<()> {
    if bundle.protocol != CONNECT_PROTOCOL {
        bail!("Console connection bundle protocol must be {CONNECT_PROTOCOL}");
    }
    for receipt in &bundle.enrollment_receipts {
        for pointer in [
            "/offer/signature/subjectDigest",
            "/receipt/signature/subjectDigest",
            "/receipt/managedServiceId",
        ] {
            if receipt.pointer(pointer).and_then(Value::as_str).is_none() {
                bail!("Console connection enrollment is missing {pointer}");
            }
        }
    }
    if let Some(composition) = &bundle.artifact_composition
        && (composition.get("kind").and_then(Value::as_str) != Some("console_composition")
            || composition
                .get("candidate_lock_digest")
                .or_else(|| composition.get("candidateLockDigest"))
                .and_then(Value::as_str)
                .is_none())
    {
        bail!("artifactComposition must be an exact console_composition effect");
    }
    for field in [
        "systemId",
        "topologyDigest",
        "topology",
        "managementBinding",
    ] {
        if bundle.system_connection.get(field).is_none() {
            bail!("systemConnection.{field} is required");
        }
    }
    Ok(())
}

fn secure_console_url(value: &str) -> Result<Url> {
    let mut url = Url::parse(value).context("parse --console-url")?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("--console-url must not contain credentials");
    }
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        bail!("--console-url must use HTTPS unless it targets loopback");
    }
    url.set_path("/");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn operator_token(path: Option<&Path>, env_name: &str) -> Result<String> {
    let token = if let Some(path) = path {
        private_token_file(path)?
    } else {
        std::env::var(env_name)
            .with_context(|| format!("read Console operator token from {env_name}"))?
    };
    let token = token.trim().to_owned();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        bail!("Console operator token must be one non-empty bearer value");
    }
    Ok(token)
}

fn private_token_file(path: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect token file {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("token file must be a regular file and not a symbolic link");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("token file must not be readable or writable by group or others");
        }
    }
    fs::read_to_string(path).with_context(|| format!("read token file {}", path.display()))
}

async fn request_json(
    client: &Client,
    base: &Url,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> Result<Value> {
    let url = base.join(path).context("build Console API URL")?;
    let request = if let Some(body) = body {
        client.post(url).bearer_auth(token).json(body)
    } else {
        client.get(url).bearer_auth(token)
    };
    let response = request.send().await.context("call Console API")?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .context("read Console API response")?;
    if !status.is_success() {
        let detail = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| {
                value
                    .get("detail")
                    .or_else(|| value.get("title"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| {
                status
                    .canonical_reason()
                    .unwrap_or("request failed")
                    .to_owned()
            });
        bail!("Console API returned {}: {detail}", status.as_u16());
    }
    if bytes.is_empty() || status == StatusCode::NO_CONTENT {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&bytes).context("decode Console API response")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exact_bundle_requires_the_three_connection_boundaries() {
        let bundle: ConnectBundle = serde_json::from_value(json!({
            "protocol": CONNECT_PROTOCOL,
            "enrollmentReceipts": [],
            "artifactComposition": {
                "kind": "console_composition",
                "candidate_lock_digest": "sha256:lock"
            },
            "systemConnection": {
                "systemId": "taste",
                "topologyDigest": "sha256:topology",
                "topology": {},
                "managementBinding": {}
            }
        }))
        .unwrap();
        assert!(validate_bundle(&bundle).is_ok());
    }

    #[test]
    fn console_url_is_https_or_loopback_only() {
        assert!(secure_console_url("http://127.0.0.1:3030").is_ok());
        assert!(secure_console_url("https://console.example.com").is_ok());
        assert!(secure_console_url("http://console.example.com").is_err());
    }
}
