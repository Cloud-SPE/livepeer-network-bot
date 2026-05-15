# livepeer-payout-bot

Discord bot that watches `WinningTicketRedeemed` events from the [Livepeer protocol explorer](https://livepeer-network-api.cloudspe.com/) and posts:

- **Real-time digests** of new winning tickets, grouped by orchestrator
- **Daily / weekly / monthly summaries** of network-wide payout activity

Data flows in one direction: explorer REST API → local SQLite (cursor + dedup state) → Discord webhook. No blockchain access, no subgraph queries, no host cron.

## Where to look first

Start at `AGENTS.md`. It is a short map of the repository. From there, dive into:

- `docs/design-docs/` — architecture decisions and core operating principles
- `docs/product-specs/messages.md` — exact Discord embed templates
- `docs/exec-plans/active/` — work currently in flight

## Running locally

```sh
cp .env.example .env       # fill DISCORD_WEBHOOK_URL etc.
cargo run --release
```

The binary creates / migrates the SQLite database on startup and then enters three concurrent loops (event poller, digest poster, summary poster) until `SIGTERM`.
