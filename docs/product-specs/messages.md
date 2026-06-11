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
- If `amount_usd` / `native_usd_price` are missing or zero for the ticket, the embed falls back to ETH-only formatting:
  - headline becomes `**{face_value:.4} ETH**`
  - `ETH Price …` line is omitted
  - commission line becomes `Commission: **{orch_commission:.4} ETH**`

## 2. Multi-ticket digest embed

Posted when a digest window contains 2+ winning tickets for an orchestrator. Tickets are split by job type — one embed for AI, one for transcoding. Source: `ticket_digest.rs:204-355`.

| Field | Value |
|---|---|
| `color` | `0xFFA500` (orange) if AI, `0xFFD700` (gold) if transcoding |
| `title` | `"Orchestrator Payout"` |
| `url` | `"https://arbiscan.io/address/{orch_addr}?mtd=0xec8b3cb6~Redeem%20Winning%20Ticket"` |
| `timestamp` | newest ticket `block_timestamp` in the batch, RFC 3339 |
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
- `{eth_price}` is derived from sane priced rows in the batch (preferring rows whose direct and derived prices agree; median selection across valid rows)
- `{avg_fee_cut}` is `(sum(fee_cut) / ticket_count) * 100.0`
- If a ticket batch has missing/zero valuation fields, the digest uses only sane priced rows for USD totals and ETH price selection. When no sane batch ETH price exists, the `ETH Price …` line is omitted and the headline / commission fallback to ETH-only formatting.
- Across a digest run, outgoing public-channel payout messages are posted oldest effective timestamp first. Single-ticket messages use the event timestamp; multi-ticket messages use the newest ticket timestamp in that batch.

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

## 4. Reward event DM (subscriber)

Built with `serenity::all::CreateMessage` + `CreateEmbed`. Sent privately to each user subscribed to the orchestrator that triggered the `Reward` event. Source: `src/domains/notify/dm.rs::build_reward_event_dm`.

| Field | Value |
|---|---|
| `title` | `"Reward earned"` |
| `color` | `#ffa500` (orange) |
| `timestamp` | event `block_timestamp` |
| `thumbnail.url` | orchestrator `avatar_url` if present, omitted otherwise |
| `description` | see below |

### Description format

```
[**{orch_name}**](https://tools.livepeer.cloud/orchestrator/{orch_addr}) earned **{lpt:.4} LPT**{ (~${usd:.2})}? in inflation rewards.

[View transaction](https://arbiscan.io/tx/{tx_hash})
```

The `(~${usd:.2})` parenthesized USD value is appended only when `amount_usd > 0` (the explorer returns valuations alongside the event when priced). If the orchestrator has no `display_name`, the raw address is shown in the bold link text.

## 5. Subscriber digest DM (delegator activity)

Built with `serenity::all::CreateMessage` + `CreateEmbed`. Sent privately every `SUBSCRIBER_DIGEST_INTERVAL_SECS` (default 900s) to each subscriber, scoped to one orchestrator. Source: `src/domains/notify/dm.rs::build_delegator_digest_dm`.

| Field | Value |
|---|---|
| `title` | `"Delegator activity"` |
| `color` | `#46a758` (green) |
| `timestamp` | end of the digest window |
| `thumbnail.url` | orchestrator `avatar_url` if present, omitted otherwise |
| `description` | see below |

### Description structure

```
[**{orch_name}**](https://tools.livepeer.cloud/orchestrator/{orch_addr}) had delegator activity:

**New delegators (N)**
• [`{short_addr}`](https://arbiscan.io/tx/{tx_hash}) bonded **{lpt:.4} LPT**
…

**Stake increases (N)**
• [`{short_addr}`](https://arbiscan.io/tx/{tx_hash}) added **{lpt:.4} LPT**
…

**Unbonds (N)**
• [`{short_addr}`](https://arbiscan.io/tx/{tx_hash}) unbonded **{lpt:.4} LPT**
…

**Rebonds (N)**
• [`{short_addr}`](https://arbiscan.io/tx/{tx_hash}) rebonded **{lpt:.4} LPT**
…
```

