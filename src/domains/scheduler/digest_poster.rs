use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::Utc;
use tokio::time::{interval, MissedTickBehavior};

use crate::domains::{
    explorer::{
        client::ExplorerClient,
        types::{GatewayProfileRow, GatewayProfileRowExt, OrchestratorProfileRow},
    },
    notify::{
        embed::{build_digest, build_single_ticket, TicketView},
        service::Notifier,
    },
    state::repo::{SqliteStateRepo, StoredEvent},
};

pub async fn run<N: Notifier>(
    explorer: Arc<ExplorerClient>,
    notifier: Arc<N>,
    state: Arc<SqliteStateRepo>,
    window: Duration,
) {
    let mut tick = interval(window);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // First tick fires immediately; skip it so the first real run has data.
    tick.tick().await;

    loop {
        tick.tick().await;
        if let Err(err) = run_once(&explorer, notifier.as_ref(), &state, window).await {
            tracing::error!(?err, "digest poster iteration failed");
        }
    }
}

async fn run_once<N: Notifier>(
    explorer: &ExplorerClient,
    notifier: &N,
    state: &SqliteStateRepo,
    window: Duration,
) -> anyhow::Result<()> {
    let end = Utc::now();
    let start = end - chrono::Duration::from_std(window)?;

    let events = state.fetch_unsent_between(start, end).await?;
    if events.is_empty() {
        return Ok(());
    }

    let mut by_orch: HashMap<String, Vec<StoredEvent>> = HashMap::new();
    for ev in events {
        let Some(addr) = ev.to_address.clone() else {
            continue;
        };
        by_orch.entry(addr).or_default().push(ev);
    }

    for (orch_addr, mut tickets) in by_orch {
        tickets.sort_by_key(|t| t.block_timestamp);

        let orch = explorer.get_orchestrator(&orch_addr).await?;
        let fee_cut = parse_fee_cut(&orch);

        let mut gateways: HashMap<String, GatewayProfileRow> = HashMap::new();
        for t in &tickets {
            if let Some(addr) = t.from_address.as_deref() {
                if !gateways.contains_key(addr) {
                    let gw = explorer.get_gateway(addr).await?;
                    gateways.insert(addr.to_string(), gw);
                }
            }
        }

        let totals = state
            .orch_totals_since(&orch_addr, end - chrono::Duration::hours(24), fee_cut)
            .await?;

        let views: Vec<TicketView> = tickets
            .iter()
            .filter_map(|t| {
                let addr = t.from_address.as_deref()?;
                let gw = gateways.get(addr).cloned()?;
                Some(TicketView {
                    event: t.clone(),
                    gateway: gw,
                })
            })
            .collect();

        let sent_ids = if views.len() == 1 {
            let ticket_id = views[0].event.id.clone();
            let payload = build_single_ticket(&orch, &views[0], fee_cut, &totals);
            match notifier.send(payload).await {
                Ok(()) => vec![ticket_id],
                Err(err) => {
                    tracing::error!(?err, orch=%orch_addr, "single-ticket post failed");
                    Vec::new()
                }
            }
        } else {
            let (ai, tx): (Vec<_>, Vec<_>) = views.into_iter().partition(|v| v.gateway.is_ai());
            let mut all_ids = Vec::new();
            let mut batch_ok = true;
            for (group, is_ai) in [(&ai, true), (&tx, false)] {
                if group.is_empty() {
                    continue;
                }
                let payload = build_digest(&orch, &orch_addr, is_ai, group, fee_cut, end, &totals);
                match notifier.send(payload).await {
                    Ok(()) => all_ids.extend(group.iter().map(|v| v.event.id.clone())),
                    Err(err) => {
                        batch_ok = false;
                        tracing::error!(?err, orch=%orch_addr, ai=is_ai, "digest post failed");
                    }
                }
            }
            if batch_ok {
                all_ids
            } else {
                Vec::new()
            }
        };

        if !sent_ids.is_empty() {
            state.mark_sent(&sent_ids).await?;
        }
    }

    Ok(())
}

fn parse_fee_cut(orch: &OrchestratorProfileRow) -> f64 {
    orch.fee_cut_percent
        .parse::<f64>()
        .map(|p| p / 100.0)
        .unwrap_or(0.0)
}
