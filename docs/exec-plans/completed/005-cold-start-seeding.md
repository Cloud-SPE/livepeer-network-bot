# Exec plan 005 — Cold-start `delegator_history` seeding

**Goal:** eliminate the "fresh DB labels every existing delegator as new" mis-classification noted in 004c.

**Status:** ✅ shipped.

## Problem

On a freshly-seeded SQLite, `delegator_history` is empty. The first `Bond` event we observe for an orchestrator's *existing* delegator looks identical to one for a brand-new delegator — `count_prior_delegator_events` returns 0 in both cases — so 004c's digest builder labels them all as "new delegator." This decays as the bot runs but is wrong on day one and after every fresh deploy.

## What landed

`src/domains/subscriptions/seed.rs` with two helpers:

- `seed_one(explorer, streams, orch_address)` — pages `/api/v1/orchestrators/{addr}/delegators` (default limit 500, cursor-paginated) and `INSERT OR IGNORE`s each `(delegator, orch)` pair into `delegator_history`.
- `seed_all_subscribed(explorer, streams, subscriptions)` — finds every distinct orch with a subscriber and seeds subscription-scoped history for each. It originally called `seed_one` for delegator history only; it now also marks existing cut-change history as already sent.

Wired in two places:

| Where | When | Why |
|---|---|---|
| `runtime::run` | Synchronously on startup, before any poller spawns | The digest poster's first tick must not see un-seeded history |
| `commands::subscribe` | After a successful `subscriptions.insert` | New subscriptions don't suffer the same gap |

Failures are logged at `warn` and the bot continues — seeding is best-effort, not load-bearing.

## Supporting changes

- `subscriptions::repo::distinct_subscribed_orchestrators` — `SELECT DISTINCT orchestrator_address FROM subscriptions`
- `explorer::client::orchestrator_delegators` — gained a `cursor` parameter (was previously single-page only). Existing caller in `/orchestrator delegators` passes `None`.
- `commands::BotData` — gained `streams: Arc<EventStreamsRepo>` so the `/subscribe` handler can call the seeder.
- `providers::discord_gateway::run` — signature gained the `streams` argument.

## Out of scope

- **Network-wide seeding.** We seed only orchs that already have subscribers. Subscribing to a totally-new orchestrator first triggers the per-orch seed inside `/subscribe`.
- **Periodic re-seed.** If a delegator unbonds entirely and re-bonds months later, we'll still label them "new delegator" — `delegator_history` is append-only. Acceptable for v1.
- **Concurrency.** `seed_all_subscribed` runs orchs sequentially. With low subscriber counts and a small distinct-orch set, latency is fine. If the orch count grows, parallelize with bounded concurrency.
