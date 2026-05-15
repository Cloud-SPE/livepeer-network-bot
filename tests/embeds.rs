//! Embed builder snapshot tests.
//!
//! Locks the byte-for-byte JSON shape of every embed the bot ships. The
//! format strings are the contract (docs/product-specs/messages.md) —
//! intentional changes require touching BOTH the builder and the expected
//! literal in this file.
//!
//! Webhook embeds (build_single_ticket, build_digest, build_summary) return
//! `serde_json::Value` directly. DM embeds (build_reward_event_dm,
//! build_delegator_digest_dm) return `serenity::all::CreateMessage` and are
//! compared via `serde_json::to_value`.

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use livepeer_payout_bot::domains::{
    explorer::types::{
        Cadence, GatewayProfileRow, OrchestratorProfileRow, PayoutLeaderboardRow,
        PayoutSummaryResponse,
    },
    notify::{
        dm::{build_delegator_digest_dm, build_reward_event_dm, DelegatorDigest},
        embed::{build_digest, build_single_ticket, build_summary, TicketView},
    },
    state::{
        event_streams::{DelegatorEventRow, RewardEventRow},
        repo::{OrchTotals, StoredEvent},
    },
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

// ----- fixture helpers -----------------------------------------------------

fn ts(yy: i32, mm: u32, dd: u32, h: u32, m: u32, s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(yy, mm, dd, h, m, s).unwrap()
}

fn orch_fixture(
    name: &str,
    addr: &str,
    fee_cut_percent: &str,
    avatar: Option<&str>,
) -> OrchestratorProfileRow {
    OrchestratorProfileRow {
        address: addr.to_string(),
        total_stake: None,
        fee_cut_percent: Some(fee_cut_percent.to_string()),
        fee_share_percent: None,
        reward_cut_percent: None,
        is_active: Some(true),
        as_of_block: None,
        as_of_round: None,
        display_name: Some(name.to_string()),
        avatar_url: avatar.map(String::from),
        service_uri: None,
        last_lifecycle_event_at: None,
    }
}

fn gateway_fixture(addr: &str, name: &str, kind: &str) -> GatewayProfileRow {
    GatewayProfileRow {
        address: addr.to_string(),
        display_name: Some(name.to_string()),
        avatar_url: None,
        kind: Some(kind.to_string()),
        latest_deposit: None,
        latest_reserve: None,
        reserve_claimed_in_current_round: None,
        withdraw_round: None,
        unlock_in_progress: None,
        as_of_block: None,
    }
}

fn stored_event_fixture(
    id: &str,
    tx: &str,
    from: &str,
    to: &str,
    eth: &str,
    usd: &str,
    price: &str,
) -> StoredEvent {
    StoredEvent {
        id: id.into(),
        tx_hash: tx.into(),
        block_timestamp: ts(2026, 5, 15, 12, 30, 0),
        from_address: Some(from.into()),
        to_address: Some(to.into()),
        amount_native: Some(eth.into()),
        amount_usd: Some(usd.into()),
        native_usd_price: Some(price.into()),
    }
}

// ----- build_single_ticket -------------------------------------------------

#[test]
fn single_ticket_embed_shape() {
    let event = stored_event_fixture(
        "311468",
        "0xtx0001",
        "0xbcst0001",
        "0xorch0001",
        "0.0824",
        "262.65",
        "3185.81",
    );
    let gateway = gateway_fixture("0xbcst0001", "MyBroadcaster", "transcoding");
    let view = TicketView { event, gateway };
    let orch = orch_fixture("MyOrch", "0xorch0001", "30", Some("https://avatar"));
    let totals = OrchTotals {
        face_value_eth: 1.234,
        face_value_usd: 567.89,
        commission_eth: 0.3702,
        commission_usd: 170.37,
    };

    let actual = build_single_ticket(&orch, &view, 0.30, &totals);

    let expected = json!({
        "username": "Payout Alert Bot",
        "avatar_url": "https://cdn.discordapp.com/avatars/808142296959680532/338766470b721d9081680c7cb34921df.webp?size=80",
        "embeds": [{
            "color": 60296,
            "title": "Orchestrator Payout",
            "description": "[**MyOrch**](https://tools.livepeer.cloud/orchestrator/0xorch0001) just earned **0.0824 ETH $262.65**\ntranscoding video streams.\n\nPaid By [**MyBroadcaster**](https://tools.livepeer.cloud/broadcaster/0xbcst0001)\nETH Price **$3185.81**\nFee cut: **30.00%**\nCommission: **0.0247 ETH ($78.79)**\n\n24H Rolling Total\n**1.2340 ETH ($567.89)**\nKeeping 0.37020 ETH ($170.37)",
            "timestamp": "2026-05-15T12:30:00+00:00",
            "url": "https://arbiscan.io/tx/0xtx0001",
            "thumbnail": {"url": "https://avatar"},
        }]
    });

    assert_eq!(actual, expected);
}

// ----- build_digest (multi-ticket, AI grouped) -----------------------------

#[test]
fn digest_embed_shape() {
    let orch_addr = "0xorch0001";
    let orch = orch_fixture("MyOrch", orch_addr, "30", None);

    let t1 = TicketView {
        event: stored_event_fixture(
            "1", "0xtxA", "0xbcstA", orch_addr, "0.1000", "318.58", "3185.81",
        ),
        gateway: gateway_fixture("0xbcstA", "AiGateway1", "ai"),
    };
    let t2 = TicketView {
        event: stored_event_fixture(
            "2", "0xtxB", "0xbcstA", orch_addr, "0.0500", "159.29", "3185.81",
        ),
        gateway: gateway_fixture("0xbcstA", "AiGateway1", "ai"),
    };

    let totals = OrchTotals {
        face_value_eth: 2.0,
        face_value_usd: 6372.0,
        commission_eth: 0.6,
        commission_usd: 1911.6,
    };

    let actual = build_digest(
        &orch,
        orch_addr,
        true,
        &[t1, t2],
        0.30,
        ts(2026, 5, 15, 13, 0, 0),
        &totals,
    );

    let expected = json!({
        "username": "Payout Alert Bot",
        "avatar_url": "https://cdn.discordapp.com/avatars/808142296959680532/338766470b721d9081680c7cb34921df.webp?size=80",
        "embeds": [{
            "color": 0xFFA500,
            "title": "Orchestrator Payout",
            "description": "[**MyOrch**](https://tools.livepeer.cloud/orchestrator/0xorch0001) just earned **0.1500 ETH $477.87**\nperforming AI inference.\n\nPaid By:\n• [AiGateway1](https://tools.livepeer.cloud/broadcaster/0xbcstA) — 2 Tickets for 0.1500 ETH\n\nETH Price **$3185.81**\nFee cut: **30.00%**\nCommission: **0.0450 ETH ($143.36)**\n\n24H Rolling Total\n**2.0000 ETH ($6372.00)**\nKeeping 0.60000 ETH ($1911.60)",
            "timestamp": "2026-05-15T13:00:00+00:00",
            "url": "https://arbiscan.io/address/0xorch0001?mtd=0xec8b3cb6~Redeem%20Winning%20Ticket",
        }]
    });

    assert_eq!(actual, expected);
}

// ----- build_summary -------------------------------------------------------

#[test]
fn summary_embed_shape() {
    let summary = PayoutSummaryResponse {
        period_start: "2026-05-14".into(),
        period_end: "2026-05-14".into(),
        valuation_version: "v1".into(),
        job_type: "both".into(),
        ticket_count: "322".into(),
        sum_face_value_native: "0.7027".into(),
        sum_face_value_usd: "1600.12".into(),
        sum_commission_native: "0.6456".into(),
        sum_commission_usd: "1470.14".into(),
        sum_delegators_share_native: "0.0571".into(),
        sum_delegators_share_usd: "129.97".into(),
        distinct_gateways: "4".into(),
        usd_rows_priced: "322".into(),
    };
    let leaderboard = vec![
        PayoutLeaderboardRow {
            orchestrator_address: "0xorchA".into(),
            ticket_count: "100".into(),
            sum_face_value_native: "0.3500".into(),
            sum_face_value_usd: "800.00".into(),
            sum_commission_native: "0.3000".into(),
            sum_commission_usd: "750.00".into(),
            sum_delegators_share_native: "0.0500".into(),
            sum_delegators_share_usd: "50.00".into(),
            distinct_gateways: "2".into(),
            usd_rows_priced: "100".into(),
            display_name: Some("OrchAlpha".into()),
            avatar_url: None,
        },
        PayoutLeaderboardRow {
            orchestrator_address: "0xorchB".into(),
            ticket_count: "50".into(),
            sum_face_value_native: "0.2000".into(),
            sum_face_value_usd: "400.00".into(),
            sum_commission_native: "0.1500".into(),
            sum_commission_usd: "350.00".into(),
            sum_delegators_share_native: "0.0500".into(),
            sum_delegators_share_usd: "50.00".into(),
            distinct_gateways: "1".into(),
            usd_rows_priced: "50".into(),
            display_name: None,
            avatar_url: None,
        },
    ];

    let actual = build_summary(
        Cadence::Daily,
        NaiveDate::from_ymd_opt(2026, 5, 14).unwrap(),
        &summary,
        &leaderboard,
    );

    // Network block:
    //   322 winning tickets
    //   2 orchestrators earned (leaderboard.len())
    //   Transcoding Fees: 0.7027 ETH
    //   Orch Commission 0.6456
    // Then per-orch blocks:
    //   #1 OrchAlpha won 100 tickets, 0.3500 ETH (49.81%), 0.3000 ETH (46.46%)
    //   #2 0xorchB won 50 tickets, 0.2000 ETH (28.46%), 0.1500 ETH (23.23%)

    let expected_description = format!(
        "{}{}{}",
        "```css\n322 winning tickets\n2 orchestrators earned\nTranscoding Fees: 0.7027 ETH\nOrch Commission 0.6456\n    ```",
        "```\n#1: OrchAlpha won 100 tickets\nTotal 0.3500 ETH (49.81%)\nCommission: 0.3000 ETH (46.47%)\n        ```",
        "```\n#2: 0xorchB won 50 tickets\nTotal 0.2000 ETH (28.46%)\nCommission: 0.1500 ETH (23.23%)\n        ```",
    );

    let expected = json!({
        "username": "Payout Alert Bot",
        "avatar_url": "https://cdn.discordapp.com/avatars/808142296959680532/338766470b721d9081680c7cb34921df.webp?size=80",
        "embeds": [{
            "color": "60296",
            "title": "Daily Payout Summary  (*May 14 2026*)",
            "description": expected_description,
            "url": "https://tools.livepeer.cloud/payout/daily/summary/2026-05-14",
        }]
    });

    assert_eq!(actual, expected);
}

// ----- build_reward_event_dm ----------------------------------------------

#[test]
fn reward_dm_shape() {
    let event = RewardEventRow {
        id: "9001".into(),
        tx_hash: "0xrewardtx".into(),
        block_timestamp: ts(2026, 5, 15, 9, 0, 0),
        orch_address: "0xorch0001".into(),
        amount_native: Some("12.3456".into()),
        amount_usd: Some("99.99".into()),
        native_usd_price: Some("8.10".into()),
    };
    let orch = orch_fixture("MyOrch", "0xorch0001", "30", Some("https://avatar"));

    let full = serde_json::to_value(build_reward_event_dm(&orch, &event)).unwrap();
    // Compare only the embed payload; serenity::CreateMessage also serializes
    // empty-default envelope fields (attachments, tts, enforce_nonce, …)
    // that aren't part of our contract.
    let actual = &full["embeds"][0];

    let expected = json!({
        "type": "rich",
        "title": "Reward earned",
        "description": "[**MyOrch**](https://tools.livepeer.cloud/orchestrator/0xorch0001) earned **12.3456 LPT** (~$99.99) in inflation rewards.\n\n[View transaction](https://arbiscan.io/tx/0xrewardtx)",
        "color": 0xFFA500,
        "timestamp": "2026-05-15T09:00:00Z",
        "thumbnail": {
            "url": "https://avatar",
            "height": null,
            "proxy_url": null,
            "width": null,
        },
    });

    assert_eq!(actual, &expected);
}

// ----- build_delegator_digest_dm -------------------------------------------

#[test]
fn delegator_digest_dm_shape() {
    let new_bond = DelegatorEventRow {
        id: "n1".into(),
        event_name: "Bond".into(),
        tx_hash: "0xbondtx".into(),
        block_timestamp: ts(2026, 5, 15, 14, 0, 0),
        delegator_address: "0xdelegator0001".into(),
        orch_address: "0xorch0001".into(),
        amount_native: Some("100.0000".into()),
        amount_usd: None,
    };
    let unbond = DelegatorEventRow {
        id: "u1".into(),
        event_name: "Unbond".into(),
        tx_hash: "0xunbondtx".into(),
        block_timestamp: ts(2026, 5, 15, 14, 30, 0),
        delegator_address: "0xdelegator0002".into(),
        orch_address: "0xorch0001".into(),
        amount_native: Some("50.0000".into()),
        amount_usd: None,
    };

    let digest = DelegatorDigest {
        new_bonds: vec![&new_bond],
        stake_change_bonds: vec![],
        unbonds: vec![&unbond],
        rebonds: vec![],
    };

    let orch = orch_fixture("MyOrch", "0xorch0001", "30", None);
    let full = serde_json::to_value(build_delegator_digest_dm(
        &orch,
        "0xorch0001",
        &digest,
        ts(2026, 5, 15, 15, 0, 0),
    ))
    .unwrap();
    let actual = &full["embeds"][0];

    let expected = json!({
        "type": "rich",
        "title": "Delegator activity",
        "description": "[**MyOrch**](https://tools.livepeer.cloud/orchestrator/0xorch0001) had delegator activity:\n\n**New delegators (1)**\n• [`0xdele…0001`](https://arbiscan.io/tx/0xbondtx) bonded **100.0000 LPT**\n\n**Unbonds (1)**\n• [`0xdele…0002`](https://arbiscan.io/tx/0xunbondtx) unbonded **50.0000 LPT**\n",
        "color": 0x46a758,
        "timestamp": "2026-05-15T15:00:00Z",
    });

    assert_eq!(actual, &expected);
}

// Silence the unused-import diagnostic when individual tests are gated by
// cfg(test) features in the future.
#[allow(dead_code)]
fn _ensure_value_in_scope() -> Value {
    Value::Null
}
