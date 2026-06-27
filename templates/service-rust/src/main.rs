use axum::{Json, Router, routing::get};
use lenso_service::{ModuleManifest, ServiceContract, ServiceHealth, ServiceProvider};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--check") {
        println!("{}", serde_json::to_string_pretty(&service_contract())?);
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

async fn manifest() -> Json<ServiceContract> {
    Json(service_contract())
}

fn service_contract() -> ServiceContract {
    ServiceContract::new(
        "{{service_name}}",
        vec![
            ModuleManifest::builder("{{module_name}}")
                .capabilities(vec!["{{module_name}}.read".to_owned()])
                .build(),
        ],
    )
    .version("0.1.0")
    .provider(ServiceProvider {
        name: "{{service_name}}".to_owned(),
        vendor: None,
        summary: Some("{{service_label}} provider".to_owned()),
        homepage: None,
    })
    .health(ServiceHealth {
        ready_url: Some("http://127.0.0.1:4100/lenso/service/v1/ready".to_owned()),
        status_url: Some("http://127.0.0.1:4100/lenso/service/v1/status".to_owned()),
        ..ServiceHealth::default()
    })
}