Sections are omitted entirely when empty. If all four buckets are empty for a (subscriber, orch) pair in this window, no DM is sent. The transaction link points at the specific Bond / Unbond / Rebond on Arbiscan. Address truncation matches the slash command convention (`0x123456…7890`).

### New-vs-stake-change classification

Bonds are split into "New delegators" (no prior `delegator_events` row for this `(delegator, orch)` pair) and "Stake increases" (one or more prior rows exist). The check is a `COUNT(*)` against the same table with `block_timestamp < this_event.block_timestamp`.

## 6. Slash command response embeds

Built with `serenity::all::CreateEmbed` (NOT `serde_json::Value`) because they are gateway responses, not webhook posts. All slash command replies are **ephemeral** (`CreateReply::default().ephemeral(true)`). See `src/domains/commands/`.

### Color palette

| Variant | RGB |
|---|---|
| Success / informational | `#46a758` (green) |
| Error / not-found | `#d04a4a` (red) |
| Neutral (e.g. "you weren't subscribed") | `#969696` (grey) |
| `/orchestrator rewards` | `#ffa500` (orange — matches digest AI accent) |
| `/orchestrator tickets` | `#ffd700` (gold — matches digest transcoding accent) |

### Per-command shape

- **`/subscribe <orchestrator>`** — Title: `Subscribed` or `Already subscribed` or `Error`. Description includes the orchestrator's `display_name`, truncated address, and `N of CAP` usage line. Sends `error_reply` for invalid address, cap reached, or orchestrator-not-found.
- **`/unsubscribe <orchestrator>`** — Title: `Unsubscribed` or `Not subscribed` or `Error`. Description echoes the truncated address.
- **`/subscriptions`** — Title: `Your subscriptions`. Description either lists each subscription as `• **{name}** — \`{short_addr}\`` or shows an empty-state line pointing at `/subscribe`. Lookups are best-effort: if the explorer lookup for a name fails, the address is shown instead so the list still renders.
- **`/orchestrator delegators <orchestrator>`** — Title: `Delegators of {short_addr}`. Description lists top-10 delegators ranked by current stake (`pending_stake` when present and positive, otherwise `bonded_principal`), each line as `**#{rank}** \`{short_addr}\` — {LPT:.2} LPT ({pct:.2}%)`. Percentages are computed against the orchestrator profile's `total_stake`, not just the 10 displayed rows or the sum of fetched `bonded_principal` values. Footer line: `_Top N by stake; total stake: {LPT:.2} LPT_`.
- **`/orchestrator rewards <orchestrator> <period>`** — Title: `Rewards · {short_addr}`. Description has the period label + date range, then `Reward events: N`, `Total distributed: X LPT ($Y)`, `Orchestrator cut: X LPT ($Y)`, `Delegators cut: X LPT`. Empty case: `No reward activity for {short_addr} in {period} ({from} – {to}).`
- **`/orchestrator tickets <orchestrator> <period>`** — Title: `Tickets · {short_addr}`. Same shape as rewards but with ETH/USD on face value, commission, and delegators' share, plus distinct gateways count.

### Address truncation

All addresses in command responses use `short_addr()` → `0x1234…5678`. The first 6 chars and last 4 chars; an ellipsis between. Source: `src/domains/commands/mod.rs::short_addr`.

### Period window semantics

`daily | weekly | monthly` always refers to the **last complete UTC period**:

- daily → yesterday (`today − 1` day)
- weekly → previous Mon–Sun
- monthly → previous month, 1st through last day

Source: `src/domains/commands/orchestrator.rs::period_window`.

## Test expectations

Each builder has a snapshot test in `tests/embeds.rs` that constructs a known input fixture and asserts the produced JSON matches a checked-in `tests/fixtures/{single_ticket,digest_ai,digest_tx,summary_daily}.json`. Any divergence from those fixtures fails CI — embed shape is part of the contract.
