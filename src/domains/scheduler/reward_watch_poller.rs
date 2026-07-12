//! Reward-call watcher.
//!
//! Each tick establishes how far the current protocol round has progressed
//! and escalates for every subscribed, active orchestrator that has not yet
//! called reward:
//!
//!   * ladder DMs at `first_alert_pct`, then every `realert_step_pct` of
//!     round completion (`alerts_sent` in `reward_watch_state` makes rungs
//!     idempotent across restarts and missed ticks);
//!   * one public delinquency digest — covering ALL active orchestrators,
//!     not just subscribed ones — once the round passes `digest_pct` (90% is
//!     the protocol's round-lock point);
//!   * one final "missed reward" DM per orchestrator after the round closes,
//!     gated on the explorer having indexed the round boundary
//!     (`meta.to_block` set on the closed round's events).
//!
//! Round progress is derived from the round's `started_at` timestamp: rounds
//! are `round_length_blocks` Ethereum L1 blocks of ~12s each, while the
//! events themselves land on Arbitrum, so wall-clock time is the only unit
//! the two chains share. "Has called reward" comes from
//! `/rounds/{id}/events?kinds=Reward`, whose `to_address` is the
//! orchestrator.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::time::{interval, MissedTickBehavior};

use crate::{
    domains::{
        explorer::{client::ExplorerClient, types::OrchestratorProfileRow},
        notify::{
            dm::{build_reward_missed_dm, build_reward_watch_dm},
            embed::{build_reward_watch_digest, DelinquentOrch},
            service::Notifier,
        },
        state::{repo::SqliteStateRepo, reward_watch::RewardWatchRepo},
        subscriptions::repo::{SqliteSubscriptionsRepo, Subscription},
    },
    providers::{
        discord::FanOutNotifier,
        discord_bot::{BotDmSender, DmError},
        metrics::Metrics,
    },
};

/// Seconds per Ethereum L1 block (fixed post-merge slot time).
const L1_BLOCK_SECS: u64 = 12;
/// Page size when listing round events / active orchestrators.
const PAGE_LIMIT: u32 = 200;
/// Cursor row (in the `cursors` table) holding the last round for which the
/// public delinquency digest was posted.
const DIGEST_CURSOR: &str = "reward_watch_digest";
/// Cursor row holding the last closed round fully processed for missed DMs.
const MISSED_CURSOR: &str = "reward_watch_missed_done";
/// Keep reward_watch_state rows for this many recent rounds.
const PRUNE_KEEP_ROUNDS: i64 = 8;

