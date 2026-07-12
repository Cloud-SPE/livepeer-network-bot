# livepeer-payout-bot

[![CI](https://github.com/Cloud-SPE/livepeer-network-bot/actions/workflows/ci.yml/badge.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95-orange.svg?logo=rust)](rust-toolchain.toml)
[![Edition 2021](https://img.shields.io/badge/edition-2021-blue.svg?logo=rust)](Cargo.toml)
[![Made with Rust](https://img.shields.io/badge/Made%20with-Rust-dea584.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/async-tokio-8a2be2.svg)](https://tokio.rs/)
[![SQLx](https://img.shields.io/badge/db-sqlx-blueviolet.svg)](https://github.com/launchbadge/sqlx)
[![SQLite](https://img.shields.io/badge/storage-SQLite-003B57.svg?logo=sqlite&logoColor=white)](https://www.sqlite.org/)
[![Poise](https://img.shields.io/badge/discord-poise-5865F2.svg?logo=discord&logoColor=white)](https://github.com/serenity-rs/poise)
[![Serenity](https://img.shields.io/badge/discord-serenity-5865F2.svg?logo=discord&logoColor=white)](https://github.com/serenity-rs/serenity)
[![Docker](https://img.shields.io/badge/container-docker-2496ED.svg?logo=docker&logoColor=white)](Dockerfile)
[![Platform: Linux](https://img.shields.io/badge/platform-linux-lightgrey.svg?logo=linux&logoColor=white)](#)
[![Code style: rustfmt](https://img.shields.io/badge/code%20style-rustfmt-1f425f.svg)](https://github.com/rust-lang/rustfmt)
[![Lints: clippy](https://img.shields.io/badge/lints-clippy-yellowgreen.svg)](https://github.com/rust-lang/rust-clippy)
[![GitHub last commit](https://img.shields.io/github/last-commit/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/commits/main)
[![GitHub commit activity](https://img.shields.io/github/commit-activity/m/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/commits/main)
[![GitHub issues](https://img.shields.io/github/issues/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/issues)
[![GitHub pull requests](https://img.shields.io/github/issues-pr/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/pulls)
[![GitHub stars](https://img.shields.io/github/stars/Cloud-SPE/livepeer-network-bot.svg?style=social)](https://github.com/Cloud-SPE/livepeer-network-bot/stargazers)
[![GitHub forks](https://img.shields.io/github/forks/Cloud-SPE/livepeer-network-bot.svg?style=social)](https://github.com/Cloud-SPE/livepeer-network-bot/network/members)
[![GitHub watchers](https://img.shields.io/github/watchers/Cloud-SPE/livepeer-network-bot.svg?style=social)](https://github.com/Cloud-SPE/livepeer-network-bot/watchers)
[![Repo size](https://img.shields.io/github/repo-size/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot)
[![Code size](https://img.shields.io/github/languages/code-size/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot)
[![Top language](https://img.shields.io/github/languages/top/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot)
[![Languages](https://img.shields.io/github/languages/count/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot)
[![Contributors](https://img.shields.io/github/contributors/Cloud-SPE/livepeer-network-bot.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/graphs/contributors)
[![Maintenance](https://img.shields.io/maintenance/yes/2026.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/commits/main)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/Cloud-SPE/livepeer-network-bot/pulls)
[![Conventional Commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-yellow.svg)](https://www.conventionalcommits.org)
[![Livepeer](https://img.shields.io/badge/network-Livepeer-00EB88.svg)](https://livepeer.org/)

Rust service that watches the Livepeer protocol explorer API, persists event state in SQLite, and delivers Discord notifications.

The app has two deployment shapes:

- `webhook-only` mode posts public-channel payout digests and daily/weekly/monthly network summaries to a Discord webhook.
- `commands-enabled` mode adds a Discord bot user, slash commands, per-user orchestrator subscriptions, reward DMs, delegator-activity digest DMs, reward-cut / fee-share change DMs, and the reward-call watch (pending/missed reward DMs plus a public delinquency digest).

Detailed architecture lives in [docs/design-docs/architecture.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/architecture.md). Core repo rules live in [docs/design-docs/core-beliefs.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/core-beliefs.md).

## Overview

The bot consumes the explorer API only. It does not talk to chain nodes, subgraphs, or external cron.

Core responsibilities:

- Poll `WinningTicketRedeemed` events and post orchestrator payout digests.
- Post closed-period network summaries for daily, weekly, and monthly windows.
- Optionally expose Discord slash commands for subscribing to orchestrators.
- Optionally DM subscribers about `Reward`, `Bond`, `Unbond`, `Rebond`, and `TranscoderUpdate` (reward-cut / fee-share change) activity.
- Optionally watch reward calls per round: DM subscribers when a subscribed orchestrator has not called reward as the round progresses, post a public digest of all delinquent active orchestrators when the round locks, and DM a final missed-reward notice after the round closes.
- Persist cursors, dedup state, delivery watermarks, and subscription data in SQLite.

## High-level features

- Typed explorer boundary: upstream JSON is parsed into generated Rust types in `src/domains/explorer/types.rs`.
- Strict startup validation: env vars are parsed once in [src/config.rs](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/src/config.rs); bad config fails fast.
- Append-only persistence: migrations are additive, cursors are explicit, and delivery flags are written after successful sends.
- Contract-locked embeds: webhook and DM payloads are snapshot-tested in [tests/embeds.rs](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/tests/embeds.rs).
- Architecture guardrails: domain import rules are enforced in [tests/architecture.rs](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/tests/architecture.rs).
- Optional interactive mode: slash commands, DM delivery, the reward-call watch, and cold-start seeding are enabled only when `COMMANDS_ENABLED=true`.
- Optional observability: a Prometheus `/metrics` + `/health` endpoint is served when `METRICS_BIND` is set.

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

For containerized runs, keep `DATABASE_URL` on the mounted `/data` volume, for
example `sqlite:///data/livepeer-payout-bot.db`. A relative SQLite path such as
`sqlite://./livepeer-payout-bot.db` lives inside the container filesystem and
will appear to "lose" subscriptions and cursors after container replacement.

## Configuration

The full env var contract is documented in [.env.example](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/.env.example). The key variables are:

Required in all modes:

- `EXPLORER_BASE_URL`: Livepeer protocol explorer base URL.
- `DISCORD_WEBHOOK_URL`: Discord webhook(s) for public digest and summary
  embeds. Accepts a single URL, or several comma-separated URLs to fan the
  same posts out to multiple servers (one webhook per server channel).
  Delivery is best-effort per webhook and all servers share one global
  send watermark, so a permanently-broken webhook silently misses messages
  until fixed rather than blocking or duplicating to the healthy ones.
- `DATABASE_URL`: SQLite connection string.

Optional timing and transport knobs:

- `EVENT_POLL_INTERVAL_SECS`
- `DIGEST_WINDOW_SECS`
- `DIGEST_FETCH_LIMIT`
- `SUMMARY_POLL_INTERVAL_SECS`
- `SUMMARY_SETTLE_DAILY_SECS` / `SUMMARY_SETTLE_WEEKLY_SECS` /
  `SUMMARY_SETTLE_MONTHLY_SECS` / `SUMMARY_MAX_DEFER_SECS` (summary
  readiness gating; see `.env.example`)
- `HTTP_TIMEOUT_SECS`
- `RUST_LOG`
- `USER_AGENT`
- `METRICS_BIND` (e.g. `0.0.0.0:9300`; serves Prometheus `/metrics` +
  `/health`, disabled when unset)

Optional safety flag:

- `WEBHOOK_POST_ENABLED` (default `true`): when set to `false`, the bot
  still polls and persists events but does not spawn `digest_poster` or
  `summary_poster`, so nothing is sent to `DISCORD_WEBHOOK_URL`. Use it in
  a dev process that shares its webhook URL with prod to avoid
  double-posting. Flipping back to `true` drains the backlog at the next
  digest boundary.

Additional variables when `COMMANDS_ENABLED=true`:

- `DISCORD_BOT_TOKEN`
- `DISCORD_APPLICATION_ID`
- `DISCORD_GUILD_ID`
- `MAX_SUBSCRIPTIONS_PER_USER`
- `DM_FAILURE_AUTO_UNSUB`
- `REWARD_POLL_INTERVAL_SECS`
- `DELEGATOR_POLL_INTERVAL_SECS`
- `CUT_CHANGE_POLL_INTERVAL_SECS`
- `SUBSCRIBER_DIGEST_INTERVAL_SECS`
- `REWARD_WATCH_ENABLED` / `REWARD_WATCH_POLL_INTERVAL_SECS` /
  `REWARD_WATCH_FIRST_ALERT_PCT` / `REWARD_WATCH_REALERT_STEP_PCT` /
  `REWARD_WATCH_DIGEST_PCT` / `ROUND_LENGTH_BLOCKS` (reward-call watch;
  see `.env.example`)

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
  - `cut_change_poller`
  - `subscriber_digest_poster`
  - `reward_watch_poller` (unless `REWARD_WATCH_ENABLED=false`)
  - Discord gateway / slash command runtime
- Detached (never fatal): the Prometheus `/metrics` + `/health` server,
  spawned only when `METRICS_BIND` is set.

The process exits on `SIGINT`/`SIGTERM` or when one of the always-on webhook
tasks dies unexpectedly. In commands-enabled mode, the Discord gateway is
treated as non-critical and is restarted in-process so slash-command trouble
does not take down payout digests or summaries.

## Documentation map

- Repo map: [AGENTS.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/AGENTS.md)
- Detailed architecture: [docs/design-docs/architecture.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/architecture.md)
- Operating rules: [docs/design-docs/core-beliefs.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/design-docs/core-beliefs.md)
- Embed contract: [docs/product-specs/messages.md](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/product-specs/messages.md)
- Upstream API contract input: [docs/generated/openapi.json](/home/mazup/git-repos/livepeer-cloud-spe/livepeer-network-bot/docs/generated/openapi.json)

## License

Licensed under the [MIT License](LICENSE).
