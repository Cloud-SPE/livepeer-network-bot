//! Explorer API types.
//!
//! All struct definitions are re-exports from `super::generated::types`
//! (produced by `progenitor` at build time from
//! `docs/generated/openapi.json`). Re-exporting here keeps the import path
//! (`crate::domains::explorer::types::X`) stable across the codebase even
//! as the upstream spec evolves.
//!
//! Bot-internal helpers that don't have an OpenAPI representation
//! (`Cadence`, `GatewayProfileRowExt`) live in this module directly.

pub use super::generated::types::{
    EventListResponse, EventRow, GatewayProfileRow, OrchDelegatorRow, OrchDelegatorsMeta,
    OrchDelegatorsResponse, OrchestratorListResponse, OrchestratorProfileRow,
    PayoutLeaderboardMeta, PayoutLeaderboardResponse, PayoutLeaderboardRow, PayoutSummaryResponse,
    ProfileListMeta, RewardLeaderboardMeta, RewardLeaderboardResponse, RewardLeaderboardRow,
    RoundEventRow, RoundEventsMeta, RoundEventsResponse, RoundIndexRow, RoundsIndexMeta,
    RoundsIndexResponse, TranscoderParamsHistoryResponse, TranscoderParamsRow, ValuationInline,
};

pub fn preferred_valuation(valuations: &[ValuationInline]) -> Option<&ValuationInline> {
    valuations.iter().max_by_key(|v| {
        (
            status_rank(&v.status),
            has_positive_number(v.amount_usd.as_deref()),
            has_positive_number(v.native_usd_price.as_deref()),
        )
    })
}

/// Period selector for the daily / weekly / monthly summary endpoints. Not
/// in the OpenAPI spec — this is how the bot models the three URL templates
/// uniformly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    Daily,
    Weekly,
    Monthly,
}

impl Cadence {
    pub fn as_path(&self) -> &'static str {
        match self {
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
        }
    }

    pub fn title_word(&self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
        }
    }
}

/// Inherent-style helper exposed via extension trait because the underlying
/// `GatewayProfileRow` is generated code we can't add `impl` blocks to from
/// this module.
pub trait GatewayProfileRowExt {
    fn is_ai(&self) -> bool;
}

impl GatewayProfileRowExt for GatewayProfileRow {
    fn is_ai(&self) -> bool {
        self.kind == "ai"
    }
}

fn status_rank(status: &str) -> u8 {
    match status {
        "priced" => 3,
        "priced_with_warning" => 2,
        _ => 1,
    }
}

fn has_positive_number(raw: Option<&str>) -> bool {
    raw.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) > 0.0
}