/// Knobs for the watcher, flattened from `RewardWatchConfig` plus the bits of
/// runtime context the poller needs.
#[derive(Debug, Clone)]
pub struct RewardWatchSettings {
    pub poll_interval: Duration,
    pub first_alert_pct: u32,
    pub realert_step_pct: u32,
    pub digest_pct: u32,
    pub round_length_blocks: u64,
    /// Mirrors WEBHOOK_POST_ENABLED: when false the public digest is skipped
    /// (DMs still flow).
    pub post_digest: bool,
    pub failure_threshold: i64,
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    explorer: Arc<ExplorerClient>,
    subscriptions: Arc<SqliteSubscriptionsRepo>,
    watch: Arc<RewardWatchRepo>,
    state: Arc<SqliteStateRepo>,
    notifier: Arc<FanOutNotifier>,
    dm: Arc<BotDmSender>,
    metrics: Arc<Metrics>,
    settings: RewardWatchSettings,
) {
    let mut tick = interval(settings.poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        if let Err(err) = poll_once(
            &explorer,
            &subscriptions,
            &watch,
            &state,
            &notifier,
            &dm,
            &metrics,
            &settings,
        )
        .await
        {
            tracing::error!(?err, "reward watch poller: tick failed");
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn poll_once(
    explorer: &ExplorerClient,
    subscriptions: &SqliteSubscriptionsRepo,
    watch: &RewardWatchRepo,
    state: &SqliteStateRepo,
    notifier: &FanOutNotifier,
    dm: &BotDmSender,
    metrics: &Metrics,
    settings: &RewardWatchSettings,
) -> anyhow::Result<()> {
    let Some(round_row) = explorer.latest_round().await? else {
        tracing::warn!("reward watch poller: explorer returned an empty rounds index");
        return Ok(());
    };
    let round: i64 = round_row.round.parse()?;
    let progress = round_progress(
        round_row.started_at,
        Utc::now(),
        settings.round_length_blocks,
    );

    // Per-tick profile cache: active-set membership + display name/avatar.
    let mut profiles: HashMap<String, OrchestratorProfileRow> = HashMap::new();

    if let Err(err) = process_closed_round(
        explorer,
        subscriptions,
        watch,
        state,
        dm,
        metrics,
        settings,
        round,
        &mut profiles,
    )
    .await
    {
        tracing::error!(
            ?err,
            prev_round = round - 1,
            "reward watch poller: missed-reward pass failed"
        );
    }

    let rewarded = rewarded_orchs(explorer, round).await?;
    let subscribed = subscriptions.distinct_subscribed_orchestrators().await?;

    for orch in &subscribed {
        if rewarded.contains(&orch.to_lowercase()) {
            watch.mark_resolved(round, orch).await?;
        }
    }

    let due = due_alerts(
        progress.elapsed_pct,
        settings.first_alert_pct,
        settings.realert_step_pct,
    );
    if due > 0 {
        for orch in &subscribed {
            if rewarded.contains(&orch.to_lowercase()) {
                continue;
            }
            let sent = watch.alerts_sent(round, orch).await?;
            if sent >= due {
                continue;
            }
            let profile = match cached_profile(explorer, &mut profiles, orch).await {
                Ok(p) => p,
                Err(err) => {
                    tracing::warn!(?err, %orch, "reward watch poller: profile fetch failed; skipping this tick");
                    continue;
                }
            };
            // Inactive orchestrators are not expected to call reward (they
            // earn nothing this round) — same rule as the original
            // orchestrator-watcher bot.
            if !profile.is_active {
                continue;
            }

            let subs = subscriptions.find_for_orchestrator(orch).await?;
            if !subs.is_empty() {
                let message = build_reward_watch_dm(&profile, orch, round, &progress);
                dm_fan_out(
                    dm,
                    subscriptions,
                    settings.failure_threshold,
                    &subs,
                    message,
                )
                .await?;
                metrics.record_reward_watch_alert();
            }
            watch.set_alerts_sent(round, orch, due).await?;
        }
    }

    if settings.post_digest && progress.elapsed_pct >= settings.digest_pct as f64 {
        if let Err(err) = post_digest_once(
            explorer, state, notifier, metrics, round, &progress, &rewarded,
        )
        .await
        {
            tracing::error!(?err, round, "reward watch poller: digest post failed");
        }
    }

    Ok(())
}

/// Send the one-per-round "missed reward" DMs for the most recently closed
/// round, then advance the `reward_watch_missed_done` cursor. Deferred (no
/// cursor advance) until the explorer has indexed past the round boundary,
/// signalled by `to_block` being set on the closed round's events response.
#[allow(clippy::too_many_arguments)]
async fn process_closed_round(
    explorer: &ExplorerClient,
    subscriptions: &SqliteSubscriptionsRepo,
    watch: &RewardWatchRepo,
    state: &SqliteStateRepo,
    dm: &BotDmSender,
    metrics: &Metrics,
    settings: &RewardWatchSettings,
    current_round: i64,
    profiles: &mut HashMap<String, OrchestratorProfileRow>,
) -> anyhow::Result<()> {
    let prev_round = current_round - 1;
    let done: i64 = state
        .get_cursor(MISSED_CURSOR)
        .await?
        .and_then(|v| v.parse().ok())
        // First deploy: treat the previous round as already handled so we
        // don't spray "missed reward" DMs about a round nobody was watching.
        .unwrap_or(prev_round);
    if done >= prev_round {
        return Ok(());
    }

    // Only the most recently closed round is reconciled; if the bot was down
    // across several rounds those alerts are stale and intentionally skipped.
    let first_page = explorer.round_events(prev_round, "Reward", None, 1).await?;
    if first_page.meta.to_block.is_none() {
        tracing::info!(
            prev_round,
            "reward watch poller: closed round not fully indexed yet; deferring missed-reward DMs"
        );
        return Ok(());
    }

    let rewarded = rewarded_orchs(explorer, prev_round).await?;
    for orch in subscriptions.distinct_subscribed_orchestrators().await? {
        if rewarded.contains(&orch.to_lowercase()) {
            continue;
        }
        if watch.was_missed_notified(prev_round, &orch).await? {
            continue;
        }
        let profile = match cached_profile(explorer, profiles, &orch).await {
            Ok(p) => p,
            Err(err) => {
                tracing::warn!(?err, %orch, "reward watch poller: profile fetch failed in missed pass");
                continue;
            }
        };
        if !profile.is_active {
            continue;
        }

        let subs = subscriptions.find_for_orchestrator(&orch).await?;
        if !subs.is_empty() {
            let message = build_reward_missed_dm(&profile, &orch, prev_round);
            dm_fan_out(
                dm,
                subscriptions,
                settings.failure_threshold,
                &subs,
                message,
            )
            .await?;
            metrics.record_reward_watch_missed();
        }
        watch.mark_missed_notified(prev_round, &orch).await?;
    }

    state
        .set_cursor(MISSED_CURSOR, &prev_round.to_string())
        .await?;
    watch
        .prune_before(current_round - PRUNE_KEEP_ROUNDS)
        .await?;
    Ok(())
}

/// Post the public delinquency digest exactly once per round: every active
/// orchestrator (full set, not just subscribed) that has not called reward.
async fn post_digest_once(
    explorer: &ExplorerClient,
    state: &SqliteStateRepo,
    notifier: &FanOutNotifier,
    metrics: &Metrics,
    round: i64,
    progress: &RoundProgress,
    rewarded: &HashSet<String>,
) -> anyhow::Result<()> {
    let posted: i64 = state
        .get_cursor(DIGEST_CURSOR)
        .await?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if posted >= round {
        return Ok(());
    }

    let mut delinquents = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = explorer
            .list_orchestrators(cursor.as_deref(), PAGE_LIMIT, true)
            .await?;
        for orch in page.data {
            if orch.is_active && !rewarded.contains(&orch.address.to_lowercase()) {
                delinquents.push(DelinquentOrch {
                    address: orch.address,
                    display_name: orch.display_name,
                    total_stake_lpt: orch.total_stake.parse().unwrap_or(0.0),
                });
            }
        }
        cursor = page.meta.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    if !delinquents.is_empty() {
        // Largest stake first: those misses cost delegators the most.
        delinquents.sort_by(|a, b| b.total_stake_lpt.total_cmp(&a.total_stake_lpt));
        let payload = build_reward_watch_digest(round, progress, &delinquents);
        notifier.send(payload).await?;
        metrics.record_reward_watch_digest();
    }
    state.set_cursor(DIGEST_CURSOR, &round.to_string()).await?;
    Ok(())
}

/// Orchestrator addresses (lowercased) that have called reward in `round`.
async fn rewarded_orchs(explorer: &ExplorerClient, round: i64) -> anyhow::Result<HashSet<String>> {
    let mut rewarded = HashSet::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = explorer
            .round_events(round, "Reward", cursor.as_deref(), PAGE_LIMIT)
            .await?;
        for ev in page.data {
            if let Some(addr) = ev.to_address {
                rewarded.insert(addr.to_lowercase());
            }
        }
        cursor = page.meta.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(rewarded)
}

async fn cached_profile(
    explorer: &ExplorerClient,
    profiles: &mut HashMap<String, OrchestratorProfileRow>,
    orch: &str,
) -> anyhow::Result<OrchestratorProfileRow> {
    if let Some(p) = profiles.get(orch) {
        return Ok(p.clone());
    }
    let p = explorer.get_orchestrator(orch).await?;
    profiles.insert(orch.to_string(), p.clone());
    Ok(p)
}

/// DM every subscriber, mirroring the failure handling of the other pollers:
/// clear the failure counter on success, count 403s toward `dm_blocked`, and
/// log-but-continue on transient errors.
async fn dm_fan_out(
    dm: &BotDmSender,
    subscriptions: &SqliteSubscriptionsRepo,
    failure_threshold: i64,
    subs: &[Subscription],
    message: poise::serenity_prelude::CreateMessage,
) -> anyhow::Result<()> {
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
                    "reward watch DM send failed (transient)"
                );
            }
        }
    }
    Ok(())
}

