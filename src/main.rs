use livepeer_payout_bot::{config::Config, runtime};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("info,sqlx::query=warn"))
        .unwrap();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_target(true)
        .init();

    let config = Config::from_env()?;
    tracing::info!(?config, "starting livepeer-payout-bot");

    runtime::run(config).await
}
