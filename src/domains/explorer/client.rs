use chrono::NaiveDate;
use reqwest::Client;
use url::Url;

use super::types::{
    Cadence, EventListResponse, EventRow, GatewayProfileRow, OrchDelegatorsResponse,
    OrchestratorProfileRow, PayoutLeaderboardResponse, PayoutSummaryResponse,
    RewardLeaderboardResponse, TranscoderParamsHistoryResponse,
};

#[derive(Clone, Debug)]
pub struct ExplorerClient {
    client: Client,
    base_url: Url,
}

/// Resolve a profile `avatar_url` from the explorer into an absolute,
/// publicly-fetchable URL.
///
/// TD-033: the explorer serves locally-cached avatars (ENS records it
/// resolved to image bytes — including ipfs:// and eip155 NFT references)
/// as a root-relative path `/api/v1/orchestrators/{addr}/avatar`. Discord
/// fetches embed thumbnails server-side and needs an absolute URL, so we
/// join the relative path onto the explorer's public base URL. Absolute
/// values (http(s) passthrough, or still-unresolved ipfs/eip155 records)
/// are left untouched — downstream thumbnail validation drops the non-http
/// ones, exactly as before.
fn absolutize_avatar(base: &Url, raw: Option<String>) -> Option<String> {
    let raw = raw?;
    if raw.starts_with('/') {
        match base.join(&raw) {
            Ok(abs) => Some(abs.to_string()),
            Err(_) => Some(raw),
        }
    } else {
        Some(raw)
    }
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

    pub async fn get_winning_ticket_by_tx_hash(
        &self,
        tx_hash: &str,
    ) -> anyhow::Result<Option<EventRow>> {
        let mut url = self.url("api/v1/events")?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("event_name", "WinningTicketRedeemed");
            q.append_pair("tx_hash", tx_hash);
            q.append_pair("with_valuations", "true");
            q.append_pair("limit", "10");
        }
        let resp: EventListResponse = self
            .client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp.data.into_iter().find(|ev| ev.tx_hash == tx_hash))
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
        let mut row: OrchestratorProfileRow = resp.json().await?;
        row.avatar_url = absolutize_avatar(&self.base_url, row.avatar_url.take());
        Ok(row)
    }

    pub async fn get_gateway(&self, address: &str) -> anyhow::Result<GatewayProfileRow> {
        let url = self.url(&format!("api/v1/gateways/{address}/profile"))?;
        let resp = self.client.get(url).send().await?.error_for_status()?;
        let mut row: GatewayProfileRow = resp.json().await?;
        row.avatar_url = absolutize_avatar(&self.base_url, row.avatar_url.take());
        Ok(row)
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

    pub async fn transcoder_params_history(
        &self,
        address: &str,
        limit: u32,
    ) -> anyhow::Result<TranscoderParamsHistoryResponse> {
        let mut url = self.url(&format!("api/v1/transcoders/{address}/params/history"))?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        let resp = self.client.get(url).send().await?.error_for_status()?;
        Ok(resp.json().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::absolutize_avatar;
    use url::Url;

    fn base() -> Url {
        Url::parse("https://livepeer-network-api.cloudspe.com").unwrap()
    }

    #[test]
    fn relative_cached_avatar_is_absolutized() {
        let got = absolutize_avatar(
            &base(),
            Some("/api/v1/orchestrators/0xabc/avatar".to_string()),
        );
        assert_eq!(
            got.as_deref(),
            Some("https://livepeer-network-api.cloudspe.com/api/v1/orchestrators/0xabc/avatar")
        );
    }

    #[test]
    fn absolute_http_passthrough_is_untouched() {
        let got = absolutize_avatar(&base(), Some("https://override.example/a.png".to_string()));
        assert_eq!(got.as_deref(), Some("https://override.example/a.png"));
    }

    #[test]
    fn non_http_records_are_left_for_downstream_to_drop() {
        // ipfs/eip155 records that the explorer couldn't cache pass through
        // unchanged; the embed thumbnail validator drops them.
        assert_eq!(
            absolutize_avatar(&base(), Some("eip155:1/erc721:0xabc/123".to_string())).as_deref(),
            Some("eip155:1/erc721:0xabc/123")
        );
        assert_eq!(absolutize_avatar(&base(), None), None);
    }
}
