//! Discord webhook client with bucket-aware rate limiting + bounded 5xx
//! retry + structured metrics.
//!
//! Honors the following response headers on every send:
//!
//!   x-ratelimit-remaining     (i64)  — capacity left in the current bucket
//!   x-ratelimit-reset-after   (f64)  — seconds until the bucket refills
//!   x-ratelimit-scope         (str)  — "user" | "shared" | "global" on 429s
//!   retry-after               (f64)  — seconds to wait, on 429s
//!
//! Plus the 429 JSON body fields `retry_after` (preferred over header when
//! present) and `global` (treated as a global-scope 429).
//!
//! Retry policy:
//!   2xx                       → return Ok immediately
//!   429 (per-route or global) → respect the wait, retry up to MAX_ATTEMPTS
//!   5xx                       → exponential backoff (base 1s, cap 30s),
//!                               retry up to MAX_ATTEMPTS
//!   other 4xx (400/403/404…)  → return Err with no retry
//!
//! Metrics are emitted as `tracing::info!`/`warn!` events on the
//! `discord_metrics` target. Stable message names + stable field names let
//! log scrapers (Vector / Loki / promtail) derive counters and histograms
//! without us adding a `/metrics` endpoint.

use std::time::Duration;

use reqwest::{header::HeaderMap, Client, StatusCode};
use serde_json::Value;
use tokio::{
    sync::Mutex,
    time::{Instant as TokioInstant, Instant},
};
use url::Url;

use crate::domains::notify::service::Notifier;

const MAX_ATTEMPTS: u32 = 3;
const BACKOFF_BASE_MS: u64 = 1000;
const BACKOFF_CAP_MS: u64 = 30_000;

pub struct DiscordWebhook {
    client: Client,
    webhook_url: Url,
    state: Mutex<BucketState>,
}

#[derive(Debug)]
struct BucketState {
    /// Earliest `Instant` at which the next request may fire. `None` means
    /// no constraint is currently known.
    gate: Option<Instant>,
    /// Most recently observed `x-ratelimit-remaining`. `-1` means unknown.
    remaining: i64,
}

impl DiscordWebhook {
    pub fn new(client: Client, webhook_url: Url) -> Self {
        Self {
            client,
            webhook_url,
            state: Mutex::new(BucketState {
                gate: None,
                remaining: -1,
            }),
        }
    }
}

#[allow(clippy::manual_async_fn)]
impl Notifier for DiscordWebhook {
    fn send(&self, payload: Value) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        async move {
            let mut state = self.state.lock().await;

            for attempt in 1..=MAX_ATTEMPTS {
                wait_for_gate(&mut state).await;

                let started = TokioInstant::now();
                let resp = self
                    .client
                    .post(self.webhook_url.clone())
                    .json(&payload)
                    .send()
                    .await?;
                let duration_ms = started.elapsed().as_millis() as u64;

                let status = resp.status();
                let headers = resp.headers().clone();
                let remaining = parse_header_i64(&headers, "x-ratelimit-remaining");
                let reset_after = parse_header_f64(&headers, "x-ratelimit-reset-after");
                let header_retry = parse_header_f64(&headers, "retry-after");
                let header_scope = headers
                    .get("x-ratelimit-scope")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);

                if let Some(r) = remaining {
                    state.remaining = r;
                    if r <= 0 {
                        if let Some(secs) = reset_after {
                            state.gate =
                                Some(Instant::now() + Duration::from_secs_f64(secs.max(0.0)));
                        }
                    }
                }

                tracing::info!(
                    target: "discord_metrics",
                    status = status.as_u16(),
                    attempt,
                    duration_ms,
                    "send_completed"
                );

                if status.is_success() {
                    return Ok(());
                }

                if status == StatusCode::TOO_MANY_REQUESTS {
                    let body: Value = resp.json().await.unwrap_or_default();
                    let body_retry = body.get("retry_after").and_then(|v| v.as_f64());
                    let body_global = body
                        .get("global")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let is_global = header_scope.as_deref() == Some("global") || body_global;
                    let wait_secs = body_retry.or(header_retry).unwrap_or(1.0).max(0.0);

                    state.gate = Some(Instant::now() + Duration::from_secs_f64(wait_secs));

                    tracing::warn!(
                        target: "discord_metrics",
                        attempt,
                        wait_secs,
                        is_global,
                        scope = ?header_scope,
                        "ratelimited"
                    );

                    if attempt == MAX_ATTEMPTS {
                        return Err(anyhow::anyhow!(
                            "discord 429 after {MAX_ATTEMPTS} attempts (global={is_global})"
                        ));
                    }
                    continue;
                }

                if status.is_server_error() {
                    let backoff_ms = (BACKOFF_BASE_MS << (attempt - 1)).min(BACKOFF_CAP_MS);
                    state.gate = Some(Instant::now() + Duration::from_millis(backoff_ms));

                    tracing::warn!(
                        target: "discord_metrics",
                        status = status.as_u16(),
                        attempt,
                        backoff_ms,
                        "server_error_retry"
                    );

                    if attempt == MAX_ATTEMPTS {
                        let body = resp.text().await.unwrap_or_default();
                        return Err(anyhow::anyhow!(
                            "discord {status} after {MAX_ATTEMPTS} attempts: {body}"
                        ));
                    }
                    continue;
                }

                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!(
                    "discord webhook failed: status={status} body={body}"
                ));
            }
            unreachable!("retry loop bounded by MAX_ATTEMPTS")
        }
    }
}

async fn wait_for_gate(state: &mut BucketState) {
    let gate = state.gate.take();
    if let Some(t) = gate {
        let now = Instant::now();
        if t > now {
            let wait = t - now;
            let wait_ms = wait.as_millis() as u64;
            tracing::info!(
                target: "discord_metrics",
                wait_ms,
                "gate_wait"
            );
            tokio::time::sleep(wait).await;
        }
    }
}

fn parse_header_i64(headers: &HeaderMap, name: &str) -> Option<i64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}

fn parse_header_f64(headers: &HeaderMap, name: &str) -> Option<f64> {
    headers.get(name)?.to_str().ok()?.parse().ok()
}
