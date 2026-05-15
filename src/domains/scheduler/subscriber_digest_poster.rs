//! Subscriber digest poster.
//!
//! Every tick (default 15 min), reads unsent rows from `delegator_events`,
//! groups them by `orch_address`, and for each subscribed user produces ONE
//! DM per orchestrator covering all Bond / Unbond / Rebond events in the
//! window. Bonds are pre-classified using local history into new-delegator
//! and stake-change buckets so the message can label each line accurately.
//!
//! Auto-unsubscribes after `failure_threshold` consecutive 403s (same logic
//! as the reward poller in 004b).
//!
//! Marking semantics: an event is marked `sent_to_subscribers = 1` ONLY
//! after every subscriber for its orch has had a delivery attempt. Transient
//! per-subscriber failures don't retry the whole event (would duplicate-DM
//! subscribers who already received it).

use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::Utc;
use tokio::time::{interval, MissedTickBehavior};

use crate::{
    domains::{
        explorer::client::ExplorerClient,
        notify::dm::{build_delegator_digest_dm, DelegatorDigest},
        state::event_streams::{DelegatorEventRow, EventStreamsRepo},
        subscriptions::repo::SqliteSubscriptionsRepo,
    },
    providers::discord_bot::{BotDmSender, DmError},
};

const FETCH_LIMIT: i64 = 500;

pub async fn run(
    explorer: Arc<ExplorerClient>,
    streams: Arc<EventStreamsRepo>,
    subscriptions: Arc<SqliteSubscriptionsRepo>,
    dm: Arc<BotDmSender>,
    failure_threshold: i64,
    window: Duration,
) {
    let mut tick = interval(window);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // First tick fires immediately; skip so the first real tick has time to
    // accumulate events.
    tick.tick().await;

    loop {
        tick.tick().await;
        if let Err(err) =
            run_once(&explorer, &streams, &subscriptions, &dm, failure_threshold).await
        {
            tracing::error!(?err, "subscriber digest poster iteration failed");
        }
    }
}

async fn run_once(
    explorer: &ExplorerClient,
    streams: &EventStreamsRepo,
    subscriptions: &SqliteSubscriptionsRepo,
    dm: &BotDmSender,
    failure_threshold: i64,
) -> anyhow::Result<()> {
    let pending = streams.fetch_unsent_delegator_events(FETCH_LIMIT).await?;
    if pending.is_empty() {
        return Ok(());
    }

    let mut by_orch: HashMap<String, Vec<DelegatorEventRow>> = HashMap::new();
    for ev in pending {
        by_orch.entry(ev.orch_address.clone()).or_default().push(ev);
    }

    let now = Utc::now();
    let mut sent_ids = Vec::new();

    for (orch_addr, events) in by_orch {
        let subs = subscriptions.find_for_orchestrator(&orch_addr).await?;
        let event_ids: Vec<String> = events.iter().map(|e| e.id.clone()).collect();

        if !subs.is_empty() {
            // Classify each Bond as new vs stake-change against historical
            // delegator_events for the same (delegator, orch) pair.
            let mut new_bonds: Vec<&DelegatorEventRow> = Vec::new();
            let mut stake_change_bonds: Vec<&DelegatorEventRow> = Vec::new();
            let mut unbonds: Vec<&DelegatorEventRow> = Vec::new();
            let mut rebonds: Vec<&DelegatorEventRow> = Vec::new();

            for ev in &events {
                match ev.event_name.as_str() {
                    "Bond" => {
                        let prior = streams
                            .count_prior_delegator_events(
                                &ev.delegator_address,
                                &ev.orch_address,
                                ev.block_timestamp,
                            )
                            .await?;
                        if prior == 0 {
                            new_bonds.push(ev);
                        } else {
                            stake_change_bonds.push(ev);
                        }
                    }
                    "Unbond" => unbonds.push(ev),
                    "Rebond" => rebonds.push(ev),
                    _ => {}
                }
            }

            let digest = DelegatorDigest {
                new_bonds,
                stake_change_bonds,
                unbonds,
                rebonds,
            };

            if !digest.is_empty() {
                let orch = explorer.get_orchestrator(&orch_addr).await?;
                let message = build_delegator_digest_dm(&orch, &orch_addr, &digest, now);

                for sub in subs {
                    let user_id: u64 = match sub.discord_user_id.parse() {
                        Ok(u) => u,
                        Err(_) => {
                            tracing::warn!(
                                user = %sub.discord_user_id,
                                "non-numeric discord_user_id; skipping"
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
                        Err(DmError::DmsClosed) => {
                            let count = subscriptions
                                .increment_dm_failure(
                                    &sub.discord_user_id,
                                    &sub.orchestrator_address,
                                )
                                .await?;
                            if count >= failure_threshold {
                                subscriptions
                                    .delete(&sub.discord_user_id, &sub.orchestrator_address)
                                    .await?;
                                tracing::info!(
                                    user = %sub.discord_user_id,
                                    orch = %sub.orchestrator_address,
                                    failures = count,
                                    "auto-unsubscribed after consecutive DM failures"
                                );
                            }
                        }
                        Err(other) => {
                            tracing::error!(
                                ?other,
                                user = %sub.discord_user_id,
                                "subscriber digest DM send failed (transient)"
                            );
                        }
                    }
                }
            }
        }

        sent_ids.extend(event_ids);
    }

    streams.mark_delegator_events_sent(&sent_ids).await?;
    Ok(())
}