/// How far the current round has progressed, in every unit the messages need.
#[derive(Debug, Clone)]
pub struct RoundProgress {
    /// 0–100, clamped.
    pub elapsed_pct: f64,
    /// Estimated L1 block within the round, 0..=round_length_blocks.
    pub est_block: u64,
    pub round_length_blocks: u64,
    /// Estimated wall-clock time until the round can close (zero once past).
    pub remaining: Duration,
}

/// Derive round progress from `started_at`: rounds are `round_length_blocks`
/// L1 blocks at a fixed 12s each. Rounds can run long (the next round starts
/// only when someone calls `initializeRound`), so progress saturates at 100%
/// rather than being trusted as an exact deadline.
fn round_progress(
    started_at: DateTime<Utc>,
    now: DateTime<Utc>,
    round_length_blocks: u64,
) -> RoundProgress {
    let round_secs = round_length_blocks * L1_BLOCK_SECS;
    let elapsed_secs = (now - started_at).num_seconds().max(0) as u64;
    let fraction = (elapsed_secs as f64 / round_secs as f64).clamp(0.0, 1.0);
    RoundProgress {
        elapsed_pct: fraction * 100.0,
        est_block: ((fraction * round_length_blocks as f64) as u64).min(round_length_blocks),
        round_length_blocks,
        remaining: Duration::from_secs(round_secs.saturating_sub(elapsed_secs)),
    }
}

