//! TranscoderUpdate cut-change poller.
//!
//! Each tick scopes work to orchestrators that currently have subscribers,
//! fetches recent `/transcoders/{addr}/params/history` rows, persists them,
//! and DMs subscribers for newly observed rows. The first time an orchestrator
//! is observed, existing history is inserted as already-sent so deployment or
//! first subscription does not spam historical updates.

use std::{sync::Arc, time::Duration};

use tokio::time::{interval, MissedTickBehavior};

use crate::{
    domains::{
        explorer::client::ExplorerClient, notify::dm::build_cut_change_dm,
        state::event_streams::EventStreamsRepo, subscriptions::repo::SqliteSubscriptionsRepo,
    },
    providers::discord_bot::{BotDmSender, DmError},
};

const HISTORY_LIMIT: u32 = 50;
const DISPATCH_BATCH: i64 = 50;

pub async fn run(
    explorer: Arc<ExplorerClient>,
    streams: Arc<EventStreamsRepo>,
    subscriptions: Arc<SqliteSubscriptionsRepo>,
    dm: Arc<BotDmSender>,
    failure_threshold: i64,
    poll_interval: Duration,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        if let Err(err) = ingest(&explorer, &streams, &subscriptions).await {
            tracing::error!(?err, "cut-change poller: ingest failed");
            continue;
        }
        if let Err(err) =
            dispatch(&explorer, &streams, &subscriptions, &dm, failure_threshold).await
        {
            tracing::error!(?err, "cut-change poller: dispatch failed");
        }
    }
}

async fn ingest(
    explorer: &ExplorerClient,
    streams: &EventStreamsRepo,
    subscriptions: &SqliteSubscriptionsRepo,
) -> anyhow::Result<()> {
    let orchs = subscriptions.distinct_subscribed_orchestrators().await?;
    for orch in orchs {
        let first_seen = !streams.has_cut_change_events_for_orch(&orch).await?;
        let resp = explorer
            .transcoder_params_history(&orch, HISTORY_LIMIT)
            .await?;
        for ev in &resp.data {
            streams.insert_cut_change_event(ev, first_seen).await?;
        }
    }
    Ok(())
}

async fn dispatch(
    explorer: &ExplorerClient,
    streams: &EventStreamsRepo,
    subscriptions: &SqliteSubscriptionsRepo,
    dm: &BotDmSender,
    failure_threshold: i64,
) -> anyhow::Result<()> {
    let pending = streams
        .fetch_unsent_cut_change_events(DISPATCH_BATCH)
        .await?;

    for ev in pending {
        let subs = subscriptions
            .find_for_orchestrator(&ev.orch_address)
            .await?;

        if !subs.is_empty() {
            let orch = explorer.get_orchestrator(&ev.orch_address).await?;
            let message = build_cut_change_dm(&orch, &ev);

            for sub in subs {
                let user_id: u64 = match sub.discord_user_id.parse() {
                    Ok(u) => u,
                    Err(_) => {
                        tracing::warn!(
                            user = %sub.discord_user_id,
                            "non-numeric discord_user_id in subscriptions row; skipping"
                        );
                        continue;
                    }
                };

                match dm.send_dm(user_id, message.clone()).await {
                    Ok(()) => {
                        let _ = subscriptions
                            .clear_dm_failure(&sub.discord_user_id, &sub.orchestrator_address)
                            .await;
                    }
                    Err(DmError::DmsClosed { code }) => {
                        let count = subscriptions
                            .increment_dm_failure(&sub.discord_user_id, &sub.orchestrator_address)
                            .await?;
                        if count >= failure_threshold && !sub.dm_blocked {
                            subscriptions
                                .set_dm_blocked(&sub.discord_user_id, &sub.orchestrator_address)
                                .await?;
                            tracing::info!(
                                user = %sub.discord_user_id,
                                orch = %sub.orchestrator_address,
                                failures = count,
                                discord_code = ?code,
                                "flagged subscription as DM-blocked after consecutive DM failures (subscription retained)"
                            );
                        }
                    }
                    Err(other) => {
                        tracing::error!(
                            ?other,
                            user = %sub.discord_user_id,
                            "cut-change DM send failed (transient)"
                        );
                    }
                }
            }
        }

        streams.mark_cut_change_event_sent(&ev.event_id).await?;
    }

    Ok(())
}
