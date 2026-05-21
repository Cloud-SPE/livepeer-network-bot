use std::{collections::HashMap, sync::Arc, time::Duration};

use chrono::{DateTime, TimeDelta, Timelike, Utc};
use serde_json::Value;

use crate::domains::{
    explorer::{
        client::ExplorerClient,
        types::{
            preferred_valuation, GatewayProfileRow, GatewayProfileRowExt, OrchestratorProfileRow,
        },
    },
    notify::{
        embed::{build_digest, build_single_ticket, TicketView},
        service::Notifier,
    },
    state::repo::{SqliteStateRepo, StoredEvent},
};

pub async fn run<N: Notifier>(
    explorer: Arc<ExplorerClient>,
    notifier: Arc<N>,
    state: Arc<SqliteStateRepo>,
    window: Duration,
    fetch_limit: u32,
) {
    loop {
        sleep_until_next_boundary(window).await;
        if let Err(err) = run_once(&explorer, notifier.as_ref(), &state, fetch_limit).await {
            tracing::error!(?err, "digest poster iteration failed");
        }
    }
}

async fn run_once<N: Notifier>(
    explorer: &ExplorerClient,
    notifier: &N,
    state: &SqliteStateRepo,
    fetch_limit: u32,
) -> anyhow::Result<()> {
    let events = state.fetch_unsent(i64::from(fetch_limit)).await?;
    if events.is_empty() {
        return Ok(());
    }

    let mut by_orch: HashMap<String, Vec<StoredEvent>> = HashMap::new();
    for mut ev in events {
        if valuation_missing(&ev) {
            refresh_event_valuation(explorer, state, &mut ev).await?;
        }
        let Some(addr) = ev.to_address.clone() else {
            continue;
        };
        by_orch.entry(addr).or_default().push(ev);
    }

    let mut jobs = Vec::new();

    for (orch_addr, mut tickets) in by_orch {
        tickets.sort_by_key(|t| t.block_timestamp);

        let orch = explorer.get_orchestrator(&orch_addr).await?;
        let fee_cut = parse_fee_cut(&orch);

        let mut gateways: HashMap<String, GatewayProfileRow> = HashMap::new();
        for t in &tickets {
            if let Some(addr) = t.from_address.as_deref() {
                if !gateways.contains_key(addr) {
                    let gw = explorer.get_gateway(addr).await?;
                    gateways.insert(addr.to_string(), gw);
                }
            }
        }

        // Anchor the 24h rolling total at the digest's latest ticket, not
        // at wall-clock `now()`. This keeps the rolling number meaningful
        // when the poster is draining a backfill — otherwise a ticket from
        // last week would always show a 0 rolling total.
        let anchor = tickets
            .last()
            .map(|t| t.block_timestamp)
            .expect("by_orch group is non-empty");
        let totals = state
            .orch_totals_window(
                &orch_addr,
                anchor - chrono::Duration::hours(24),
                anchor,
                fee_cut,
            )
            .await?;

        let views: Vec<TicketView> = tickets
            .iter()
            .filter_map(|t| {
                let addr = t.from_address.as_deref()?;
                let gw = gateways.get(addr).cloned()?;
                Some(TicketView {
                    event: t.clone(),
                    gateway: gw,
                })
            })
            .collect();

        if views.len() == 1 {
            jobs.push(PendingMessage {
                orch_addr: orch_addr.clone(),
                latest_ts: views[0].event.block_timestamp,
                is_ai: views[0].gateway.is_ai(),
                sent_ids: vec![views[0].event.id.clone()],
                payload: build_single_ticket(&orch, &views[0], fee_cut, &totals),
            });
        } else {
            let (ai, tx): (Vec<_>, Vec<_>) = views.into_iter().partition(|v| v.gateway.is_ai());
            for (group, is_ai) in [(&ai, true), (&tx, false)] {
                if group.is_empty() {
                    continue;
                }
                jobs.push(PendingMessage {
                    orch_addr: orch_addr.clone(),
                    latest_ts: latest_group_ts(group),
                    is_ai,
                    sent_ids: group.iter().map(|v| v.event.id.clone()).collect(),
                    payload: build_digest(&orch, &orch_addr, is_ai, group, fee_cut, &totals),
                });
            }
        }
    }

    jobs.sort_by(|a, b| {
        a.latest_ts
            .cmp(&b.latest_ts)
            .then_with(|| a.orch_addr.cmp(&b.orch_addr))
            .then_with(|| a.is_ai.cmp(&b.is_ai))
    });

    for job in jobs {
        match notifier.send(job.payload).await {
            Ok(()) => state.mark_sent(&job.sent_ids).await?,
            Err(err) => {
                tracing::error!(
                    ?err,
                    orch = %job.orch_addr,
                    ai = job.is_ai,
                    latest_ts = %job.latest_ts,
                    "digest post failed"
                );
            }
        }
    }

    Ok(())
}

