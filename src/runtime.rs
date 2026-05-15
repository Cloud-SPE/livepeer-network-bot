use std::sync::Arc;

use tokio::signal;

use crate::{
    config::Config,
    domains::{
        explorer::client::ExplorerClient,
        scheduler::{digest_poster, event_poller, summary_poster},
        state::repo::SqliteStateRepo,
    },
    providers::{database, discord::DiscordWebhook, http},
};

pub async fn run(config: Config) -> anyhow::Result<()> {
    let http_client = http::build(&config)?;
    let pool = database::open_and_migrate(&config.database_url).await?;

    let explorer = Arc::new(ExplorerClient::new(
        http_client.clone(),
        config.explorer_base_url.clone(),
    ));
    let notifier = Arc::new(DiscordWebhook::new(
        http_client.clone(),
        config.discord_webhook_url.clone(),
    ));
    let state = Arc::new(SqliteStateRepo::new(pool));

    let mut tasks = tokio::task::JoinSet::new();

    {
        let explorer = explorer.clone();
        let state = state.clone();
        let interval = config.event_poll_interval;
        tasks.spawn(async move {
            event_poller::run(explorer, state, interval).await;
        });
    }
    {
        let explorer = explorer.clone();
        let notifier = notifier.clone();
        let state = state.clone();
        let window = config.digest_window;
        tasks.spawn(async move {
            digest_poster::run(explorer, notifier, state, window).await;
        });
    }
    {
        let explorer = explorer.clone();
        let notifier = notifier.clone();
        let state = state.clone();
        let interval = config.summary_poll_interval;
        tasks.spawn(async move {
            summary_poster::run(explorer, notifier, state, interval).await;
        });
    }

    tokio::select! {
        _ = signal::ctrl_c() => {
            tracing::info!("ctrl-c received, shutting down");
        }
        Some(res) = tasks.join_next() => {
            tracing::error!(?res, "task exited unexpectedly");
        }
    }

    tasks.shutdown().await;
    Ok(())
}
