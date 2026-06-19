#[tokio::main]
async fn main() -> anyhow::Result<()> {
    lenso::host::run_api_with_embedded_worker_from_env_with_composition(
        lenso_starter_host::host_composition(),
    )
    .await
}
