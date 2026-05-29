use std::{sync::Arc, time::Duration};

use chrono::{Datelike, NaiveDate, NaiveTime, Utc};
use tokio::time::{interval, MissedTickBehavior};

use crate::{
    config::SummaryReadiness,
    domains::{
        explorer::{client::ExplorerClient, types::Cadence},
        notify::{embed::build_summary, service::Notifier},
        state::repo::{SqliteStateRepo, SummarySnapshot},
    },
};

pub async fn run<N: Notifier>(
    explorer: Arc<ExplorerClient>,
    notifier: Arc<N>,
    state: Arc<SqliteStateRepo>,
    poll_interval: Duration,
    readiness: SummaryReadiness,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        for cadence in [Cadence::Daily, Cadence::Weekly, Cadence::Monthly] {
            if let Err(err) =
                maybe_post(&explorer, notifier.as_ref(), &state, cadence, &readiness).await
            {
                tracing::error!(?err, ?cadence, "summary poster iteration failed");
            }
        }
    }
}

async fn maybe_post<N: Notifier>(
    explorer: &ExplorerClient,
    notifier: &N,
    state: &SqliteStateRepo,
    cadence: Cadence,
    readiness: &SummaryReadiness,
) -> anyhow::Result<()> {
    let now = Utc::now();
    let (period_date, range_from, range_to) = last_closed_period(cadence, now.date_naive());

    if state.summary_posted(cadence, period_date).await? {
        return Ok(());
    }

    // The period closes at midnight UTC following its last day. Everything
    // downstream is measured from that instant.
    let midnight = NaiveTime::from_hms_opt(0, 0, 0).expect("midnight always valid");
    let window_start = range_from.and_time(midnight).and_utc();
    let period_close = (range_to + chrono::Duration::days(1))
        .and_time(midnight)
        .and_utc();

    // 1. Settlement floor: never post before the explorer has had time to
    //    index/enrich/derive the period.
    let settle = chrono::Duration::from_std(settle_for(readiness, cadence))?;
    let eligible_at = period_close + settle;
    if now < eligible_at {
        tracing::debug!(?cadence, %period_date, %eligible_at, "summary not yet eligible (settling)");
        return Ok(());
    }

    let summary = explorer.payout_summary(cadence, period_date).await?;
    let snapshot = SummarySnapshot {
        ticket_count: summary.ticket_count.clone(),
        usd_rows_priced: summary.usd_rows_priced.clone(),
        sum_face_value_native: summary.sum_face_value_native.clone(),
        sum_commission_native: summary.sum_commission_native.clone(),
    };

    // 2. Readiness signals (no freshness endpoint exists, so we infer).
    let ticket_count = parse_count(&summary.ticket_count);
    let priced = parse_count(&summary.usd_rows_priced);
    let local_count = state
        .count_winning_tickets_in_window(window_start, period_close)
        .await?;
    let prior = state.get_summary_snapshot(cadence, period_date).await?;

    // Enrichment: every counted ticket must carry a USD valuation.
    let enrichment_ok = priced >= ticket_count;
    // Cross-check: the rollup must not report fewer tickets than the bot has
    // already ingested on-chain for the same window.
    let crosscheck_ok = ticket_count >= local_count;
    // Stability: the figures must be unchanged since the previous poll.
    let stable = prior.as_ref() == Some(&snapshot);

    let ready = enrichment_ok && crosscheck_ok && stable;
    let past_deadline = now >= period_close + chrono::Duration::from_std(readiness.max_defer)?;

    if !ready {
        // Record this observation so the next poll can detect stability.
        state
            .upsert_summary_snapshot(cadence, period_date, &snapshot)
            .await?;

        if !past_deadline {
            tracing::info!(
                ?cadence, %period_date,
                enrichment_ok, crosscheck_ok, stable,
                ticket_count, priced, local_count,
                "summary not ready; deferring"
            );
            return Ok(());
        }

        tracing::warn!(
            ?cadence, %period_date,
            enrichment_ok, crosscheck_ok, stable,
            ticket_count, priced, local_count,
            "summary still not ready past max-defer deadline; posting with incomplete marker"
        );
    }

    let leaderboard = explorer
        .payout_leaderboard(range_from, range_to, 10)
        .await?;
    let payload = build_summary(cadence, period_date, &summary, &leaderboard.data, !ready);
    notifier.send(payload).await?;
    state.mark_summary_posted(cadence, period_date).await?;
    tracing::info!(?cadence, %period_date, incomplete = !ready, "summary posted");
    Ok(())
}

fn settle_for(readiness: &SummaryReadiness, cadence: Cadence) -> Duration {
    match cadence {
        Cadence::Daily => readiness.settle_daily,
        Cadence::Weekly => readiness.settle_weekly,
        Cadence::Monthly => readiness.settle_monthly,
    }
}

/// Parse an explorer count field (a decimal string) into a non-negative
/// integer, treating any unparseable/negative value as 0.
fn parse_count(raw: &str) -> i64 {
    raw.trim()
        .parse::<f64>()
        .ok()
        .filter(|n| *n >= 0.0)
        .map(|n| n as i64)
        .unwrap_or(0)
}

fn last_closed_period(cadence: Cadence, today: NaiveDate) -> (NaiveDate, NaiveDate, NaiveDate) {
    match cadence {
        Cadence::Daily => {
            let d = today - chrono::Duration::days(1);
            (d, d, d)
        }
        Cadence::Weekly => {
            // Previous Mon..Sun window. weekday().num_days_from_monday(): Mon=0..Sun=6
            let this_monday =
                today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
            let last_monday = this_monday - chrono::Duration::days(7);
            let last_sunday = last_monday + chrono::Duration::days(6);
            (last_monday, last_monday, last_sunday)
        }
        Cadence::Monthly => {
            let first_of_this_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1)
                .expect("first of month always valid");
            let last_day_prev = first_of_this_month - chrono::Duration::days(1);
            let first_of_prev =
                NaiveDate::from_ymd_opt(last_day_prev.year(), last_day_prev.month(), 1)
                    .expect("first of month always valid");
            (first_of_prev, first_of_prev, last_day_prev)
        }
    }
}
