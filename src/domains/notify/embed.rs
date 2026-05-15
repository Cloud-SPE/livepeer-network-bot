//! Embed builders. Output JSON must match `docs/product-specs/messages.md`
//! byte-for-byte — every format string and color constant in this file comes
//! from `livepeer-backend-rs/src/tasks/{ticket_digest,payout_summary}.rs`.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::{json, Value};

use crate::domains::{
    explorer::types::{
        Cadence, GatewayProfileRow, GatewayProfileRowExt, OrchestratorProfileRow,
        PayoutLeaderboardRow, PayoutSummaryResponse,
    },
    state::repo::{OrchTotals, StoredEvent},
};

const USERNAME: &str = "Payout Alert Bot";
const AVATAR_URL: &str = "https://cdn.discordapp.com/avatars/808142296959680532/338766470b721d9081680c7cb34921df.webp?size=80";
const TITLE: &str = "Orchestrator Payout";

const COLOR_SINGLE_AI: u32 = 16766720;
const COLOR_SINGLE_TX: u32 = 60296;
const COLOR_DIGEST_AI: u32 = 0xFFA500;
const COLOR_DIGEST_TX: u32 = 0xFFD700;

#[derive(Debug, Clone)]
pub struct TicketView {
    pub event: StoredEvent,
    pub gateway: GatewayProfileRow,
}

pub fn build_single_ticket(
    orch: &OrchestratorProfileRow,
    ticket: &TicketView,
    fee_cut: f64,
    totals_24h: &OrchTotals,
) -> Value {
    let face_value: f64 = parse_f64(&ticket.event.amount_native);
    let face_value_usd: f64 = parse_f64(&ticket.event.amount_usd);
    let eth_price: f64 = parse_f64(&ticket.event.native_usd_price);
    let orch_commission = face_value * fee_cut;
    let orch_commission_usd = face_value_usd * fee_cut;

    let is_ai = ticket.gateway.is_ai();
    let (color, job_sentence) = if is_ai {
        (COLOR_SINGLE_AI, "performing AI inference.")
    } else {
        (COLOR_SINGLE_TX, "transcoding video streams.")
    };

    let orch_addr = ticket.event.to_address.as_deref().unwrap_or("");
    let orch_name = orch
        .display_name
        .clone()
        .unwrap_or_else(|| orch_addr.to_string());
    let bcast_addr = ticket.event.from_address.as_deref().unwrap_or("");
    let bcast_name = ticket
        .gateway
        .display_name
        .clone()
        .unwrap_or_else(|| bcast_addr.to_string());

    let description = format!(
        "[**{}**](https://tools.livepeer.cloud/orchestrator/{}) just earned **{:.4} ETH ${:.2}**\n{}\n\n\
         Paid By [**{}**](https://tools.livepeer.cloud/broadcaster/{})\n\
         ETH Price **${:.2}**\n\
         Fee cut: **{:.2}%**\n\
         Commission: **{:.4} ETH (${:.2})**\n\n\
         24H Rolling Total\n\
         **{:.4} ETH (${:.2})**\n\
         Keeping {:.5} ETH (${:.2})",
        orch_name,
        orch_addr,
        face_value,
        face_value_usd,
        job_sentence,
        bcast_name,
        bcast_addr,
        eth_price,
        fee_cut * 100.0,
        orch_commission,
        orch_commission_usd,
        totals_24h.face_value_eth,
        totals_24h.face_value_usd,
        totals_24h.commission_eth,
        totals_24h.commission_usd,
    );

    let timestamp = rfc3339(ticket.event.block_timestamp);
    let url = format!("https://arbiscan.io/tx/{}", ticket.event.tx_hash);

    let mut embed = json!({
        "color": color,
        "title": TITLE,
        "description": description,
        "timestamp": timestamp,
        "url": url,
    });
    if let Some(thumb) = orch.avatar_url.as_deref() {
        embed
            .as_object_mut()
            .unwrap()
            .insert("thumbnail".into(), json!({ "url": thumb }));
    }

    envelope(embed)
}