/// Number of ladder rungs due at `elapsed_pct`: one at `first_pct`, another
/// every `step_pct` after that. Zero before the first rung.
fn due_alerts(elapsed_pct: f64, first_pct: u32, step_pct: u32) -> i64 {
    if elapsed_pct < first_pct as f64 {
        return 0;
    }
    1 + ((elapsed_pct - first_pct as f64) / step_pct as f64) as i64
}

#[cfg(test)]
mod tests {
    use super::{due_alerts, round_progress};
    use chrono::{TimeZone, Utc};

    #[test]
    fn no_alerts_due_before_first_threshold() {
        assert_eq!(due_alerts(0.0, 25, 10), 0);
        assert_eq!(due_alerts(24.9, 25, 10), 0);
    }

    #[test]
    fn ladder_advances_one_rung_per_step() {
        assert_eq!(due_alerts(25.0, 25, 10), 1);
        assert_eq!(due_alerts(34.9, 25, 10), 1);
        assert_eq!(due_alerts(35.0, 25, 10), 2);
        assert_eq!(due_alerts(85.0, 25, 10), 7);
        assert_eq!(due_alerts(100.0, 25, 10), 8);
    }

    #[test]
    fn progress_is_clamped_and_scaled() {
        let start = Utc.with_ymd_and_hms(2026, 7, 11, 0, 0, 0).unwrap();

        // Halfway: 6377 blocks * 12s = 76524s; half is 38262s.
        let half = start + chrono::Duration::seconds(38_262);
        let p = round_progress(start, half, 6377);
        assert!((p.elapsed_pct - 50.0).abs() < 0.01);
        assert_eq!(p.est_block, 3188);
        assert_eq!(p.remaining.as_secs(), 38_262);

        // Before the start (clock skew) clamps to zero.
        let before = start - chrono::Duration::seconds(60);
        let p = round_progress(start, before, 6377);
        assert_eq!(p.elapsed_pct, 0.0);
        assert_eq!(p.est_block, 0);

        // A round running long saturates at 100% / full length / zero left.
        let late = start + chrono::Duration::seconds(100_000);
        let p = round_progress(start, late, 6377);
        assert_eq!(p.elapsed_pct, 100.0);
        assert_eq!(p.est_block, 6377);
        assert_eq!(p.remaining.as_secs(), 0);
    }
}
