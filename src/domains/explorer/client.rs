use std::time::Duration;

use chrono::NaiveDate;
use reqwest::{Client, StatusCode};
use url::Url;

/// Max attempts for a GET before surfacing the error (1 initial + 2 retries).
const MAX_GET_ATTEMPTS: u32 = 3;

use super::types::{
    Cadence, EventListResponse, EventRow, GatewayProfileRow, OrchDelegatorsResponse,
    OrchestratorListResponse, OrchestratorProfileRow, PayoutLeaderboardResponse,
    PayoutSummaryResponse, RewardLeaderboardResponse, RoundEventsResponse, RoundIndexRow,
    RoundsIndexResponse, TranscoderParamsHistoryResponse,
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

    /// GET a URL and deserialize the JSON body, retrying transient failures.
    ///
    /// The explorer API (behind a Cloudflare tunnel) intermittently returns
    /// 502/500 or times out under DB load. Those blips used to abort the whole
    /// caller (e.g. a summary poll or the event poller) via `?`. We now retry
    /// 5xx / 429 / connect / timeout failures a couple of times with a short
    /// exponential backoff. Non-retryable 4xx and JSON-decode errors surface
    /// immediately.
    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: Url) -> anyhow::Result<T> {
        let resp = self.get_with_retries(url).await?.error_for_status()?;
        Ok(resp.json().await?)
    }

    /// Like [`get_json`](Self::get_json), but maps a 404 to `Ok(None)` so
    /// callers can treat a missing resource (e.g. an orchestrator the
    /// explorer hasn't indexed yet) as data rather than a failure.
    async fn get_json_optional<T: serde::de::DeserializeOwned>(
        &self,
        url: Url,
    ) -> anyhow::Result<Option<T>> {
        let resp = self.get_with_retries(url).await?;
        if resp.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let resp = resp.error_for_status()?;
        Ok(Some(resp.json().await?))
    }

    /// GET with the retry policy; returns the final response without
    /// checking its status.
    async fn get_with_retries(&self, url: Url) -> anyhow::Result<reqwest::Response> {
        let mut attempt = 0u32;
        loop {
            attempt += 1;
            match self.client.get(url.clone()).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let retryable_status =
                        status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS;
                    if retryable_status && attempt < MAX_GET_ATTEMPTS {
                        let backoff = Duration::from_millis(300 * 2u64.pow(attempt - 1));
                        tracing::warn!(url = %url, attempt, %status, "explorer GET returned retryable status; retrying");
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    // Non-retryable status, or out of attempts: surface it.
                    return Ok(resp);
                }
                Err(err)
                    if attempt < MAX_GET_ATTEMPTS
                        && (err.is_timeout() || err.is_connect() || err.is_request()) =>
                {
                    let backoff = Duration::from_millis(300 * 2u64.pow(attempt - 1));
                    tracing::warn!(url = %url, attempt, error = %err, "explorer GET transport error; retrying");
                    tokio::time::sleep(backoff).await;
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }
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
        let resp: EventListResponse = self.get_json(url).await?;
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
        self.get_json(url).await
    }

    pub async fn get_orchestrator(&self, address: &str) -> anyhow::Result<OrchestratorProfileRow> {
        let url = self.url(&format!("api/v1/orchestrators/{address}"))?;
        let mut row: OrchestratorProfileRow = self.get_json(url).await?;
        row.avatar_url = absolutize_avatar(&self.base_url, row.avatar_url.take());
        Ok(row)
    }

    /// Orchestrator profile, with a 404 (address unknown to the explorer)
    /// surfaced as `Ok(None)` instead of an error.
    pub async fn try_get_orchestrator(
        &self,
        address: &str,
    ) -> anyhow::Result<Option<OrchestratorProfileRow>> {
        let url = self.url(&format!("api/v1/orchestrators/{address}"))?;
        let Some(mut row) = self
            .get_json_optional::<OrchestratorProfileRow>(url)
            .await?
        else {
            return Ok(None);
        };
        row.avatar_url = absolutize_avatar(&self.base_url, row.avatar_url.take());
        Ok(Some(row))
    }

    pub async fn get_gateway(&self, address: &str) -> anyhow::Result<GatewayProfileRow> {
        let url = self.url(&format!("api/v1/gateways/{address}/profile"))?;
        let mut row: GatewayProfileRow = self.get_json(url).await?;
        row.avatar_url = absolutize_avatar(&self.base_url, row.avatar_url.take());
        Ok(row)
    }

    /// Gateway profile, with a 404 (address unknown to the explorer)
    /// surfaced as `Ok(None)` instead of an error.
    pub async fn try_get_gateway(
        &self,
        address: &str,
    ) -> anyhow::Result<Option<GatewayProfileRow>> {
        let url = self.url(&format!("api/v1/gateways/{address}/profile"))?;
        let Some(mut row) = self.get_json_optional::<GatewayProfileRow>(url).await? else {
            return Ok(None);
        };
        row.avatar_url = absolutize_avatar(&self.base_url, row.avatar_url.take());
        Ok(Some(row))
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
        self.get_json(url).await
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
        self.get_json(url).await
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
        self.get_json(url).await
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
        self.get_json(url).await
    }

    /// Most recent protocol round (the `/rounds` index is newest-first).
    /// `None` only if the explorer returns an empty index.
    pub async fn latest_round(&self) -> anyhow::Result<Option<RoundIndexRow>> {
        let mut url = self.url("api/v1/rounds")?;
        url.query_pairs_mut().append_pair("limit", "1");
        let resp: RoundsIndexResponse = self.get_json(url).await?;
        Ok(resp.data.into_iter().next())
    }

    /// Events within one round, filtered to `kinds` (comma-separated event
    /// names). `meta.to_block` is `None` while the round is still open and set
    /// once the explorer has indexed the following round's start.
    pub async fn round_events(
        &self,
        round: i64,
        kinds: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> anyhow::Result<RoundEventsResponse> {
        let mut url = self.url(&format!("api/v1/rounds/{round}/events"))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("kinds", kinds);
            q.append_pair("limit", &limit.to_string());
            if let Some(c) = cursor {
                q.append_pair("cursor", c);
            }
        }
        self.get_json(url).await
    }

    /// Orchestrator profiles, optionally restricted to the current active set.
    pub async fn list_orchestrators(
        &self,
        cursor: Option<&str>,
        limit: u32,
        active_only: bool,
    ) -> anyhow::Result<OrchestratorListResponse> {
        let mut url = self.url("api/v1/orchestrators")?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair("limit", &limit.to_string());
            if active_only {
                q.append_pair("active_only", "true");
            }
            if let Some(c) = cursor {
                q.append_pair("cursor", c);
            }
        }
        self.get_json(url).await
    }

    pub async fn transcoder_params_history(
        &self,
        address: &str,
        limit: u32,
    ) -> anyhow::Result<TranscoderParamsHistoryResponse> {
        let mut url = self.url(&format!("api/v1/transcoders/{address}/params/history"))?;
        url.query_pairs_mut()
            .append_pair("limit", &limit.to_string());
        self.get_json(url).await
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
