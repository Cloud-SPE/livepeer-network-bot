use std::sync::Arc;

use tokio::signal;
use tokio::time::{sleep, Duration as TokioDuration};

use crate::{
    config::Config,
    domains::{
        explorer::client::ExplorerClient,
        scheduler::{
            cut_change_poller, delegator_poller, digest_poster, event_poller, reward_poller,
            subscriber_digest_poster, summary_poster,
        },
        state::{event_streams::EventStreamsRepo, repo::SqliteStateRepo},
        subscriptions::repo::SqliteSubscriptionsRepo,
    },
    providers::{
        database, discord::FanOutNotifier, discord_bot::BotDmSender, discord_gateway, http,
    },
};

pub async fn run(config: Config) -> anyhow::Result<()> {
    let http_client = http::build(&config)?;
    let pool = database::open_and_migrate(&config.database_url).await?;

    let explorer = Arc::new(ExplorerClient::new(
        http_client.clone(),
        config.explorer_base_url.clone(),
    ));
    let notifier = Arc::new(FanOutNotifier::new(
        http_client.clone(),
        config.discord_webhook_urls.clone(),
    ));
    let state = Arc::new(SqliteStateRepo::new(pool.clone()));
    let streams = Arc::new(EventStreamsRepo::new(pool.clone()));
    let subscriptions = Arc::new(SqliteSubscriptionsRepo::new(pool));

    let metrics = Arc::new(crate::providers::metrics::Metrics::new());
    if let Some(bind) = config.metrics_bind.clone() {
        let metrics = metrics.clone();
        let state = state.clone();
        // Detached (NOT in `tasks`): a metrics bind failure must not trip the
        // JoinSet teardown below and take the whole bot down with it.
        tokio::spawn(async move {
            crate::providers::metrics::serve(bind, metrics, state).await;
        });
    }

    let mut tasks = tokio::task::JoinSet::new();

    {
        let explorer = explorer.clone();
        let state = state.clone();
        let interval = config.event_poll_interval;
        tasks.spawn(async move {
            event_poller::run(explorer, state, interval).await;
        });
    }
    if config.webhook_post_enabled {
        {
            let explorer = explorer.clone();
            let notifier = notifier.clone();
            let state = state.clone();
            let window = config.digest_window;
            let fetch_limit = config.digest_fetch_limit;
            tasks.spawn(async move {
                digest_poster::run(explorer, notifier, state, window, fetch_limit).await;
            });
        }
        {
            let explorer = explorer.clone();
            let notifier = notifier.clone();
            let state = state.clone();
            let interval = config.summary_poll_interval;
            let readiness = config.summary_readiness.clone();
            let metrics = metrics.clone();
            tasks.spawn(async move {
                summary_poster::run(explorer, notifier, state, interval, readiness, metrics).await;
            });
        }
    } else {
        tracing::info!(
            "webhook posts disabled (WEBHOOK_POST_ENABLED=false); ticket digests + summaries not spawned; events still poll and persist"
        );
    }

    if let Some(commands) = config.commands.clone() {
        let dm = Arc::new(BotDmSender::new(&commands.bot_token));
        let failure_threshold = commands.dm_failure_auto_unsub;
        let reward_interval = config.reward_poll_interval;
        let delegator_interval = config.delegator_poll_interval;
        let cut_change_interval = config.cut_change_poll_interval;

        // Seed delegator_history for every orch with an existing subscriber
        // before the pollers start. Without this, the first Bond observed
        // for a pre-existing delegator would be mislabeled "new delegator."
        // Done synchronously so the digest poster doesn't fire before the
        // seed completes.
        if let Err(err) = crate::seed::seed_all_subscribed(
            explorer.clone(),
            streams.clone(),
            subscriptions.clone(),
        )
        .await
        {
            tracing::warn!(?err, "startup seed of delegator_history failed");
        }

        {
            let explorer = explorer.clone();
            let streams = streams.clone();
            let subscriptions = subscriptions.clone();
            let state = state.clone();
            let dm = dm.clone();
            tokio::spawn(async move {
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
            tokio::spawn(async move {
                delegator_poller::run(explorer, streams, state, delegator_interval).await;
            });
        }

        {
            let explorer = explorer.clone();
            let streams = streams.clone();
            let subscriptions = subscriptions.clone();
            let dm = dm.clone();
            tokio::spawn(async move {
                cut_change_poller::run(
                    explorer,
                    streams,
                    subscriptions,
                    dm,
                    failure_threshold,
                    cut_change_interval,
                )
                .await;
            });
        }

        {
            let explorer = explorer.clone();
            let streams = streams.clone();
            let subscriptions = subscriptions.clone();
            let dm = dm.clone();
            let window = config.subscriber_digest_interval;
            tokio::spawn(async move {
                subscriber_digest_poster::run(
                    explorer,
                    streams,
                    subscriptions,
                    dm,
                    failure_threshold,
                    window,
                )
                .await;
            });
        }

        {
            let explorer = explorer.clone();
            let subscriptions = subscriptions.clone();
            let streams = streams.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(err) = discord_gateway::run(
                        commands.clone(),
                        explorer.clone(),
                        subscriptions.clone(),
                        streams.clone(),
                    )
                    .await
                    {
                        tracing::error!(?err, "discord gateway exited; retrying");
                        sleep(TokioDuration::from_secs(5)).await;
                        continue;
                    }

                    tracing::warn!("discord gateway exited without error; restarting");
                    sleep(TokioDuration::from_secs(5)).await;
                }
            });
        }

        tracing::info!(
            "subscriptions enabled (gateway + reward poller + delegator poller + cut-change poller + subscriber digest); webhook core remains up if commands tasks fail"
        );
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
