use axum::{Json, Router, routing::get};
include!(concat!(env!("OUT_DIR"), "/modules.rs"));

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if let Err(error) = dotenvy::from_path(".env") {
        if !matches!(error,dotenvy::Error::Io(ref e) if e.kind()==std::io::ErrorKind::NotFound) {
            return Err("Invalid project .env".into());
        }
    }
    let migrate = std::env::args().any(|arg| arg == "--migrate" || arg == "--migrate-only");
    let app = Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({"status":"ok"})) }),
        )
        .merge(mount_modules(migrate).await?);
    if std::env::args().any(|arg| arg == "--migrate-only") {
        return Ok(());
    }
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "__APP_PORT__".into())
        .parse()?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!("LISTENING {}", listener.local_addr()?.port());
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
