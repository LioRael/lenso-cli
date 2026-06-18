#[tokio::main]
async fn main() -> anyhow::Result<()> {
    lenso_host::run_api_from_env_with_composition(lenso_starter_host::host_composition()).await
}
