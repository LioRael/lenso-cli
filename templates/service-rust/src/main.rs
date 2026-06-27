use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--check") {
        println!("{}", serde_json::to_string_pretty(&service_manifest())?);
        return Ok(());
    }

    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(4100);
    let app = Router::new()
        .route("/lenso/service/v1/manifest", get(manifest))
        .merge(lenso_service::health_router());
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;

    println!("Lenso service ready: http://127.0.0.1:{port}/lenso/service/v1/manifest");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn manifest() -> Json<Value> {
    Json(service_manifest())
}

fn service_manifest() -> Value {
    json!({
        "name": "{{service_name}}",
        "version": "0.1.0",
        "provider": {
            "name": "{{service_name}}",
            "summary": "{{service_label}} provider",
        },
        "compatibility": {
            "remoteProtocolVersion": "1",
            "requiredHostFeatures": ["service.status"],
        },
        "install": {
            "services": [
                {
                    "name": "{{service_name}}",
                    "command": "cargo run",
                    "cwd": {{service_cwd}},
                    "readyUrl": "http://127.0.0.1:4100/lenso/service/v1/status",
                    "autoStart": true,
                    "readyTimeoutMs": 10000,
                },
            ],
        },
        "modules": [
            {
                "name": "{{module_name}}",
                "version": "0.1.0",
                "capabilities": ["{{module_name}}.read"],
            },
        ],
    })
}
