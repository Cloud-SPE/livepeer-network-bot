//! Bond / Unbond / Rebond event poller.
//!
//! Each tick runs three paginated fetches sequentially, one per event_name
//! (the explorer's `/events` endpoint accepts a single `event_name` per
//! query). New rows are persisted into `delegator_events`; on `Bond`, the
//! `(delegator_address, orch_address)` pair is also upserted into
//! `delegator_history` so the 004c digest can label subsequent Bonds as
//! "new delegator" vs "stake change".
//!
//! No DM fan-out happens here — that's the `subscriber_digest_poster`
//! responsibility in 004c. This poller purely persists.

use std::{sync::Arc, time::Duration};

use tokio::time::{interval, MissedTickBehavior};

use crate::domains::{
    explorer::client::ExplorerClient,
    state::{event_streams::EventStreamsRepo, repo::SqliteStateRepo},
};

const EVENT_NAMES: &[&str] = &["Bond", "Unbond", "Rebond"];
const PAGE_LIMIT: u32 = 100;

pub async fn run(
    explorer: Arc<ExplorerClient>,
    streams: Arc<EventStreamsRepo>,
    state: Arc<SqliteStateRepo>,
    poll_interval: Duration,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        for &event_name in EVENT_NAMES {
            if let Err(err) = run_one(&explorer, &streams, &state, event_name).await {
                tracing::error!(?err, %event_name, "delegator poller failed");
            }
        }
    }
}

async fn run_one(
    explorer: &ExplorerClient,
    streams: &EventStreamsRepo,
    state: &SqliteStateRepo,
    event_name: &str,
) -> anyhow::Result<()> {
    let cursor_name = format!("events:{event_name}");
    let mut cursor = state.get_cursor(&cursor_name).await?;

    loop {
        let resp = explorer
            .list_events(event_name, cursor.as_deref(), PAGE_LIMIT)
            .await?;
        if resp.data.is_empty() {
            break;
        }

        for ev in &resp.data {
            let inserted = streams.insert_delegator_event(ev).await?;
            if inserted && event_name == "Bond" {
                if let (Some(d), Some(o)) = (ev.from_address.as_deref(), ev.to_address.as_deref()) {
                    let _ = streams.record_first_seen(d, o).await;
                }
            }
        }

        match resp.next_cursor {
            Some(next) => {
                state.set_cursor(&cursor_name, &next).await?;
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
