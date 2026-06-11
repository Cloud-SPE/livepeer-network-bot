//! Direct-message embed builders. Returns `serenity::all::CreateMessage`
//! rather than `serde_json::Value` because DMs flow through serenity's HTTP
//! client (see `providers::discord_bot`), not through a raw webhook POST.

use std::fmt::Write;

use chrono::{DateTime, Utc};
use poise::serenity_prelude::{Colour, CreateEmbed, CreateMessage, Timestamp};

use crate::domains::{
    explorer::types::OrchestratorProfileRow,
    state::event_streams::{CutChangeEventRow, DelegatorEventRow, RewardEventRow},
};

/// Reward event DM — fired per-event by the reward poller (004b).
pub fn build_reward_event_dm(
    orch: &OrchestratorProfileRow,
    event: &RewardEventRow,
) -> CreateMessage {
    let orch_addr = &event.orch_address;
    let orch_name = orch
        .display_name
        .clone()
        .unwrap_or_else(|| orch_addr.clone());

    let lpt = parse_decimal(event.amount_native.as_deref());
    let usd = parse_decimal(event.amount_usd.as_deref());

    let mut description = format!(
        "[**{}**](https://tools.livepeer.cloud/orchestrator/{}) earned **{:.4} LPT**",
        orch_name, orch_addr, lpt,
    );
    if usd > 0.0 {
        description.push_str(&format!(" (~${:.2})", usd));
    }
    description.push_str(" in inflation rewards.\n\n");
    description.push_str(&format!(
        "[View transaction](https://arbiscan.io/tx/{})",
        event.tx_hash
    ));

    let mut embed = CreateEmbed::new()
        .title("Reward earned")
        .description(description)
        .colour(Colour::from_rgb(0xff, 0xa5, 0x00))
        .timestamp(Timestamp::from(event.block_timestamp));

    if let Some(thumb) = orch.avatar_url.as_deref() {
        embed = embed.thumbnail(thumb);
    }

    CreateMessage::new().embed(embed)
}

/// Cut-change DM — fired per TranscoderUpdate observed for a subscribed
/// orchestrator.
pub fn build_cut_change_dm(
    orch: &OrchestratorProfileRow,
    event: &CutChangeEventRow,
) -> CreateMessage {
    let orch_addr = &event.orch_address;
    let orch_name = orch
        .display_name
        .clone()
        .unwrap_or_else(|| orch_addr.clone());

    let description = format!(
        "[**{}**](https://tools.livepeer.cloud/orchestrator/{}) updated its cuts:\n\n\
         Reward cut: **{}** (orchestrator)\n\
         Fee Share: **{}** (delegators)\n\
         Fee Cut: **{}** (orchestrator)\n\n\
         [View transaction](https://arbiscan.io/tx/{})",
        orch_name,
        orch_addr,
        format_percent(&event.reward_cut_percent),
        format_percent(&event.fee_share_percent),
        format_percent(&event.fee_cut_percent),
        event.tx_hash,
    );

    let mut embed = CreateEmbed::new()
        .title("Cut change")
        .description(description)
        .colour(Colour::from_rgb(0x96, 0x96, 0x96))
        .timestamp(Timestamp::from(event.block_timestamp));

    if let Some(thumb) = orch.avatar_url.as_deref() {
        embed = embed.thumbnail(thumb);
    }

    CreateMessage::new().embed(embed)
}

/// One bucket of the delegator digest. Bonds get pre-classified by the
/// scheduler into `new` vs `stake_change`; Unbonds and Rebonds are kept as-is.
#[derive(Debug, Default)]
pub struct DelegatorDigest<'a> {
    pub new_bonds: Vec<&'a DelegatorEventRow>,
    pub stake_change_bonds: Vec<&'a DelegatorEventRow>,
    pub unbonds: Vec<&'a DelegatorEventRow>,
    pub rebonds: Vec<&'a DelegatorEventRow>,
}

impl DelegatorDigest<'_> {
    pub fn is_empty(&self) -> bool {
        self.new_bonds.is_empty()
            && self.stake_change_bonds.is_empty()
            && self.unbonds.is_empty()
            && self.rebonds.is_empty()
    }
}

/// Subscriber digest DM — fired every `SUBSCRIBER_DIGEST_INTERVAL_SECS` by
/// the subscriber digest poster (004c). One message per (subscriber, orch)
/// covering all Bond / Unbond / Rebond events in the window.
pub fn build_delegator_digest_dm(
    orch: &OrchestratorProfileRow,
    orch_addr: &str,
    digest: &DelegatorDigest<'_>,
    window_end: DateTime<Utc>,
) -> CreateMessage {
    let orch_name = orch
        .display_name
        .clone()
        .unwrap_or_else(|| orch_addr.to_string());

    let mut description = format!(
        "[**{}**](https://tools.livepeer.cloud/orchestrator/{}) had delegator activity:\n",
        orch_name, orch_addr
    );

    if !digest.new_bonds.is_empty() {
        let _ = write!(
            description,
            "\n**New delegators ({})**\n",
            digest.new_bonds.len()
        );
        for ev in &digest.new_bonds {
            append_delegator_line(&mut description, ev, "bonded");
        }
    }

    if !digest.stake_change_bonds.is_empty() {
        let _ = write!(
            description,
            "\n**Stake increases ({})**\n",
            digest.stake_change_bonds.len()
        );
        for ev in &digest.stake_change_bonds {
            append_delegator_line(&mut description, ev, "added");
        }
    }

    if !digest.unbonds.is_empty() {
        let _ = write!(description, "\n**Unbonds ({})**\n", digest.unbonds.len());
        for ev in &digest.unbonds {
            append_delegator_line(&mut description, ev, "unbonded");
        }
    }

    if !digest.rebonds.is_empty() {
        let _ = write!(description, "\n**Rebonds ({})**\n", digest.rebonds.len());
        for ev in &digest.rebonds {
            append_delegator_line(&mut description, ev, "rebonded");
        }
    }

    let mut embed = CreateEmbed::new()
        .title("Delegator activity")
        .description(description)
        .colour(Colour::from_rgb(0x46, 0xa7, 0x58))
        .timestamp(Timestamp::from(window_end));

    if let Some(thumb) = orch.avatar_url.as_deref() {
        embed = embed.thumbnail(thumb);
    }

    CreateMessage::new().embed(embed)
}

fn append_delegator_line(buf: &mut String, ev: &DelegatorEventRow, verb: &str) {
    let lpt = parse_decimal(ev.amount_native.as_deref());
    let _ = writeln!(
        buf,
        "• [`{}`](https://arbiscan.io/tx/{}) {} **{:.4} LPT**",
        short_addr(&ev.delegator_address),
        ev.tx_hash,
        verb,
        lpt
    );
}

fn short_addr(addr: &str) -> String {
    if addr.len() >= 12 {
        format!("{}…{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

fn parse_decimal(s: Option<&str>) -> f64 {
    s.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
}

fn format_percent(raw: &str) -> String {
    format!("{:.2}%", parse_decimal(Some(raw)))
}