pub fn build_digest(
    orch: &OrchestratorProfileRow,
    orch_addr: &str,
    is_ai: bool,
    tickets: &[TicketView],
    fee_cut: f64,
    window_end: DateTime<Utc>,
    totals_24h: &OrchTotals,
) -> Value {
    let mut sum_face_eth = 0.0;
    let mut sum_face_usd = 0.0;
    let mut eth_price = 0.0;

    use std::collections::BTreeMap;
    let mut by_bcast: BTreeMap<String, (String, usize, f64)> = BTreeMap::new();

    for t in tickets {
        let fv = parse_f64(&t.event.amount_native);
        let fv_usd = parse_f64(&t.event.amount_usd);
        sum_face_eth += fv;
        sum_face_usd += fv_usd;
        if eth_price == 0.0 {
            eth_price = parse_f64(&t.event.native_usd_price);
        }
        let bcast_addr = t.event.from_address.clone().unwrap_or_default();
        let bcast_name = t
            .gateway
            .display_name
            .clone()
            .unwrap_or_else(|| bcast_addr.clone());
        let entry = by_bcast.entry(bcast_addr).or_insert((bcast_name, 0, 0.0));
        entry.1 += 1;
        entry.2 += fv;
    }

    let sum_keep_eth = sum_face_eth * fee_cut;
    let sum_keep_usd = sum_face_usd * fee_cut;
    let avg_fee_cut = fee_cut * 100.0;

    let mut gateways: Vec<_> = by_bcast.into_iter().collect();
    gateways.sort_by(|a, b| b.1 .2.total_cmp(&a.1 .2));
    let gateway_lines: Vec<String> = gateways
        .into_iter()
        .take(3)
        .map(|(eth, (name, count, total_eth))| {
            format!(
                "• [{}](https://tools.livepeer.cloud/broadcaster/{}) — {} Tickets for {:.4} ETH",
                name, eth, count, total_eth
            )
        })
        .collect();

    let (color, job_sentence) = if is_ai {
        (COLOR_DIGEST_AI, "performing AI inference.")
    } else {
        (COLOR_DIGEST_TX, "transcoding video streams.")
    };

    let orch_name = orch
        .display_name
        .clone()
        .unwrap_or_else(|| orch_addr.to_string());

    let description = format!(
        "[**{}**](https://tools.livepeer.cloud/orchestrator/{}) just earned **{:.4} ETH ${:.2}**\n{}\n\n\
         Paid By:\n{}\n\n\
         ETH Price **${:.2}**\n\
         Fee cut: **{:.2}%**\n\
         Commission: **{:.4} ETH (${:.2})**\n\n\
         24H Rolling Total\n\
         **{:.4} ETH (${:.2})**\n\
         Keeping {:.5} ETH (${:.2})",
        orch_name,
        orch_addr,
        sum_face_eth,
        sum_face_usd,
        job_sentence,
        gateway_lines.join("\n"),
        eth_price,
        avg_fee_cut,
        sum_keep_eth,
        sum_keep_usd,
        totals_24h.face_value_eth,
        totals_24h.face_value_usd,
        totals_24h.commission_eth,
        totals_24h.commission_usd,
    );

    let url = format!(
        "https://arbiscan.io/address/{}?mtd=0xec8b3cb6~Redeem%20Winning%20Ticket",
        orch_addr
    );

    let mut embed = json!({
        "color": color,
        "title": TITLE,
        "description": description,
        "timestamp": rfc3339(window_end),
        "url": url,
    });
    if let Some(thumb) = orch.avatar_url.as_deref() {
        embed
            .as_object_mut()
            .unwrap()
            .insert("thumbnail".into(), json!({ "url": thumb }));
    }

    envelope(embed)
}

pub fn build_summary(
    cadence: Cadence,
    period_date: NaiveDate,
    summary: &PayoutSummaryResponse,
    leaderboard: &[PayoutLeaderboardRow],
) -> Value {
    let total_ticket: f64 = parse_f64(&Some(summary.ticket_count.clone()));
    let total_eth: f64 = parse_f64(&Some(summary.sum_face_value_native.clone()));
    let total_orch_commission_eth: f64 = parse_f64(&Some(summary.sum_commission_native.clone()));
    let total_orchs = leaderboard.len();

    let network_block = format!(
        "```css\n{} winning tickets\n{} orchestrators earned\nTranscoding Fees: {:.4} ETH\nOrch Commission {:.4}\n    ```",
        total_ticket as u64, total_orchs, total_eth, total_orch_commission_eth
    );

    let mut orch_blocks = String::new();
    for (i, row) in leaderboard.iter().enumerate() {
        let rank = i + 1;
        let orch_name = row
            .display_name
            .clone()
            .unwrap_or_else(|| row.orchestrator_address.clone());
        let orch_total_ticket: f64 = parse_f64(&Some(row.ticket_count.clone()));
        let orch_total_eth: f64 = parse_f64(&Some(row.sum_face_value_native.clone()));
        let orch_total_commission_eth: f64 = parse_f64(&Some(row.sum_commission_native.clone()));
        let orch_total_percent = if total_eth > 0.0 {
            100.0 * orch_total_eth / total_eth
        } else {
            0.0
        };
        let orch_total_commission_percent = if total_orch_commission_eth > 0.0 {
            100.0 * orch_total_commission_eth / total_orch_commission_eth
        } else {
            0.0
        };
        orch_blocks.push_str(&format!(
            "```\n#{}: {} won {} tickets\nTotal {:.4} ETH ({:.2}%)\nCommission: {:.4} ETH ({:.2}%)\n        ```",
            rank,
            orch_name,
            orch_total_ticket as u64,
            orch_total_eth,
            orch_total_percent,
            orch_total_commission_eth,
            orch_total_commission_percent,
        ));
    }

    let description = format!("{}{}", network_block, orch_blocks);

    let title = format!(
        "{} Payout Summary  (*{}*)",
        cadence.title_word(),
        period_date.format("%B %e %Y")
    );
    let url = format!(
        "https://tools.livepeer.cloud/payout/{}/summary/{}",
        cadence.as_path(),
        period_date.format("%Y-%m-%d")
    );

    json!({
        "username": USERNAME,
        "avatar_url": AVATAR_URL,
        "embeds": [{
            "color": "60296",
            "title": title,
            "description": description,
            "url": url,
        }]
    })
}

fn envelope(embed: Value) -> Value {
    json!({
        "username": USERNAME,
        "avatar_url": AVATAR_URL,
        "embeds": [embed]
    })
}

fn parse_f64(s: &Option<String>) -> f64 {
    s.as_deref()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn rfc3339(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339()
}
