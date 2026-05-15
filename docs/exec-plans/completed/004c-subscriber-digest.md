# Exec plan 004c — Subscriber digest poster

**Goal:** deliver the per-subscriber DM of Bond / Unbond / Rebond events accumulated in `delegator_events`, closing out the subscriptions feature.

**Parent:** [004-subscriptions.md](004-subscriptions.md). Follows 004b. After this lands, all four 004 plans move to `completed/`.

## What lands

### New code

| Path | Purpose |
|---|---|
| `src/domains/scheduler/subscriber_digest_poster.rs` | 15-min wall-clock-aligned tick. Reads unsent delegator events, groups by orch, finds subscribers, fans out one DM per (subscriber, orch). Same auto-unsub pattern as `reward_poller`. |
| `src/domains/notify/dm.rs` | `DelegatorDigest<'_>` aggregation struct + `build_delegator_digest_dm` that produces the CreateMessage. |

### Code changes

- `src/domains/state/event_streams.rs` — `count_prior_delegator_events(delegator, orch, before)` for the new-vs-stake-change classification on Bonds. `fetch_unsent_delegator_events` and `mark_delegator_events_sent` are no longer `#[allow(dead_code)]`.
- `src/config.rs` — `SUBSCRIBER_DIGEST_INTERVAL_SECS` (default 900).
- `src/runtime.rs` — spawns the new task alongside the existing pollers.
- `docs/product-specs/messages.md` — new section documenting the digest DM shape.
- `.env.example` — documents the new variable.

## New-vs-stake-change classification

For each `Bond` row in the digest window:

```sql
SELECT COUNT(*) FROM delegator_events
WHERE delegator_address = ? AND orch_address = ? AND block_timestamp < ?
```

Zero → "new delegator." Non-zero → "stake increase." Note that `delegator_history.first_seen_at` is also written by the delegator poller, but the query above is cheaper and equivalent for our purposes.

**Known limitation:** on a freshly-seeded database, every existing delegator's first observed Bond will be labeled "new." This is acceptable — the bot is meant to run continuously; cold-start hallucinations decay within a few digest cycles.

## Marking semantics

An event row is marked `sent_to_subscribers = 1` once every subscriber for its orch has had a delivery attempt this tick. Per-subscriber transient failures don't roll the whole event back; they're logged. Persistent 403s drive the per-subscription failure counter that auto-unsubscribes (reused from 004b).

If an orch has zero subscribers, events still get marked sent — there's nobody to notify, so leaving them unsent would just accumulate.

## Verification

1. `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build` — all green
2. With `COMMANDS_ENABLED=true` and an active subscription to an orch that receives a Bond, a DM arrives within one `SUBSCRIBER_DIGEST_INTERVAL_SECS` tick
3. Bonding to the same orch twice in one window → first labeled "new delegator", second labeled "stake increase"
4. Subscriber with DMs disabled receives 3 attempts (across three windows or events), then is auto-removed

## After 004c

All four exec-plans (`004`, `004a`, `004b`, `004c`) move from `docs/exec-plans/active/` to `docs/exec-plans/completed/`. The subscriptions feature is complete.
