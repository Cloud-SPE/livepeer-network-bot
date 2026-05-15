//! Slash command handlers + shared types.
//!
//! All commands operate via `poise::Context<'_, BotData, CommandError>` and
//! reply with ephemeral embeds built via `serenity::all::CreateEmbed` (NOT
//! the `serde_json::Value` embeds used by the public-channel webhook posters).

use std::sync::Arc;

use crate::domains::{
    explorer::client::ExplorerClient, state::event_streams::EventStreamsRepo,
    subscriptions::repo::SqliteSubscriptionsRepo,
};

pub mod orchestrator;
pub mod subscribe;
pub mod subscriptions;
pub mod unsubscribe;

pub type CommandError = Box<dyn std::error::Error + Send + Sync>;
pub type CommandContext<'a> = poise::Context<'a, BotData, CommandError>;

/// Data shared across every command invocation. Cloned cheaply (via `Arc`).
#[derive(Debug)]
pub struct BotData {
    pub explorer: Arc<ExplorerClient>,
    pub subscriptions: Arc<SqliteSubscriptionsRepo>,
    pub streams: Arc<EventStreamsRepo>,
    pub max_subscriptions_per_user: u32,
}

pub fn all_commands() -> Vec<poise::Command<BotData, CommandError>> {
    vec![
        subscribe::subscribe(),
        unsubscribe::unsubscribe(),
        subscriptions::subscriptions(),
        orchestrator::orchestrator(),
    ]
}

/// Validate that a string looks like a 0x-prefixed Ethereum address.
/// We accept mixed case and normalize to lowercase elsewhere.
pub fn is_valid_eth_address(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Truncate an address for display: `0x1234…5678`.
pub fn short_addr(addr: &str) -> String {
    if addr.len() >= 12 {
        format!("{}…{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

/// Parse a decimal-string field (the explorer encodes all numerics as
/// strings) into `f64` with a fallback to `0.0` for empty / unparsable values.
pub fn parse_f64_or_zero(s: &str) -> f64 {
    s.parse().unwrap_or(0.0)
}
