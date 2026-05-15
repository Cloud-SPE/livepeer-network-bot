use std::sync::Arc;

use tokio::signal;

use crate::{
    config::Config,
    domains::{
        explorer::client::ExplorerClient,
        scheduler::{delegator_poller, digest_poster, event_poller, reward_poller, summary_poster},
        state::{event_streams::EventStreamsRepo, repo::SqliteStateRepo},
        subscriptions::repo::SqliteSubscriptionsRepo,
    },
    providers::{
        database, discord::DiscordWebhook, discord_bot::BotDmSender, discord_gateway, http,
    },
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
    let state = Arc::new(SqliteStateRepo::new(pool.clone()));
    let streams = Arc::new(EventStreamsRepo::new(pool.clone()));
    let subscriptions = Arc::new(SqliteSubscriptionsRepo::new(pool));

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

    if let Some(commands) = config.commands.clone() {
        let dm = Arc::new(BotDmSender::new(&commands.bot_token));
        let failure_threshold = commands.dm_failure_auto_unsub;
        let reward_interval = config.reward_poll_interval;
        let delegator_interval = config.delegator_poll_interval;

        {
            let explorer = explorer.clone();
            let streams = streams.clone();
            let subscriptions = subscriptions.clone();
            let state = state.clone();
            let dm = dm.clone();
            tasks.spawn(async move {
                reward_poller::run(
                    explorer,
                    streams,
                    subscriptions,
                    state,
                    dm,
                    failure_threshold,
                    reward_interval,
                )
                .await;
            });
        }

        {
            let explorer = explorer.clone();
            let streams = streams.clone();
            let state = state.clone();
            tasks.spawn(async move {
                delegator_poller::run(explorer, streams, state, delegator_interval).await;
            });
        }

        {
            let explorer = explorer.clone();
            let subscriptions = subscriptions.clone();
            tasks.spawn(async move {
                if let Err(err) = discord_gateway::run(commands, explorer, subscriptions).await {
                    tracing::error!(?err, "discord gateway exited with error");
                }
            });
        }

        tracing::info!("slash commands + reward/delegator pollers enabled");
    } else {
        tracing::info!(
            "slash commands disabled (set COMMANDS_ENABLED=true to enable subscriptions)"
        );
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
