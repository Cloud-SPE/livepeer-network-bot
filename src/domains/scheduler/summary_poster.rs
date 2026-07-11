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
    providers::metrics::Metrics,
};

pub async fn run<N: Notifier>(
    explorer: Arc<ExplorerClient>,
    notifier: Arc<N>,
    state: Arc<SqliteStateRepo>,
    poll_interval: Duration,
    readiness: SummaryReadiness,
    metrics: Arc<Metrics>,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        for cadence in [Cadence::Daily, Cadence::Weekly, Cadence::Monthly] {
            if let Err(err) = maybe_post(
                &explorer,
                notifier.as_ref(),
                &state,
                cadence,
                &readiness,
                &metrics,
            )
            .await
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
    metrics: &Metrics,
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

    // Enrichment: every counted ticket carries a USD valuation. This is the
    // HARD completeness gate for a closed period.
    let enrichment_ok = priced >= ticket_count;
    // Cross-check + stability are ADVISORY. They used to be hard gates, which
    // meant a busy / late-settling day (figures still shifting across the hour,
    // or the bot's own ingest briefly behind) could wedge on `stable` and miss
    // the post entirely — the 2026-06-10 case. For a fully-closed UTC day the
    // explorer's rollups are authoritative once enrichment holds, so we post on
    // enrichment alone and only warn on the soft signals.
    let crosscheck_ok = ticket_count >= local_count;
    let stable = prior.as_ref() == Some(&snapshot);

    if enrichment_ok && !crosscheck_ok {
        tracing::warn!(
            ?cadence, %period_date, ticket_count, local_count,
            "crosscheck: explorer ticket_count below locally-ingested count; posting anyway"
        );
    }
    if enrichment_ok && !stable {
        tracing::info!(
            ?cadence, %period_date,
            "figures not yet identical across polls; posting on enrichment_ok"
        );
    }

    let ready = enrichment_ok;
    let past_deadline = now >= period_close + chrono::Duration::from_std(readiness.max_defer)?;

    if !ready {
        // Record this observation so the next poll can detect stability.
        state
            .upsert_summary_snapshot(cadence, period_date, &snapshot)
            .await?;

        if !past_deadline {
            let hours_since_close = (now - period_close).num_minutes() as f64 / 60.0;
            tracing::info!(
                ?cadence, %period_date,
                enrichment_ok, crosscheck_ok, stable,
                ticket_count, priced, local_count,
                hours_since_close,
                "summary not ready; deferring"
            );
            metrics.record_deferral(cadence);
            return Ok(());
        }

        tracing::warn!(
            ?cadence, %period_date,
            enrichment_ok, crosscheck_ok, stable,
            ticket_count, priced, local_count,
            "summary still not ready past max-defer deadline; posting with incomplete marker"
        );
    }

    // Best-effort: a leaderboard fetch failure must NOT block the summary post
    // (this is the safety net that should have caught 2026-06-10). The core
    // summary numbers are what matter; degrade to an empty leaderboard on error.
    let leaderboard_rows = match explorer.payout_leaderboard(range_from, range_to, 10).await {
        Ok(lb) => lb.data,
        Err(err) => {
            tracing::warn!(?cadence, %period_date, ?err, "leaderboard fetch failed; posting summary without it");
            Vec::new()
        }
    };
    let payload = build_summary(cadence, period_date, &summary, &leaderboard_rows, !ready);
    notifier.send(payload).await?;
    state.mark_summary_posted(cadence, period_date).await?;
    metrics.record_post(cadence);
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
