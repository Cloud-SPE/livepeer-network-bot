//! Subscription-related seeding.
//!
//! On a fresh database the bot has no record of which delegators were
//! already bonded to any given orchestrator at boot time. Without a seed,
//! the first `Bond` event we observe for an existing delegator would be
//! mislabeled as "new delegator" in the 004c subscriber digest.
//!
//! This module exposes two helpers:
//!
//!   - `seed_all_subscribed`: at startup, page every distinct subscribed orch,
//!     record its current delegator set into `delegator_history`, and mark
//!     existing cut-change history as already sent.
//!   - `seed_one`: invoked by `/subscribe` so a freshly-subscribed orch gets
//!     its history populated immediately (before the next Bond arrives).
//!   - `seed_cut_history_one`: invoked by `/subscribe` so existing
//!     TranscoderUpdate rows are marked as already sent before new cut changes
//!     become eligible for subscriber DMs.
//!
//! These helpers use `INSERT OR IGNORE` semantics — calling them repeatedly
//! is safe; only first-seen rows are written.

use std::sync::Arc;

use crate::domains::{
    explorer::client::ExplorerClient, state::event_streams::EventStreamsRepo,
    subscriptions::repo::SqliteSubscriptionsRepo,
};

const PAGE_LIMIT: u32 = 500;
const CUT_HISTORY_LIMIT: u32 = 50;

/// Seed `delegator_history` for one orchestrator's current delegator set.
/// Returns the number of `(delegator, orch)` pairs newly inserted.
pub async fn seed_one(
    explorer: &ExplorerClient,
    streams: &EventStreamsRepo,
    orch_address: &str,
) -> anyhow::Result<usize> {
    let mut inserted = 0;
    let mut cursor: Option<String> = None;
    loop {
        let resp = explorer
            .orchestrator_delegators(orch_address, cursor.as_deref(), PAGE_LIMIT)
            .await?;
        for d in &resp.data {
            if streams
                .record_first_seen(&d.delegator_address, orch_address)
                .await?
            {
                inserted += 1;
            }
        }
        match resp.meta.next_cursor {
            Some(next) => cursor = Some(next),
            None => break,
        }
    }
    Ok(inserted)
}

/// Seed `cut_change_events` for one orchestrator's existing TranscoderUpdate
/// history as already sent. Returns the number of rows newly inserted.
pub async fn seed_cut_history_one(
    explorer: &ExplorerClient,
    streams: &EventStreamsRepo,
    orch_address: &str,
) -> anyhow::Result<usize> {
    let resp = explorer
        .transcoder_params_history(orch_address, CUT_HISTORY_LIMIT)
        .await?;
    let mut inserted = 0;
    for ev in &resp.data {
        if streams.insert_cut_change_event(ev, true).await? {
            inserted += 1;
        }
    }
    Ok(inserted)
}

/// Seed every orchestrator that has at least one subscriber.
pub async fn seed_all_subscribed(
    explorer: Arc<ExplorerClient>,
    streams: Arc<EventStreamsRepo>,
    subscriptions: Arc<SqliteSubscriptionsRepo>,
) -> anyhow::Result<()> {
    let orchs = subscriptions.distinct_subscribed_orchestrators().await?;
    if orchs.is_empty() {
        tracing::info!("seed: no subscribed orchestrators to seed");
        return Ok(());
    }
    tracing::info!(count = orchs.len(), "seed: priming subscription history");
    for orch in orchs {
        match seed_one(&explorer, &streams, &orch).await {
            Ok(n) => tracing::info!(orch = %orch, inserted = n, "seed: delegator_history seeded"),
            Err(err) => tracing::warn!(?err, orch = %orch, "seed: delegator_history failed"),
        }
        match seed_cut_history_one(&explorer, &streams, &orch).await {
            Ok(n) => tracing::info!(orch = %orch, inserted = n, "seed: cut_change_events seeded"),
            Err(err) => tracing::warn!(?err, orch = %orch, "seed: cut_change_events failed"),
        }
    }
    Ok(())
}
