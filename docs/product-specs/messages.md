# Discord embed templates

This document is the **contract**. The Rust embed builders in `src/domains/notify/embed.rs` must produce JSON that matches these shapes byte-for-byte. Format strings, color constants, and URL patterns are lifted verbatim from `livepeer-backend-rs/src/tasks/ticket_digest.rs` and `payout_summary.rs`.

## Common envelope

Every Discord payload posted by the bot uses the same envelope:

```json
{
  "username": "Payout Alert Bot",
  "avatar_url": "https://cdn.discordapp.com/avatars/808142296959680532/338766470b721d9081680c7cb34921df.webp?size=80",
  "embeds": [ { ... } ]
}
```

## 1. Single-ticket embed

Posted when a digest window contains exactly one winning ticket for an orchestrator. Source: `ticket_digest.rs:104-202`.

| Field | Value |
|---|---|
| `color` | `16766720` (decimal) if AI, `60296` (decimal) if transcoding |
| `title` | `"Orchestrator Payout"` |
| `url` | `"https://arbiscan.io/tx/{tx_hash}"` |
| `timestamp` | event `block_timestamp` as RFC 3339 |
| `thumbnail.url` | orchestrator `avatar_url` if present, omitted otherwise |
| `description` | multi-line, see below |

### Description format string

```
[**{orch_name}**](https://tools.livepeer.cloud/orchestrator/{orch_addr}) just earned **{face_value:.4} ETH ${face_value_usd:.2}**
{job_type_sentence}

Paid By [**{broadcaster_name}**](https://tools.livepeer.cloud/broadcaster/{broadcaster_eth})
ETH Price **${eth_price:.2}**
Fee cut: **{fee_cut_percent:.2}%**
Commission: **{orch_commission:.4} ETH (${orch_commission_usd:.2})**

24H Rolling Total
**{total_eth:.4} ETH (${total_usd:.2})**
Keeping {total_orch_commission:.5} ETH (${total_orch_commission_usd:.2})
```

- `{job_type_sentence}` is `"performing AI inference."` or `"transcoding video streams."`
- `{fee_cut_percent}` is `fee_cut * 100.0` (fee_cut comes from the API as a percent already; divide by 100 when fetched, multiply here for display)
- The 24h rolling totals come from the local SQLite `events` table aggregated over the prior 24 hours for that orchestrator address

## 2. Multi-ticket digest embed

Posted when a digest window contains 2+ winning tickets for an orchestrator. Tickets are split by job type — one embed for AI, one for transcoding. Source: `ticket_digest.rs:204-355`.

| Field | Value |
|---|---|
| `color` | `0xFFA500` (orange) if AI, `0xFFD700` (gold) if transcoding |
| `title` | `"Orchestrator Payout"` |
| `url` | `"https://arbiscan.io/address/{orch_addr}?mtd=0xec8b3cb6~Redeem%20Winning%20Ticket"` |
| `timestamp` | end of digest window, RFC 3339 |
| `thumbnail.url` | orchestrator `avatar_url` if present, omitted otherwise |
| `description` | see below |

### Description format string

```
[**{orch_name}**](https://tools.livepeer.cloud/orchestrator/{orch_addr}) just earned **{sum_face_eth:.4} ETH ${sum_face_usd:.2}**
{job_type_sentence}

Paid By:
{gateway_lines}

ETH Price **${eth_price:.2}**
Fee cut: **{avg_fee_cut:.2}%**
Commission: **{sum_keep_eth:.4} ETH (${sum_keep_usd:.2})**

24H Rolling Total
**{total_eth:.4} ETH (${total_usd:.2})**
Keeping {total_orch_commission:.5} ETH (${total_orch_commission_usd:.2})
```

- `{gateway_lines}` is the top 3 gateways by total ETH paid, one per line:
  ```
  • [{name}](https://tools.livepeer.cloud/broadcaster/{eth}) — {count} Tickets for {total_eth:.4} ETH
  ```
- `{eth_price}` is taken from the first ticket in the batch (all tickets in a 15-min window have effectively the same price)
- `{avg_fee_cut}` is `(sum(fee_cut) / ticket_count) * 100.0`

> ⚠️ Note: the single-ticket embed and the multi-ticket digest use **different color palettes**. This is intentional and preserved from backend-rs. Single-ticket: `16766720` / `60296`. Digest: `0xFFA500` / `0xFFD700`.

## 3. Periodic summary embed (daily / weekly / monthly)

Posted at period boundaries by the summary poster. Source: `payout_summary.rs:102-162`.

| Field | Value |
|---|---|
| `color` | `"60296"` (sent as string, not int — preserved from backend-rs exactly) |
| `title` | `"{report_type} Payout Summary  (*{Month Day Year}*)"` — note the two spaces before the parenthesized date |
| `url` | `"https://tools.livepeer.cloud/payout/{report_type_lower}/summary/{YYYY-MM-DD}"` |
| `description` | concatenation of the network code block + per-orch code blocks, see below |

### Description structure

A single network-totals code block (CSS-highlighted), then up to 10 per-orchestrator code blocks (plain).

```
```css
{total_ticket} winning tickets
{total_orchs} orchestrators earned
Transcoding Fees: {total_eth:.4} ETH
Orch Commission {total_orch_commission_eth:.4}
    ```
```

Followed for each top-10 orchestrator by:

```
```
#{rank}: {orch_name_or_address} won {orch_total_ticket} tickets
Total {orch_total_eth:.4} ETH ({orch_total_percent:.2}%)
Commission: {orch_total_commission_eth:.4} ETH ({orch_total_commission_percent:.2}%)
        ```
```

- `report_type` is one of `"Daily"`, `"Weekly"`, `"Monthly"`.
- `report_type_lower` is the lowercased version for the URL.
- The leading whitespace inside the code blocks (`    ` and `        `) is intentional and preserved from backend-rs.
- `{orch_total_percent}` and `{orch_total_commission_percent}` are computed client-side: `100.0 * orch_value / network_total`.

## URL conventions

| Surface | Pattern |
|---|---|
| Single tx | `https://arbiscan.io/tx/{tx_hash}` |
| Orchestrator winning-ticket history | `https://arbiscan.io/address/{addr}?mtd=0xec8b3cb6~Redeem%20Winning%20Ticket` |
| Orchestrator dashboard | `https://tools.livepeer.cloud/orchestrator/{addr}` |
| Broadcaster dashboard | `https://tools.livepeer.cloud/broadcaster/{addr}` |
| Payout summary | `https://tools.livepeer.cloud/payout/{daily\|weekly\|monthly}/summary/{YYYY-MM-DD}` |

## Test expectations

Each builder has a snapshot test in `tests/embeds.rs` that constructs a known input fixture and asserts the produced JSON matches a checked-in `tests/fixtures/{single_ticket,digest_ai,digest_tx,summary_daily}.json`. Any divergence from those fixtures fails CI — embed shape is part of the contract.
