# livepeer-payout-bot

Rust service that watches the Livepeer protocol explorer API, persists event state in SQLite, and delivers Discord notifications.

The app has two deployment shapes:

- `webhook-only` mode posts public-channel payout digests and daily/weekly/monthly network summaries to a Discord webhook.
- `commands-enabled` mode adds a Discord bot user, slash commands, per-user orchestrator subscriptions, reward DMs, and delegator-activity digest DMs.

Detailed architecture lives in [docs/design-docs/architecture.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/architecture.md). Core repo rules live in [docs/design-docs/core-beliefs.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/core-beliefs.md).

## Overview

The bot consumes the explorer API only. It does not talk to chain nodes, subgraphs, or external cron.

Core responsibilities:

- Poll `WinningTicketRedeemed` events and post orchestrator payout digests.
- Post closed-period network summaries for daily, weekly, and monthly windows.
- Optionally expose Discord slash commands for subscribing to orchestrators.
- Optionally DM subscribers about `Reward`, `Bond`, `Unbond`, and `Rebond` activity.
- Persist cursors, dedup state, delivery watermarks, and subscription data in SQLite.

## High-level features

- Typed explorer boundary: upstream JSON is parsed into generated Rust types in `src/domains/explorer/types.rs`.
- Strict startup validation: env vars are parsed once in [src/config.rs](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/src/config.rs); bad config fails fast.
- Append-only persistence: migrations are additive, cursors are explicit, and delivery flags are written after successful sends.
- Contract-locked embeds: webhook and DM payloads are snapshot-tested in [tests/embeds.rs](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/tests/embeds.rs).
- Architecture guardrails: domain import rules are enforced in [tests/architecture.rs](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/tests/architecture.rs).
- Optional interactive mode: slash commands, DM delivery, and cold-start seeding are enabled only when `COMMANDS_ENABLED=true`.

## Project organization

```text
.
├── AGENTS.md                  # repository map and “read next” guide
├── README.md                  # operator/developer entrypoint
├── Cargo.toml                 # crate metadata and pinned dependencies
├── Dockerfile                 # container build
├── migrations/                # append-only SQLite schema
├── infra/                     # compose + image build helpers
├── docs/
│   ├── design-docs/           # architecture and invariants
│   ├── product-specs/         # exact message/embed contracts
│   ├── generated/             # vendored OpenAPI input
│   └── exec-plans/            # completed implementation plans
├── src/
│   ├── main.rs                # process bootstrap and tracing init
│   ├── config.rs              # env parsing and validation
│   ├── runtime.rs             # object graph + task spawning
│   ├── seed.rs                # cross-domain delegator-history seeding
│   ├── providers/             # HTTP, DB, Discord clients, gateway runtime
│   └── domains/
│       ├── explorer/          # typed REST client and API boundary
│       ├── state/             # SQLite repos for public bot state
│       ├── subscriptions/     # SQLite repo for user subscriptions
│       ├── notify/            # webhook and DM payload builders
│       ├── scheduler/         # pollers and posters
│       └── commands/          # slash command handlers
└── tests/                     # structural and snapshot tests
```

## Conventions

- Parse at the boundary. External bytes become typed structs before business logic touches them.
- Domains are stratified. `explorer` and `subscriptions` are strict leaves; `state` may only import `explorer::types`; composition belongs in `scheduler`, `commands`, `seed.rs`, and `runtime.rs`.
- Product docs are contracts. If embed output changes, [docs/product-specs/messages.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/product-specs/messages.md) and snapshot tests must change in the same PR.
- Startup is strict. There is no silent degraded mode for bad config, missing migrations, or failed boot wiring.
- Migrations are append-only. Never edit an already-deployed migration.

## Building

Requirements:

- Rust `1.95` via [rust-toolchain.toml](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/rust-toolchain.toml)
- SQLite via `sqlx` runtime linkage only; no separate local DB service is required

Local build and test:

```sh
cargo fmt --check
cargo test
cargo build --release
```

Run locally:

```sh
cp .env.example .env
# fill required values
cargo run --release
```

Container build:

```sh
docker build -t livepeer-payout-bot .
```

Compose-based run:

```sh
cp infra/.env.example infra/.env
docker compose -f infra/docker-compose.yaml up -d
```

## Configuration

The full env var contract is documented in [.env.example](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/.env.example). The key variables are:

Required in all modes:

- `EXPLORER_BASE_URL`: Livepeer protocol explorer base URL.
- `DISCORD_WEBHOOK_URL`: Discord webhook for public digest and summary embeds.
- `DATABASE_URL`: SQLite connection string.

Optional timing and transport knobs:

- `EVENT_POLL_INTERVAL_SECS`
- `DIGEST_WINDOW_SECS`
- `DIGEST_FETCH_LIMIT`
- `SUMMARY_POLL_INTERVAL_SECS`
- `HTTP_TIMEOUT_SECS`
- `RUST_LOG`
- `USER_AGENT`

Additional variables when `COMMANDS_ENABLED=true`:

- `DISCORD_BOT_TOKEN`
- `DISCORD_APPLICATION_ID`
- `DISCORD_GUILD_ID`
- `MAX_SUBSCRIPTIONS_PER_USER`
- `DM_FAILURE_AUTO_UNSUB`
- `REWARD_POLL_INTERVAL_SECS`
- `DELEGATOR_POLL_INTERVAL_SECS`
- `SUBSCRIBER_DIGEST_INTERVAL_SECS`

## Runtime model

`src/runtime.rs` constructs shared providers once, then spawns long-lived Tokio tasks:

- Always on:
  - `event_poller`
  - `digest_poster`
  - `summary_poster`
- Only when commands are enabled:
  - startup delegator-history seed
  - `reward_poller`
  - `delegator_poller`
  - `subscriber_digest_poster`
  - Discord gateway / slash command runtime

The process exits on `SIGINT`/`SIGTERM` or when a spawned task dies unexpectedly.

## Documentation map

- Repo map: [AGENTS.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/AGENTS.md)
- Detailed architecture: [docs/design-docs/architecture.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/architecture.md)
- Operating rules: [docs/design-docs/core-beliefs.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/core-beliefs.md)
- Embed contract: [docs/product-specs/messages.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/product-specs/messages.md)
- Upstream API contract input: [docs/generated/openapi.json](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/generated/openapi.json)