fn parse_fee_cut(orch: &OrchestratorProfileRow) -> f64 {
    orch.fee_cut_percent
        .parse::<f64>()
        .map(|p| p / 100.0)
        .unwrap_or(0.0)
}

fn valuation_missing(event: &StoredEvent) -> bool {
    let usd = event
        .amount_usd
        .as_deref()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    let price = event
        .native_usd_price
        .as_deref()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0);
    usd <= 0.0 || price <= 0.0
}

async fn refresh_event_valuation(
    explorer: &ExplorerClient,
    state: &SqliteStateRepo,
    event: &mut StoredEvent,
) -> anyhow::Result<()> {
    let Some(fresh) = explorer
        .get_winning_ticket_by_tx_hash(&event.tx_hash)
        .await?
    else {
        return Ok(());
    };
    let valuation = fresh
        .valuations
        .as_ref()
        .and_then(|vals| preferred_valuation(vals));
    let amount_usd = valuation.and_then(|v| v.amount_usd.clone());
    let native_usd_price = valuation.and_then(|v| v.native_usd_price.clone());
    let updated = state
        .repair_pending_event_valuation(&event.id, amount_usd.clone(), native_usd_price.clone())
        .await?;
    if updated {
        if has_positive_number(amount_usd.as_deref()) {
            event.amount_usd = amount_usd;
        }
        if has_positive_number(native_usd_price.as_deref()) {
            event.native_usd_price = native_usd_price;
        }
    }
    Ok(())
}

fn has_positive_number(raw: Option<&str>) -> bool {
    raw.and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0) > 0.0
}

fn latest_group_ts(group: &[TicketView]) -> DateTime<Utc> {
    group
        .iter()
        .map(|v| v.event.block_timestamp)
        .max()
        .expect("non-empty digest group")
}

struct PendingMessage {
    orch_addr: String,
    latest_ts: DateTime<Utc>,
    is_ai: bool,
    sent_ids: Vec<String>,
    payload: Value,
}

async fn sleep_until_next_boundary(window: Duration) {
    let now = Utc::now();
    let next = next_boundary(now, window);
    let wait = (next - now)
        .to_std()
        .unwrap_or_else(|_| Duration::from_secs(0));
    tokio::time::sleep(wait).await;
}

