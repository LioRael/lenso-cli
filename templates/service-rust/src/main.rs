use axum::http::{HeaderMap, StatusCode};
use axum::{Json, Router, extract::Path, routing::get};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

const SERVICE_ID: &str = "{{service_id}}";
const SERVICE_VERSION: &str = "0.1.0";
const MODULE_ID: &str = "{{module_name}}";
const MODULE_EXPORT: &str = "{{module_export}}";
const MODULE_VERSION: &str = "0.1.0";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::from_filename(".env.local");
    if std::env::args().any(|arg| arg == "--check") {
        println!("{}", serde_json::to_string_pretty(&service_manifest())?);
        return Ok(());
    }
    if std::env::args().any(|arg| arg == "--check-release") {
        println!("{}", serde_json::to_string_pretty(&module_release())?);
        return Ok(());
    }

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or({{service_port}});
    let app = Router::new()
        .route("/lenso/service/v1/manifest", get(manifest))
        .route("/lenso/provider/v1", get(provider))
        .route(
            "/lenso/provider/v1/exports/{export}/module-release",
            get(module_release_endpoint),
        )
        .route("/system-plane/v1", get(system_plane_core))
        .route("/status", get(status))
        .merge(lenso_service::health_router());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;

    println!("Lenso service ready: http://127.0.0.1:{port}/lenso/provider/v1");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn manifest() -> Json<Value> {
    Json(service_manifest())
}

async fn provider() -> Json<Value> {
    Json(provider_descriptor())
}

async fn module_release_endpoint(
    Path(export): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if export != MODULE_EXPORT {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(module_release()))
}

async fn status() -> Json<Value> {
    Json(json!({ "service": SERVICE_ID, "status": "ready" }))
}

async fn system_plane_core(headers: HeaderMap) -> Result<Json<Value>, StatusCode> {
    let expected = std::env::var("LENSO_LOCAL_ENROLLMENT_TOKEN")
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let supplied = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if supplied != expected {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(json!({
        "protocol": lenso_service::system_plane::CORE_PROTOCOL,
        "serviceId": SERVICE_ID,
        "servicePrincipal": format!("service:{SERVICE_ID}"),
        "serviceRevision": "1",
        "capabilities": [],
    })))
}

fn service_manifest() -> Value {
    json!({
        "protocol": "lenso.service.v1",
        "name": "{{service_name}}",
        "version": SERVICE_VERSION,
        "provider": {
            "name": "{{service_name}}",
            "summary": "{{service_label}} provider",
        },
        "compatibility": {
            "providerProtocolVersion": "lenso.provider.v1",
            "requiredHostFeatures": ["service.status"],
        },
        "install": {
            "services": [
                {
                    "name": "{{service_name}}",
                    "command": "cargo run",
                    "cwd": {{service_cwd}},
                    "readyUrl": "{{service_status_url}}",
                    "autoStart": true,
                    "readyTimeoutMs": 300000,
                },
            ],
        },
        "modules": [module_manifest()],
    })
}

fn module_manifest() -> Value {
    json!({
        "capabilities": [format!("{MODULE_ID}.read")],
        "console": [],
        "console_contributions": [],
        "console_slots": [],
        "http_routes": [{
            "capability": format!("{MODULE_ID}.read"),
            "display_name": "Read service status",
            "method": "GET",
            "path": "/status",
            "story_title": "Service status read",
        }],
        "module_id": MODULE_ID,
        "protocol": "lenso.module-manifest.v1",
        "story_display": [],
    })
}

fn service_release_digest() -> String {
    digest(&json!({
        "modules": [MODULE_ID],
        "protocol": "lenso.provider-service-release.v1",
        "serviceId": SERVICE_ID,
        "version": SERVICE_VERSION,
    }))
}

fn operation_contract_digest() -> String {
    digest(&json!({
        "operations": ["GET /status"],
        "protocol": "lenso.provider-http.v1",
    }))
}

fn module_release() -> Value {
    let manifest = module_manifest();
    json!({
        "compatibility": {},
        "delivery": {
            "kind": "service",
            "service_id": SERVICE_ID,
            "service_release_version": SERVICE_VERSION,
            "service_release_digest": service_release_digest(),
            "export": MODULE_EXPORT,
            "responsibility_profile": "provider",
            "contract_digests": [operation_contract_digest()],
        },
        "manifest_digest": digest(&manifest),
        "manifest": manifest,
        "module_id": MODULE_ID,
        "protocol": "lenso.module-release.v1",
        "version": MODULE_VERSION,
    })
}

fn provider_descriptor() -> Value {
    let release = module_release();
    let manifest = module_manifest();
    json!({
        "exports": [{
            "contractDigests": { "http": operation_contract_digest() },
            "exportKey": MODULE_EXPORT,
            "manifest": manifest,
            "manifestDigest": release["manifest_digest"],
            "moduleId": MODULE_ID,
            "moduleReleaseDigest": digest(&release),
            "moduleVersion": MODULE_VERSION,
        }],
        "protocolContractDigest": digest(&json!({ "protocol": "lenso.provider.v1" })),
        "runtimeInstanceId": format!("{SERVICE_ID}-local"),
        "serviceId": SERVICE_ID,
        "serviceReleaseDigest": service_release_digest(),
        "serviceReleaseVersion": SERVICE_VERSION,
    })
}

fn digest(value: &Value) -> String {
    let bytes = serde_json_canonicalizer::to_vec(value).expect("service contract canonicalizes");
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    format!("sha256:{hex}")
}
