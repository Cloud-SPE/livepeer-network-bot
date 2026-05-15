# Architecture

## One-paragraph summary

A single Rust binary runs three concurrent scheduler loops:

1. **Event poller** — queries `GET /api/v1/events?event_name=WinningTicketRedeemed&with_valuations=true&cursor=…` against the Livepeer protocol explorer, persists new event rows into SQLite, advances the cursor watermark. Never posts to Discord.
2. **Digest poster** — on wall-clock N-minute boundaries (default every 15 minutes), reads oldest unposted events from SQLite up to a bounded per-run cap, groups by orchestrator + job type (AI / transcoding), enriches with `/orchestrators/{address}` and `/gateways/{address}/profile` lookups, sorts the concrete outgoing messages by effective embed timestamp ascending (oldest first), and POSTs them to a Discord webhook. Marks events as posted.
3. **Summary poster** — at daily / weekly / monthly boundaries, reads `/payouts/summary/{period}/{date}` for network totals plus `/payouts/leaderboard?from=…&to=…&limit=10` for the per-orch top-10 and posts the summary embed. Watermarks prevent double-posting.

All three loops share an `Arc<AppState>` that holds typed providers (HTTP client, Discord notifier, SQLite pool, clock). They never call each other.

## Locked decisions

| Decision | Choice | Rationale |
|---|---|---|
| Language | Rust 1.95, single binary | Matches the rest of the Livepeer ecosystem; backend-rs code can be ported directly. |
| Data source | `livepeer-protocol-explorer` REST API only | No chain access, no subgraph. Single upstream contract. |
| Boundary types | Hand-written serde structs in `src/domains/explorer/types.rs`; `progenitor` codegen tracked separately | See `docs/exec-plans/active/002-progenitor-codegen.md`. Spec lives at `docs/generated/openapi.json`. |
| State store | SQLite via `sqlx`, file path from `DATABASE_URL` | Embedded, zero-ops, survives restarts, queryable for dedup. |
| Discord | Webhook only, raw `reqwest::Client` POST, behind a `Notifier` trait | Mirrors backend-rs. No gateway connection, no bot account. Trait keeps a future serenity bot pluggable. |
| Scope | All orchestrators network-wide | Single webhook target. No per-tenant config. |
| Scheduler | Three `tokio::time::interval` loops in one process | No host cron. Intervals internal. Graceful shutdown via `tokio::signal::ctrl_c`. |
| Logging | `tracing` + JSON formatter, level from `LOG_LEVEL` | Structured by default. |
| Migrations | `sqlx::migrate!` runs at startup, append-only | One source of truth for schema. |

## Dependency direction

Inside each domain, code can only depend "forward" through:

```
types → repo → service
```

`types` are pure serde structs with no I/O dependencies. `repo` owns SQL or HTTP calls. `service` orchestrates `repo` calls into business operations.

Cross-cutting concerns (HTTP client construction, Discord webhook, clock, DB pool) live in `src/providers/` and are passed into domains via constructor injection. **Domains never import each other.** Composition happens exclusively in `src/runtime.rs`.

This is mechanically checkable by reading the `mod.rs` files. A future task adds a structural test (`tests/architecture.rs`) that fails CI if a domain imports another.

## Embed-to-API field map

The Discord embeds are byte-for-byte copies of the `livepeer-backend-rs` originals. The mapping from backend-rs's internal `Ticket` model to the explorer API:

| Embed field (backend-rs name) | Explorer API source |
|---|---|
| `t.recipient_id` | event `to_address` |
| `t.sender_id` | event `from_address` |
| `t.face_value` (ETH, f64) | parse(event `amount_native`) |
| `t.face_value_usd` | parse(event `valuations[0].amount_usd`) |
| `t.eth_price` | parse(event `valuations[0].native_usd_price`) |
| `t.transaction_id` | event `tx_hash` |
| `t.timestamp` | event `block_timestamp` |
| `t.fee_cut` | parse(`GET /orchestrators/{to_address}.fee_cut_percent`) / 100 |
| `t.orch_commission` | derived `face_value * fee_cut` |
| `t.orch_commission_usd` | derived `face_value_usd * fee_cut` |
| `orch.name` | `GET /orchestrators/{to_address}.display_name` |
| `orch.avatar` | `GET /orchestrators/{to_address}.avatar_url` |
| `broadcaster.name` | `GET /gateways/{from_address}/profile.display_name` |
| `is_ai_job(broadcaster)` | `GET /gateways/{from_address}/profile.kind == "ai"` |
| 24h rolling totals | SQL aggregate over local `events` table |
| Summary `report.total_ticket` | `GET /payouts/summary/{period}/{date}.ticket_count` |
| Summary `report.total_eth` | `summary.sum_face_value_native` |
| Summary `report.total_orch_commission_eth` | `summary.sum_commission_native` |
| Summary top-10 rows | `GET /payouts/leaderboard?from=…&to=…&limit=10` |

## Known limitations

- **Historical fee_cut.** We use the orchestrator's *current* `fee_cut_percent`. For a 15-minute digest window this is effectively the same value, but it can drift if an orchestrator updates their fee cut between ticket redemption and digest post. Acceptable for v1; revisit if the explorer adds `as_of_block` to the orchestrator endpoint.
- **Discord rate limiting** is bucket-aware: `providers/discord.rs` reads `x-ratelimit-remaining`, `x-ratelimit-reset-after`, and `x-ratelimit-scope` on every response and respects them on the next send. Bounded 3-attempt retry on 429; non-429 4xx/5xx return Err and events stay `sent_to_discord=0` to be re-attempted on the next window. Open items in `docs/exec-plans/active/003-discord-rate-limiting.md`.
- **No per-tenant config.** One webhook URL, one set of intervals. Multi-tenant is out of scope for v1.

## Performance posture

The bot's working set is tiny: a few hundred winning tickets per day at most, a handful of gateways, ~100 orchestrators. Caching `/orchestrators/{address}` and `/gateways/{address}/profile` responses in memory with a 5-minute TTL is sufficient. No connection pool tuning needed beyond `reqwest` defaults. SQLite's WAL mode is enabled by the migration.
