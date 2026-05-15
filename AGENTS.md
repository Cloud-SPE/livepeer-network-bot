# AGENTS.md

This is the **map** of the repository. It is intentionally short. Deeper context lives in `docs/`.

## What this project is

A Rust binary that consumes the Livepeer protocol explorer REST API, persists event and subscription state in SQLite, and sends Discord notifications.

Base mode posts public webhook messages for:

- per-orchestrator `WinningTicketRedeemed` digests
- daily / weekly / monthly network summaries

Optional commands mode also enables:

- slash commands for subscribing to orchestrators
- reward DMs to subscribers
- delegator-activity digest DMs
- Discord gateway connectivity for the bot user

## Where to read next

| If you need to know… | Read |
|---|---|
| Why the architecture is shaped this way | `docs/design-docs/architecture.md` |
| The non-negotiable principles for this repo | `docs/design-docs/core-beliefs.md` |
| The exact Discord embed templates we must produce | `docs/product-specs/messages.md` |
| What work is in flight right now | `docs/exec-plans/active/` |
| The upstream API contract | `docs/generated/openapi.json` |
| The OpenAI Codex "harness engineering" article we modeled this on | `docs/references/openai-harness.pdf` |

## Repository layout

```
.
├── AGENTS.md                  # this file — map only
├── Cargo.toml                 # single binary, pinned deps
├── rust-toolchain.toml        # pinned channel
├── Dockerfile                 # container build
├── docs/
│   ├── design-docs/           # system of record for "why"
│   ├── exec-plans/            # active/completed work plans
│   ├── product-specs/         # what we ship (embed templates)
│   ├── generated/             # vendored artifacts (openapi.json)
│   └── references/            # external reading
├── infra/                     # container/runtime helpers
├── migrations/                # SQLite schema, applied at startup
├── ops/                       # SQL run against external systems
├── src/
│   ├── main.rs                # wiring only
│   ├── lib.rs                 # crate exports
│   ├── config.rs              # boundary parse of env vars
│   ├── runtime.rs             # composes domains, spawns loops
│   ├── seed.rs                # cross-domain delegator-history seeding
│   ├── providers/             # cross-cutting: http, discord, db, clock
│   └── domains/
│       ├── explorer/          # types, client, service
│       ├── state/             # sqlite repo + service
│       ├── subscriptions/     # subscriber persistence
│       ├── notify/            # Notifier trait, embed builders
│       ├── scheduler/         # pollers and posters
│       └── commands/          # slash command handlers
└── tests/                     # integration + drift tests
```

## Dependency direction

Per `docs/design-docs/architecture.md`, modules depend strictly downward through:

```
Types → Repo → Service → Runtime
```

Cross-cutting concerns (HTTP client, Discord webhook, Discord bot HTTP client, gateway runtime, clock, DB pool) enter every domain through `providers/` only. Domain coupling rules are enforced mechanically by `tests/architecture.rs`, and cross-domain composition belongs in `src/runtime.rs` or crate-root helpers like `src/seed.rs`.

If you need to add a new behavior that doesn't fit, **stop and update `docs/design-docs/architecture.md` first**.

## How to work in this repo

1. Read the relevant `docs/design-docs/` and `docs/product-specs/` page before writing code. If something is missing or stale, fix the doc in the same PR.
2. Embed JSON must match `docs/product-specs/messages.md` byte-for-byte. The format strings come from `livepeer-backend-rs` and are part of the contract.
3. Anything from the explorer API is parsed into a typed struct at the boundary (`src/domains/explorer/types.rs`). Never thread raw `serde_json::Value` through the runtime.
4. New env vars are documented in `.env.example` and validated in `src/config.rs`. Startup panics on bad config — there is no "missing-config fallback."
5. Migrations are append-only. Never edit a migration that has been deployed; add a new one.
