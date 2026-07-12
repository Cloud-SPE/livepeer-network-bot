//! Minimal, zero-dependency Prometheus `/metrics` + `/health` endpoint for the
//! digest bot. The bot otherwise has no HTTP server, so this hand-rolls a tiny
//! tokio TCP listener rather than pulling in axum/hyper.
//!
//! The load-bearing metric — `..._digest_last_posted_timestamp{cadence}` — is
//! read from the `summary_watermarks` table at scrape time, so it is accurate
//! across process/container restarts (an in-process gauge would reset to 0 and
//! trip a false "missed digest" alert). The counters are per-process.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::domains::{explorer::types::Cadence, state::repo::SqliteStateRepo};

const CADENCES: [Cadence; 3] = [Cadence::Daily, Cadence::Weekly, Cadence::Monthly];

fn idx(c: Cadence) -> usize {
    match c {
        Cadence::Daily => 0,
        Cadence::Weekly => 1,
        Cadence::Monthly => 2,
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Per-process digest counters. The authoritative "last posted" value is read
/// from the DB, not held here.
pub struct Metrics {
    posts_total: [AtomicU64; 3],
    deferrals_total: [AtomicU64; 3],
    reward_watch_alerts_total: AtomicU64,
    reward_watch_missed_total: AtomicU64,
    reward_watch_digests_total: AtomicU64,
    started_unix: u64,
}

impl Metrics {
    pub fn new() -> Self {
        Self {
            posts_total: Default::default(),
            deferrals_total: Default::default(),
            reward_watch_alerts_total: Default::default(),
            reward_watch_missed_total: Default::default(),
            reward_watch_digests_total: Default::default(),
            started_unix: now_unix(),
        }
    }

    /// A summary was successfully posted to Discord for `cadence`.
    pub fn record_post(&self, cadence: Cadence) {
        self.posts_total[idx(cadence)].fetch_add(1, Ordering::Relaxed);
    }

    /// A poll evaluated `cadence` but deferred (not ready / not eligible).
    pub fn record_deferral(&self, cadence: Cadence) {
        self.deferrals_total[idx(cadence)].fetch_add(1, Ordering::Relaxed);
    }

    /// A "reward call pending" ladder DM fan-out went out for one orchestrator.
    pub fn record_reward_watch_alert(&self) {
        self.reward_watch_alerts_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// A "missed reward" DM fan-out went out for one orchestrator.
    pub fn record_reward_watch_missed(&self) {
        self.reward_watch_missed_total
            .fetch_add(1, Ordering::Relaxed);
    }

    /// The public delinquency digest was posted for a round.
    pub fn record_reward_watch_digest(&self) {
        self.reward_watch_digests_total
            .fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Render the Prometheus text exposition. Reads last-posted timestamps from the
/// DB (restart-proof); everything else from the in-process counters.
async fn render(metrics: &Metrics, state: &SqliteStateRepo) -> String {
    let mut out = String::new();

    out.push_str("# HELP livepeer_bot_start_timestamp Unix time the bot process started\n");
    out.push_str("# TYPE livepeer_bot_start_timestamp gauge\n");
    out.push_str(&format!(
        "livepeer_bot_start_timestamp {}\n",
        metrics.started_unix
    ));

    out.push_str("# HELP livepeer_bot_digest_last_posted_timestamp Unix time of the most recent posted digest per cadence (from DB)\n");
    out.push_str("# TYPE livepeer_bot_digest_last_posted_timestamp gauge\n");
    for c in CADENCES {
        let ts = state
            .last_summary_posted_unix(c.as_path())
            .await
            .unwrap_or(None)
            .unwrap_or(0);
        out.push_str(&format!(
            "livepeer_bot_digest_last_posted_timestamp{{cadence=\"{}\"}} {}\n",
            c.as_path(),
            ts
        ));
    }

    out.push_str("# HELP livepeer_bot_digest_posts_total Digests posted since process start\n");
    out.push_str("# TYPE livepeer_bot_digest_posts_total counter\n");
    for c in CADENCES {
        out.push_str(&format!(
            "livepeer_bot_digest_posts_total{{cadence=\"{}\"}} {}\n",
            c.as_path(),
            metrics.posts_total[idx(c)].load(Ordering::Relaxed)
        ));
    }

    out.push_str("# HELP livepeer_bot_digest_deferrals_total Digest polls that deferred since process start\n");
    out.push_str("# TYPE livepeer_bot_digest_deferrals_total counter\n");
    for c in CADENCES {
        out.push_str(&format!(
            "livepeer_bot_digest_deferrals_total{{cadence=\"{}\"}} {}\n",
            c.as_path(),
            metrics.deferrals_total[idx(c)].load(Ordering::Relaxed)
        ));
    }

    out.push_str("# HELP livepeer_bot_reward_watch_alerts_total Reward-call pending DM fan-outs since process start\n");
    out.push_str("# TYPE livepeer_bot_reward_watch_alerts_total counter\n");
    out.push_str(&format!(
        "livepeer_bot_reward_watch_alerts_total {}\n",
        metrics.reward_watch_alerts_total.load(Ordering::Relaxed)
    ));

    out.push_str("# HELP livepeer_bot_reward_watch_missed_total Missed-reward DM fan-outs since process start\n");
    out.push_str("# TYPE livepeer_bot_reward_watch_missed_total counter\n");
    out.push_str(&format!(
        "livepeer_bot_reward_watch_missed_total {}\n",
        metrics.reward_watch_missed_total.load(Ordering::Relaxed)
    ));

    out.push_str("# HELP livepeer_bot_reward_watch_digests_total Delinquency digests posted since process start\n");
    out.push_str("# TYPE livepeer_bot_reward_watch_digests_total counter\n");
    out.push_str(&format!(
        "livepeer_bot_reward_watch_digests_total {}\n",
        metrics.reward_watch_digests_total.load(Ordering::Relaxed)
    ));

    out
}

/// Serve `/metrics` and `/health` on `bind` (e.g. `0.0.0.0:9300`). Runs forever;
/// a bind failure logs and returns (the caller must NOT treat that as fatal).
pub async fn serve(bind: String, metrics: Arc<Metrics>, state: Arc<SqliteStateRepo>) {
    let listener = match TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(bind = %bind, error = %e, "metrics server failed to bind; disabled");
            return;
        }
    };
    tracing::info!(bind = %bind, "metrics/health server listening");

    loop {
        let (mut sock, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => continue,
        };
        let metrics = metrics.clone();
        let state = state.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let n = sock.read(&mut buf).await.unwrap_or(0);
            let first_line = String::from_utf8_lossy(&buf[..n]);
            let path = first_line
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");

            let body = if path.starts_with("/health") {
                "ok".to_string()
            } else {
                render(&metrics, &state).await
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = sock.write_all(resp.as_bytes()).await;
        });
    }
}
