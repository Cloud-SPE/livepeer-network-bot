use chrono::NaiveDate;
use reqwest::Client;
use url::Url;

use super::types::{
    Cadence, EventListResponse, GatewayProfileRow, OrchDelegatorsResponse, OrchestratorProfileRow,
    PayoutLeaderboardResponse, PayoutSummaryResponse, RewardLeaderboardResponse,
};

#[derive(Clone, Debug)]
pub struct ExplorerClient {
    client: Client,
    base_url: Url,
}

impl ExplorerClient {
    pub fn new(client: Client, base_url: Url) -> Self {
        Self { client, base_url }
    }

    fn url(&self, path: &str) -> anyhow::Result<Url> {
        Ok(self.base_url.join(path)?)
    }

    pub async fn list_winning_tickets(
        &self,
        cursor: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<EventListResponse> {
        self.list_events("WinningTicketRedeemed", cursor, limit)
            .await
    }

    /// Generic single-event-name listing with valuations. Each call passes
    /// one event_name (the explorer doesn't accept a CSV list).
    pub async fn list_events(
        &self,
        event_name: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<EventListResponse> {
        let mut url = self.url("api/v1/events")?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("event_name", event_name);
            q.append_pair("with_valuations", "true");
            q.append_pair("limit", &limit.to_string());
            if let Some(c) = cursor {
                q.append_pair("cursor", c);
            }
        }
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn get_orchestrator(&self, address: &str) -> anyhow::Result<OrchestratorProfileRow> {
        let url = self.url(&format!("api/v1/orchestrators/{address}"))?;
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn get_gateway(&self, address: &str) -> anyhow::Result<GatewayProfileRow> {
        let url = self.url(&format!("api/v1/gateways/{address}/profile"))?;
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn payout_summary(
        &self,
        cadence: Cadence,
        date: NaiveDate,
    ) -> anyhow::Result<PayoutSummaryResponse> {
        let path = format!(
            "api/v1/payouts/summary/{}/{}",
            cadence.as_path(),
            date.format("%Y-%m-%d")
        );
        let mut url = self.url(&path)?;
        url.query_pairs_mut().append_pair("job_type", "both");
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn payout_leaderboard(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        limit: u32,
    ) -> anyhow::Result<PayoutLeaderboardResponse> {
        let mut url = self.url("api/v1/payouts/leaderboard")?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("from", &from.format("%Y-%m-%d").to_string());
            q.append_pair("to", &to.format("%Y-%m-%d").to_string());
            q.append_pair("job_type", "both");
            q.append_pair("sort", "commission_usd");
            q.append_pair("limit", &limit.to_string());
        }
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn rewards_leaderboard(
        &self,
        from: NaiveDate,
        to: NaiveDate,
        limit: u32,
    ) -> anyhow::Result<RewardLeaderboardResponse> {
        let mut url = self.url("api/v1/rewards/leaderboard")?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("from", &from.format("%Y-%m-%d").to_string());
            q.append_pair("to", &to.format("%Y-%m-%d").to_string());
            q.append_pair("limit", &limit.to_string());
        }
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    pub async fn orchestrator_delegators(
        &self,
        address: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<OrchDelegatorsResponse> {
        let mut url = self.url(&format!("api/v1/orchestrators/{address}/delegators"))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("limit", &limit.to_string());
            if let Some(c) = cursor {
                q.append_pair("cursor", c);
            }
        }
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }
}
