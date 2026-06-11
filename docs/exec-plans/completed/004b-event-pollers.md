# Exec plan 004b — Reward + delegator pollers, DM sender, DM-blocked state

**Goal:** stand up the data ingestion + per-event DM delivery for subscribers. After this lands, subscribers receive a DM every time the explorer surfaces a `Reward` event for an orch they follow. Bond/Unbond/Rebond rows are persisted but not yet delivered — that's 004c.

**Parent:** [004-subscriptions.md](004-subscriptions.md). Follows 004a; precedes 004c.

## What lands

### New migration

`migrations/0003_event_streams.sql` — three tables:

| Table | Purpose |
|---|---|
| `reward_events` | Mirror of `/events?event_name=Reward`. `sent_to_subscribers_at` tracks per-event delivery. |
| `delegator_events` | Bond / Unbond / Rebond rows. `sent_to_subscribers` (boolean) gates the 004c digest fan-out. |
| `delegator_history` | First-seen marker per `(delegator, orch)`. Consulted in 004c to label new delegators vs. stake changes. |

### New code

| Path | Purpose |
|---|---|
| `src/domains/state/event_streams.rs` | `EventStreamsRepo` — CRUD over the three new tables |
| `src/domains/notify/dm.rs` | `build_reward_event_dm` returning `serenity::all::CreateMessage` |
| `src/providers/discord_bot.rs` | `BotDmSender` wrapping `serenity::http::Http`. `DmError` enum distinguishes `DmsClosed` (403) / `RateLimited` (429) / `Other`. |
| `src/domains/scheduler/reward_poller.rs` | Tick: ingest new `Reward` events → dispatch DMs to each subscriber → mark sent. Marks subscriptions DM-blocked after `DM_FAILURE_AUTO_UNSUB` consecutive 403s. |
| `src/domains/scheduler/delegator_poller.rs` | Tick: three sequential paginated fetches (Bond / Unbond / Rebond). Persists only. Updates `delegator_history` on first-time Bonds. |

### Code changes

- `src/domains/explorer/client.rs` — `list_events(event_name, cursor, limit)` extracted as a generic helper; `list_winning_tickets` now delegates to it.
- `src/domains/subscriptions/repo.rs` — `increment_dm_failure(user, orch) -> i64` and `clear_dm_failure(user, orch)` for the DM-blocked counter.
- `src/config.rs` — `REWARD_POLL_INTERVAL_SECS` (default 60), `DELEGATOR_POLL_INTERVAL_SECS` (default 60), `DM_FAILURE_AUTO_UNSUB` (default 3, inside `CommandsConfig`).
- `src/runtime.rs` — spawns two new tasks (reward + delegator pollers) only when `COMMANDS_ENABLED=true` (the DM sender requires a bot token).
- `.env.example` — documents the three new variables.

## Deliverable semantics

| Event class | Cadence | On 4xx | On 5xx / network |
|---|---|---|---|
| Reward (per-event DM) | Within ~1 poll interval of explorer surfacing the event | `DmsClosed` increments counter; marks subscription DM-blocked at threshold | Logged, event marked sent (no per-event retry to avoid duplicate delivery to subscribers that already received it) |
| Delegator events | Persisted only in 004b — DM delivery shipped in 004c | n/a | n/a |

## What is intentionally NOT in 004b

- No subscriber digest for delegator events (004c).
- No DM fan-out for `WinningTicketRedeemed` — public channel only, per locked decisions.
- No per-event valuation accuracy beyond what the explorer surfaces (`amount_native`, `amount_usd`, `native_usd_price`).

## Verification

1. `cargo fmt --all --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build` — all green
2. With `COMMANDS_ENABLED=false`, neither poller starts; binary identical to v0/004a webhook-only mode
3. With `COMMANDS_ENABLED=true` + a known orch and a `/subscribe`'d user, a fresh `Reward` event on that orch produces a DM within ~1 minute of the explorer indexing it
4. Subscribing with DMs disabled → after 3 reward events, the subscription is marked DM-blocked (visible in SQLite + logs)
