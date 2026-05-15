//! Direct-message embed builders. Returns `serenity::all::CreateMessage`
//! rather than `serde_json::Value` because DMs flow through serenity's HTTP
//! client (see `providers::discord_bot`), not through a raw webhook POST.

use poise::serenity_prelude::{Colour, CreateEmbed, CreateMessage, Timestamp};

use crate::domains::{
    explorer::types::OrchestratorProfileRow, state::event_streams::RewardEventRow,
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

fn parse_decimal(s: Option<&str>) -> f64 {
    s.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
}
