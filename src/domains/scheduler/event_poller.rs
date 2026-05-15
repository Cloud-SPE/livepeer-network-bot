use std::{sync::Arc, time::Duration};

use tokio::time::{interval, MissedTickBehavior};

use crate::domains::{explorer::client::ExplorerClient, state::repo::SqliteStateRepo};

const CURSOR_NAME: &str = "events:WinningTicketRedeemed";
const PAGE_LIMIT: u32 = 100;

pub async fn run(
    explorer: Arc<ExplorerClient>,
    state: Arc<SqliteStateRepo>,
    poll_interval: Duration,
) {
    let mut tick = interval(poll_interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        match run_once(&explorer, &state).await {
            Ok(n) => {
                if n > 0 {
                    tracing::info!(inserted = n, "event poller fetched new tickets");
                }
            }
            Err(err) => tracing::error!(?err, "event poller iteration failed"),
        }
    }
}

async fn run_once(explorer: &ExplorerClient, state: &SqliteStateRepo) -> anyhow::Result<usize> {
    let mut inserted = 0usize;
    let mut cursor = state.get_cursor(CURSOR_NAME).await?;

    loop {
        let resp = explorer
            .list_winning_tickets(cursor.as_deref(), PAGE_LIMIT)
            .await?;

        if resp.data.is_empty() {
            break;
        }

        for ev in &resp.data {
            if state.insert_event(ev).await? {
                inserted += 1;
            }
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

    Ok(inserted)
}
