# Discord embed templates

This document is the **contract**. The Rust embed builders in `src/domains/notify/embed.rs` must produce JSON that matches these shapes byte-for-byte. Format strings, color constants, and URL patterns are lifted verbatim from `livepeer-backend-rs/src/tasks/ticket_digest.rs` and `payout_summary.rs`.

## Common webhook envelope

Every public webhook payload posted by the bot uses the same envelope. DM and slash-command responses use `serenity` builders and are documented separately below.

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
- **Incomplete-backstop variant:** when the readiness gate's `SUMMARY_MAX_DEFER_SECS` backstop forces a post before the explorer finished indexing the period, a `footer.text` is added: `⚠️ Data may be incomplete — published before the explorer finished indexing this period.` The description body stays byte-for-byte identical to the normal path.

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

## 6. Cut-change DM (subscriber)

Built with `serenity::all::CreateMessage` + `CreateEmbed`. Sent privately to each user subscribed to the orchestrator that emitted a new `TranscoderUpdate` row. Source: `src/domains/notify/dm.rs::build_cut_change_dm`.

| Field | Value |
|---|---|
| `title` | `"Cut change"` |
| `color` | `#969696` (grey) |
| `timestamp` | event `block_timestamp` |
| `thumbnail.url` | orchestrator `avatar_url` if present, omitted otherwise |
| `description` | see below |

### Description format

```
[**{orch_name}**](https://tools.livepeer.cloud/orchestrator/{orch_addr}) updated its cuts:

Reward cut: **{reward_cut_percent:.2}%** (orchestrator)
Fee Share: **{fee_share_percent:.2}%** (delegators)
Fee Cut: **{fee_cut_percent:.2}%** (orchestrator)

[View transaction](https://arbiscan.io/tx/{tx_hash})
```

The first time the bot observes an orchestrator's params history, existing rows are seeded as already sent. Subscribers only receive DMs for newly observed cut changes after tracking begins.

## 7. Slash command response embeds

Built with `serenity::all::CreateEmbed` (NOT `serde_json::Value`) because they are gateway responses, not webhook posts. All slash command replies are **ephemeral** (`CreateReply::default().ephemeral(true)`). See `src/domains/commands/`.

### Color palette

| Variant | RGB |
|---|---|
| Success / informational | `#46a758` (green) |
| Error / not-found | `#d04a4a` (red) |
| Neutral (e.g. "you weren't subscribed") | `#969696` (grey) |
| `/orchestrator rewards` | `#ffa500` (orange — matches digest AI accent) |
| `/orchestrator tickets` | `#ffd700` (gold — matches digest transcoding accent) |
| `/orchestrator cuts` | `#969696` (grey) |

### Per-command shape

- **`/subscribe <orchestrator>`** — Title: `Subscribed` or `Already subscribed` or `Error`. On first subscribe the description is `Now following **{name}** (\`{short_addr}\`). You're subscribed to {N} of {CAP} orchestrators.` followed by a blank line and a DM-settings paragraph: `📩 Notifications arrive as DMs. Make sure we share a server and that **"Allow direct messages from server members"** is enabled, or I won't be able to reach you — /subscriptions shows delivery status.` The `Already subscribed` case is just `You're already following **{name}** (\`{short_addr}\`).` with no usage line. Sends `error_reply` for invalid address, cap reached, or orchestrator-not-found.
- **`/unsubscribe <orchestrator>`** — Title: `Unsubscribed` or `Not subscribed` or `Error`. Description echoes the truncated address.
- **`/subscriptions`** — Title: `Your subscriptions`. Description opens with `You follow {N} of {CAP} orchestrators:` then lists each subscription as `• **{name}** — \`{short_addr}\``, with a trailing `  ⚠️ DM-blocked` marker on entries whose DMs are paused. When any entry is DM-blocked, a closing paragraph explains that the subscription is kept, notifications are paused, and delivery resumes automatically on the next successful DM. Empty state shows a line pointing at `/subscribe`. Lookups are best-effort: if the explorer lookup for a name fails, the address is shown instead so the list still renders.
- **`/orchestrator delegators <orchestrator>`** — Title: `Delegators of {short_addr}`. Description lists top-10 delegators ranked by current stake (`pending_stake` when present and positive, otherwise `bonded_principal`), each line as `**#{rank}** \`{short_addr}\` — {LPT:.2} LPT ({pct:.2}%)`. Percentages are computed against the orchestrator profile's `total_stake`, not just the 10 displayed rows or the sum of fetched `bonded_principal` values. Footer line: `_Top N by stake; total stake: {LPT:.2} LPT_`.
- **`/orchestrator rewards <orchestrator> <period>`** — Title: `Rewards · {short_addr}`. Description has the period label + date range, then `Reward events: N`, `Total distributed: X LPT ($Y)`, `Orchestrator cut: X LPT ($Y)`, `Delegators cut: X LPT`. Empty case: `No reward activity for {short_addr} in {period} ({from} – {to}).`
- **`/orchestrator tickets <orchestrator> <period>`** — Title: `Tickets · {short_addr}`. Same shape as rewards but with ETH/USD on face value, commission, and delegators' share, plus distinct gateways count.
- **`/orchestrator cuts <orchestrator>`** — Title: `Current cuts · {short_addr}`. Description is exactly `Reward cut: **{reward_cut_percent:.2}%** (orchestrator)`, `Fee Share: **{fee_share_percent:.2}%** (delegators)`, and `Fee Cut: **{fee_cut_percent:.2}%** (orchestrator)` on separate lines.

