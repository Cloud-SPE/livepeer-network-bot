//! Reward event poller.
//!
//! Two responsibilities, run sequentially each tick:
//!
//! 1. **Ingest.** Paginated GET against `/api/v1/events?event_name=Reward`,
//!    advancing the local cursor. New rows go into `reward_events`.
//! 2. **Notify.** Pull `reward_events` rows with `sent_to_subscribers_at IS
//!    NULL`, for each find the subscribers of that orch, DM each subscriber.
//!    Flag the subscription as DM-blocked after `failure_threshold`
//!    consecutive DM 403s (the row is retained, not deleted).
//!
//! The event is marked `sent_to_subscribers_at = now()` once we've attempted
//! every subscriber. Transient failures are NOT retried per-event (would
//! cause duplicates for already-delivered subscribers); they're tolerated and
//! the failure counter on the subscription handles the persistent cases.

use std::{sync::Arc, time::Duration};

use tokio::time::{interval, MissedTickBehavior};

use crate::{
    domains::{
        explorer::client::ExplorerClient,
        notify::dm::build_reward_event_dm,
        state::{event_streams::EventStreamsRepo, repo::SqliteStateRepo},
        subscriptions::repo::SqliteSubscriptionsRepo,
    },
    providers::discord_bot::{BotDmSender, DmError},
};

const CURSOR_NAME: &str = "events:Reward";
const PAGE_LIMIT: u32 = 100;
const DISPATCH_BATCH: i64 = 50;

pub async fn run(
    explorer: Arc<ExplorerClient>,
    streams: Arc<EventStreamsRepo>,
    subscriptions: Arc<SqliteSubscriptionsRepo>,
    state: Arc<SqliteStateRepo>,
    dm: Arc<BotDmSender>,
    failure_threshold: i64,
    poll_interval: Duration,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        if let Err(err) = ingest(&explorer, &streams, &state).await {
            tracing::error!(?err, "reward poller: ingest failed");
            continue;
        }
        if let Err(err) =
            dispatch(&explorer, &streams, &subscriptions, &dm, failure_threshold).await
        {
            tracing::error!(?err, "reward poller: dispatch failed");
        }
    }
}

async fn ingest(
    explorer: &ExplorerClient,
    streams: &EventStreamsRepo,
    state: &SqliteStateRepo,
) -> anyhow::Result<()> {
    let mut cursor = state.get_cursor(CURSOR_NAME).await?;
    loop {
        let resp = explorer
            .list_events("Reward", cursor.as_deref(), PAGE_LIMIT)
            .await?;
        if resp.data.is_empty() {
            break;
        }
        for ev in &resp.data {
            streams.insert_reward_event(ev).await?;
        }
        match resp.next_cursor {
            Some(next) => {
                state.set_cursor(CURSOR_NAME, &next).await?;
                cursor = Some(next);
                if resp.data.len() < PAGE_LIMIT as usize {
                    break;
                }
            }
            None => break,
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
    let pending = streams.fetch_unsent_reward_events(DISPATCH_BATCH).await?;
    for ev in pending {
        let subs = subscriptions
            .find_for_orchestrator(&ev.orch_address)
            .await?;

        if !subs.is_empty() {
            let orch = explorer.get_orchestrator(&ev.orch_address).await?;
            let message = build_reward_event_dm(&orch, &ev);

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
                            "reward DM send failed (transient)"
                        );
                    }
                }
            }
        }

        // Mark sent unconditionally: transient failures are not retried at
        // event scope (would duplicate-deliver to subscribers who already
        // received it); per-subscription failure counters drive auto-unsub.
        streams.mark_reward_event_sent(&ev.id).await?;
    }
    Ok(())
}
