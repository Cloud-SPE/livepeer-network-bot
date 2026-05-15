//! Hand-written serde structs that mirror the relevant subset of the explorer
//! OpenAPI spec (`docs/generated/openapi.json`). The explorer encodes most
//! numeric values as decimal strings — preserve them as `String` here and parse
//! to `f64` only inside embed builders.
//!
//! See `docs/exec-plans/active/002-progenitor-codegen.md` — these structs will
//! be replaced by `progenitor`-generated types in a follow-up.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventListResponse {
    pub data: Vec<EventRow>,
    pub next_cursor: Option<String>,
    pub last_finalized_block: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EventRow {
    pub id: String,
    pub chain_id: String,
    pub tx_hash: String,
    pub log_index: i64,
    pub block_number: String,
    pub block_hash: String,
    pub block_timestamp: DateTime<Utc>,
    pub contract_address: String,
    pub contract_name: String,
    pub event_name: String,
    pub event_signature: String,
    pub asset: Option<String>,
    pub amount_native: Option<String>,
    pub is_valuable: bool,
    pub from_address: Option<String>,
    pub to_address: Option<String>,
    pub finality: String,
    pub is_canonical: bool,
    #[serde(default)]
    pub valuations: Option<Vec<ValuationInline>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ValuationInline {
    pub asset: String,
    pub valuation_version: String,
    pub amount_native: String,
    pub native_usd_price: Option<String>,
    pub amount_usd: Option<String>,
    pub source: Option<String>,
    pub pricing_method: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrchestratorProfileRow {
    pub address: String,
    pub total_stake: Option<String>,
    pub fee_cut_percent: Option<String>,
    pub fee_share_percent: Option<String>,
    pub reward_cut_percent: Option<String>,
    pub is_active: Option<bool>,
    pub as_of_block: Option<String>,
    pub as_of_round: Option<String>,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub service_uri: Option<String>,
    pub last_lifecycle_event_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GatewayProfileRow {
    pub address: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub kind: Option<String>,
    pub latest_deposit: Option<String>,
    pub latest_reserve: Option<String>,
    pub reserve_claimed_in_current_round: Option<String>,
    pub withdraw_round: Option<String>,
    pub unlock_in_progress: Option<bool>,
    pub as_of_block: Option<String>,
}

impl GatewayProfileRow {
    pub fn is_ai(&self) -> bool {
        matches!(self.kind.as_deref(), Some("ai"))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PayoutSummaryResponse {
    pub period_start: String,
    pub period_end: String,
    pub valuation_version: String,
    pub job_type: String,
    pub ticket_count: String,
    pub sum_face_value_native: String,
    pub sum_face_value_usd: String,
    pub sum_commission_native: String,
    pub sum_commission_usd: String,
    pub sum_delegators_share_native: String,
    pub sum_delegators_share_usd: String,
    pub distinct_gateways: String,
    pub usd_rows_priced: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PayoutLeaderboardResponse {
    pub data: Vec<PayoutLeaderboardRow>,
    pub meta: PayoutLeaderboardMeta,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PayoutLeaderboardMeta {
    pub chain_id: String,
    pub from: String,
    pub to: String,
    pub valuation_version: String,
    pub job_type: String,
    pub sort: String,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PayoutLeaderboardRow {
    pub orchestrator_address: String,
    pub ticket_count: String,
    pub sum_face_value_native: String,
    pub sum_face_value_usd: String,
    pub sum_commission_native: String,
    pub sum_commission_usd: String,
    pub sum_delegators_share_native: String,
    pub sum_delegators_share_usd: String,
    pub distinct_gateways: String,
    pub usd_rows_priced: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewardLeaderboardResponse {
    pub data: Vec<RewardLeaderboardRow>,
    pub meta: RewardLeaderboardMeta,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewardLeaderboardRow {
    pub orchestrator_address: String,
    pub reward_event_count: String,
    pub sum_total_tokens: String,
    pub sum_total_tokens_usd: String,
    pub sum_orch_tokens: String,
    pub sum_orch_tokens_usd: String,
    pub sum_delegators_tokens: String,
    pub sum_delegators_tokens_usd: String,
    pub usd_rows_priced: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RewardLeaderboardMeta {
    pub chain_id: String,
    pub from: String,
    pub to: String,
    pub valuation_version: String,
    pub sort: String,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrchDelegatorsResponse {
    pub data: Vec<OrchDelegatorRow>,
    pub meta: OrchDelegatorsMeta,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrchDelegatorRow {
    pub delegator_address: String,
    pub bonded_principal: String,
    pub pending_stake: Option<String>,
    pub pending_fees: Option<String>,
    pub pending_round: Option<String>,
    pub as_of_block: String,
    pub as_of_timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrchDelegatorsMeta {
    pub chain_id: String,
    pub orchestrator_address: String,
    pub next_cursor: Option<String>,
}

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