### Address truncation

All addresses in command responses use `short_addr()` → `0x1234…5678`. The first 6 chars and last 4 chars; an ellipsis between. Source: `src/domains/commands/mod.rs::short_addr`.

### Period window semantics

`daily | weekly | monthly` always refers to the **last complete UTC period**:

- daily → yesterday (`today − 1` day)
- weekly → previous Mon–Sun
- monthly → previous month, 1st through last day

Source: `src/domains/commands/orchestrator.rs::period_window`.

## 8. Reward-call pending DM (subscriber)

Built with `serenity::all::CreateMessage` + `CreateEmbed`. Sent privately to each subscriber of an active orchestrator that has not called reward for the current round, once per ladder rung (`REWARD_WATCH_FIRST_ALERT_PCT`, then every `REWARD_WATCH_REALERT_STEP_PCT` of round completion). Source: `src/domains/notify/dm.rs::build_reward_watch_dm`.

| Field | Value |
|---|---|
| `title` | `"Reward call pending"` |
| `color` | `#e67e22` (orange) |
| `timestamp` | send time (`Timestamp::now()`, not an event timestamp) |
| `thumbnail.url` | orchestrator `avatar_url` if present, omitted otherwise |
| `footer.text` | `"Reward-call status lags chain finality by up to ~25 minutes."` |
| `description` | see below |

### Description format

```
[**{orch_name}**](https://tools.livepeer.cloud/orchestrator/{orch_addr}) has **not called reward** for round **{round}** yet.

Round progress: block ~**{est_block} of {round_length_blocks}** ({elapsed_pct:.0}% complete)
Time left to call reward: **~{Hh Mm}**

If no reward call lands before the round ends, delegators earn no inflation rewards from this orchestrator for the round.
```

- `{est_block}`/`{elapsed_pct}` are estimates derived from the round's `started_at` at 12s per L1 block; the remaining time renders as `{h}h {m}m`, or `{m}m` when under an hour.
- Inactive orchestrators never trigger this DM.

## 9. Reward call missed DM (subscriber)

Built with `serenity::all::CreateMessage` + `CreateEmbed`. Sent privately once per `(round, orchestrator)` after a round closes without a reward call, deferred until the explorer has indexed past the round boundary. Source: `src/domains/notify/dm.rs::build_reward_missed_dm`.

| Field | Value |
|---|---|
| `title` | `"Reward call missed"` |
| `color` | `#cc3333` (red) |
| `timestamp` | send time (`Timestamp::now()`) |
| `thumbnail.url` | orchestrator `avatar_url` if present, omitted otherwise |
| `description` | see below |

### Description format

```
[**{orch_name}**](https://tools.livepeer.cloud/orchestrator/{orch_addr}) **did not call reward** during round **{round}**.

Delegators earned no inflation rewards from this orchestrator for that round. The reward call for the new round is still available.
```

## 10. Reward-call delinquency digest (public webhook)

A fourth public webhook embed, posted through the common envelope once per round when the round passes `REWARD_WATCH_DIGEST_PCT` (default 90%, the protocol lock point) and at least one active orchestrator has not called reward. Source: `src/domains/notify/embed.rs::build_reward_watch_digest`.

| Field | Value |
|---|---|
| `color` | `0xE67E22` (orange) |
| `title` | `"Reward calls pending — round locked"` |
| `timestamp` | send time, RFC 3339 |
| `description` | see below |

### Description format

```
Round **{round}** is **{elapsed_pct:.0}% complete** (block ~{est_block} of {round_length_blocks}) and has entered its lock period. **{count}** active orchestrator{ has| s have} not called reward yet:

• [**{name}**](https://tools.livepeer.cloud/orchestrator/{address}) — {total_stake:.0} LPT staked
…

Delegators to these orchestrators earn no inflation rewards for the round unless a reward call lands before it ends.
```

- Covers **all** active orchestrators (not just subscribed ones), sorted largest stake first.
- At most 20 bullet lines; overflow renders as `…and {N} more`.
- `{name}` falls back to the truncated address when the orchestrator has no `display_name`.

## Test expectations

Each builder has a snapshot test in `tests/embeds.rs` that constructs a known input fixture and asserts the produced JSON matches a checked-in `tests/fixtures/{single_ticket,digest_ai,digest_tx,summary_daily}.json`. Any divergence from those fixtures fails CI — embed shape is part of the contract.
