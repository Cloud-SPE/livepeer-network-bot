use std::{sync::Arc, time::Duration};

use chrono::{Datelike, NaiveDate, Utc};
use tokio::time::{interval, MissedTickBehavior};

use crate::domains::{
    explorer::{client::ExplorerClient, types::Cadence},
    notify::{embed::build_summary, service::Notifier},
    state::repo::SqliteStateRepo,
};

pub async fn run<N: Notifier>(
    explorer: Arc<ExplorerClient>,
    notifier: Arc<N>,
    state: Arc<SqliteStateRepo>,
    poll_interval: Duration,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        for cadence in [Cadence::Daily, Cadence::Weekly, Cadence::Monthly] {
            if let Err(err) = maybe_post(&explorer, notifier.as_ref(), &state, cadence).await {
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
) -> anyhow::Result<()> {
    let (period_date, range_from, range_to) = last_closed_period(cadence, Utc::now().date_naive());

    if state.summary_posted(cadence, period_date).await? {
        return Ok(());
    }

    let summary = explorer.payout_summary(cadence, period_date).await?;
    let leaderboard = explorer
        .payout_leaderboard(range_from, range_to, 10)
        .await?;

    let payload = build_summary(cadence, period_date, &summary, &leaderboard.data);
    notifier.send(payload).await?;
    state.mark_summary_posted(cadence, period_date).await?;
    tracing::info!(?cadence, %period_date, "summary posted");
    Ok(())
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