fn next_boundary(now: DateTime<Utc>, window: Duration) -> DateTime<Utc> {
    let window_secs = window.as_secs();
    if window_secs == 0 {
        return now;
    }

    let day_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("midnight is always valid")
        .and_utc();
    let since_midnight = u64::from(now.num_seconds_from_midnight());
    let remainder = since_midnight % window_secs;
    let delta_secs = if remainder == 0 {
        window_secs
    } else {
        window_secs - remainder
    };

    day_start + TimeDelta::seconds((since_midnight + delta_secs) as i64)
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
        time::Duration,
    };

    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use reqwest::Client;
    use serde_json::{json, Value};
    use sqlx::sqlite::SqlitePoolOptions;
    use url::Url;

    use super::{next_boundary, run_once};
    use crate::domains::{
        explorer::client::ExplorerClient, notify::service::Notifier, state::repo::SqliteStateRepo,
    };

    struct RecordingNotifier {
        payloads: Arc<Mutex<Vec<Value>>>,
    }

    impl RecordingNotifier {
        fn new() -> Self {
            Self {
                payloads: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn payloads(&self) -> Vec<Value> {
            self.payloads.lock().unwrap().clone()
        }
    }

    impl Notifier for RecordingNotifier {
        fn send(
            &self,
            payload: Value,
        ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
            let payloads = self.payloads.clone();
            async move {
                payloads.lock().unwrap().push(payload);
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn retries_old_unsent_ticket_outside_digest_window() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let state = SqliteStateRepo::new(pool.clone());

        let old_ts = Utc::now() - ChronoDuration::hours(2);
        sqlx::query(
            r#"
            INSERT INTO events (
                id, chain_id, tx_hash, log_index, block_number, block_timestamp,
                contract_address, contract_name, event_name, event_signature,
                asset, amount_native, amount_usd, native_usd_price,
                from_address, to_address, finality, is_canonical, fetched_at
            ) VALUES (
                ?, ?, ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?,
                ?, ?, ?, ?, ?
            )
            "#,
        )
        .bind("ticket-1")
        .bind("42161")
        .bind("0xtx1")
        .bind(0_i64)
        .bind("1")
        .bind(old_ts)
        .bind("0xcontract")
        .bind("TicketBroker")
        .bind("WinningTicketRedeemed")
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(Some("0.5"))
        .bind(Option::<String>::None)
        .bind(Option::<String>::None)
        .bind(Some("0xgateway"))
        .bind(Some("0xorch"))
        .bind("finalized")
        .bind(1_i64)
        .bind(old_ts)
        .execute(&pool)
        .await
        .unwrap();

        let server = TestExplorerServer::start();
        let explorer = ExplorerClient::new(Client::new(), server.base_url());
        let notifier = RecordingNotifier::new();

        run_once(&explorer, &notifier, &state, 500).await.unwrap();

        let payloads = notifier.payloads();
        assert_eq!(payloads.len(), 1);
        assert_eq!(
            payloads[0]["embeds"][0]["timestamp"],
            json!(old_ts.to_rfc3339())
        );
        let description = payloads[0]["embeds"][0]["description"].as_str().unwrap();
        assert!(description.contains("**0.5000 ETH $1500.00**"));
        assert!(description.contains("ETH Price **$3000.00**"));

        let remaining: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM events WHERE event_name = 'WinningTicketRedeemed' AND sent_to_discord = 0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(remaining, 0);

        let repaired: (Option<String>, Option<String>) =
            sqlx::query_as("SELECT amount_usd, native_usd_price FROM events WHERE id = 'ticket-1'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(repaired.0.as_deref(), Some("1500.0"));
        assert_eq!(repaired.1.as_deref(), Some("3000.0"));
    }

    #[tokio::test]
    async fn posts_messages_oldest_effective_timestamp_first() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let state = SqliteStateRepo::new(pool.clone());

        for (id, tx, ts, from, to, usd, price) in [
            (
                "e1",
                "0xtx1",
                ts(2026, 5, 15, 10, 0, 0),
                "0xgw-tx",
                "0xorch-a",
                "300.0",
                "3000.0",
            ),
            (
                "e2",
                "0xtx2",
                ts(2026, 5, 15, 10, 5, 0),
                "0xgw-ai",
                "0xorch-b",
                "200.0",
                "2000.0",
            ),
            (
                "e3",
                "0xtx3",
                ts(2026, 5, 15, 10, 6, 0),
                "0xgw-tx",
                "0xorch-b",
                "210.0",
                "2100.0",
            ),
            (
                "e4",
                "0xtx4",
                ts(2026, 5, 15, 10, 7, 0),
                "0xgw-ai",
                "0xorch-b",
                "220.0",
                "2200.0",
            ),
        ] {
            sqlx::query(
                r#"
                INSERT INTO events (
                    id, chain_id, tx_hash, log_index, block_number, block_timestamp,
                    contract_address, contract_name, event_name, event_signature,
                    asset, amount_native, amount_usd, native_usd_price,
                    from_address, to_address, finality, is_canonical, fetched_at
                ) VALUES (
                    ?, ?, ?, ?, ?, ?,
                    ?, ?, ?, ?,
                    ?, ?, ?, ?,
                    ?, ?, ?, ?, ?
                )
                "#,
            )
            .bind(id)
            .bind("42161")
            .bind(tx)
            .bind(0_i64)
            .bind("1")
            .bind(ts)
            .bind("0xcontract")
            .bind("TicketBroker")
            .bind("WinningTicketRedeemed")
            .bind("0xsig")
            .bind("ETH")
            .bind("0.1")
            .bind(Some(usd))
            .bind(Some(price))
            .bind(Some(from))
            .bind(Some(to))
            .bind("finalized")
            .bind(1_i64)
            .bind(ts)
            .execute(&pool)
            .await
            .unwrap();
        }

        let server = TestExplorerServer::start();
        let explorer = ExplorerClient::new(Client::new(), server.base_url());
        let notifier = RecordingNotifier::new();

        run_once(&explorer, &notifier, &state, 500).await.unwrap();

        let payloads = notifier.payloads();
        let timestamps: Vec<_> = payloads
            .iter()
            .map(|p| p["embeds"][0]["timestamp"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            timestamps,
            vec![
                "2026-05-15T10:00:00+00:00".to_string(),
                "2026-05-15T10:06:00+00:00".to_string(),
                "2026-05-15T10:07:00+00:00".to_string(),
            ]
        );
    }

    #[test]
    fn aligns_to_next_wall_clock_boundary() {
        let now = Utc.with_ymd_and_hms(2026, 5, 15, 14, 7, 12).unwrap();
        let next = next_boundary(now, Duration::from_secs(15 * 60));
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 5, 15, 14, 15, 0).unwrap());

        let on_boundary = Utc.with_ymd_and_hms(2026, 5, 15, 14, 15, 0).unwrap();
        let next = next_boundary(on_boundary, Duration::from_secs(15 * 60));
        assert_eq!(next, Utc.with_ymd_and_hms(2026, 5, 15, 14, 30, 0).unwrap());
    }

    struct TestExplorerServer {
        addr: String,
        handle: thread::JoinHandle<()>,
    }

    impl TestExplorerServer {
        fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let addr = listener.local_addr().unwrap();

            let orch_body = json!({
                "address": "0xorch",
                "total_stake": "0",
                "fee_cut_percent": "30",
                "fee_share_percent": "0",
                "reward_cut_percent": "0",
                "is_active": true,
                "as_of_block": "0",
                "as_of_round": null,
                "display_name": "TestOrch",
                "avatar_url": null,
                "service_uri": null,
                "last_lifecycle_event_at": null
            })
            .to_string();
            let orch_a_body = json!({
                "address": "0xorch-a",
                "total_stake": "0",
                "fee_cut_percent": "30",
                "fee_share_percent": "0",
                "reward_cut_percent": "0",
                "is_active": true,
                "as_of_block": "0",
                "as_of_round": null,
                "display_name": "OrchA",
                "avatar_url": null,
                "service_uri": null,
                "last_lifecycle_event_at": null
            })
            .to_string();
            let orch_b_body = json!({
                "address": "0xorch-b",
                "total_stake": "0",
                "fee_cut_percent": "30",
                "fee_share_percent": "0",
                "reward_cut_percent": "0",
                "is_active": true,
                "as_of_block": "0",
                "as_of_round": null,
                "display_name": "OrchB",
                "avatar_url": null,
                "service_uri": null,
                "last_lifecycle_event_at": null
            })
            .to_string();
            let gw_body = json!({
                "address": "0xgateway",
                "display_name": "TestGateway",
                "avatar_url": null,
                "kind": "transcoding",
                "latest_deposit": "0",
                "latest_reserve": "0",
                "reserve_claimed_in_current_round": "0",
                "withdraw_round": "0",
                "unlock_in_progress": false,
                "as_of_block": "0"
            })
            .to_string();
            let gw_ai_body = json!({
                "address": "0xgw-ai",
                "display_name": "AiGateway",
                "avatar_url": null,
                "kind": "ai",
                "latest_deposit": "0",
                "latest_reserve": "0",
                "reserve_claimed_in_current_round": "0",
                "withdraw_round": "0",
                "unlock_in_progress": false,
                "as_of_block": "0"
            })
            .to_string();
            let gw_tx_body = json!({
                "address": "0xgw-tx",
                "display_name": "TxGateway",
                "avatar_url": null,
                "kind": "transcoding",
                "latest_deposit": "0",
                "latest_reserve": "0",
                "reserve_claimed_in_current_round": "0",
                "withdraw_round": "0",
                "unlock_in_progress": false,
                "as_of_block": "0"
            })
            .to_string();

            let handle = thread::spawn(move || {
                let mut idle_spins = 0u32;
                loop {
                    let Ok((mut stream, _)) = listener.accept() else {
                        if idle_spins >= 50 {
                            break;
                        }
                        idle_spins += 1;
                        thread::sleep(Duration::from_millis(10));
                        continue;
                    };
                    idle_spins = 0;
                    let mut buf = [0_u8; 4096];
                    let n = stream.read(&mut buf).unwrap();
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let body = if request.starts_with(
                        "GET /api/v1/events?event_name=WinningTicketRedeemed&tx_hash=0xtx1&with_valuations=true&limit=10 ",
                    ) {
                        r#"{"data":[{"id":"ticket-1","chain_id":"42161","tx_hash":"0xtx1","log_index":0,"block_number":"1","block_hash":"0xblock","block_timestamp":"2026-05-15T12:30:00Z","contract_address":"0xcontract","contract_name":"TicketBroker","event_name":"WinningTicketRedeemed","event_signature":"0xsig","asset":"ETH","amount_native":"0.5","is_valuable":true,"from_address":"0xgateway","to_address":"0xorch","finality":"finalized","is_canonical":true,"valuations":[{"asset":"ETH","valuation_version":"v1","amount_native":"0.5","native_usd_price":"3000.0","amount_usd":"1500.0","source":"test","pricing_method":"test","status":"priced"}]}],"next_cursor":null}"#
                    } else if request.starts_with("GET /api/v1/orchestrators/0xorch-a ") {
                        &orch_a_body
                    } else if request.starts_with("GET /api/v1/orchestrators/0xorch-b ") {
                        &orch_b_body
                    } else if request.starts_with("GET /api/v1/orchestrators/0xorch ") {
                        &orch_body
                    } else if request.starts_with("GET /api/v1/gateways/0xgw-ai/profile ") {
                        &gw_ai_body
                    } else if request.starts_with("GET /api/v1/gateways/0xgw-tx/profile ") {
                        &gw_tx_body
                    } else if request.starts_with("GET /api/v1/gateways/0xgateway/profile ") {
                        &gw_body
                    } else {
                        panic!("unexpected request: {request}");
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    stream.flush().unwrap();
                }
            });

            Self {
                addr: format!("http://{addr}/"),
                handle,
            }
        }

        fn base_url(&self) -> Url {
            Url::parse(&self.addr).unwrap()
        }
    }

    fn ts(yy: i32, mm: u32, dd: u32, h: u32, m: u32, s: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(yy, mm, dd, h, m, s).unwrap()
    }

    impl Drop for TestExplorerServer {
        fn drop(&mut self) {
            let handle = std::mem::replace(&mut self.handle, thread::spawn(|| {}));
            handle.join().unwrap();
        }
    }
}
