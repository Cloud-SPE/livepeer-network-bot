# Exec plan 004a — Slash commands + gateway runtime

**Goal:** stand up the bot user, the gateway connection, the slash command registration path, and the six v1 commands. Subscriptions persist intent only — no DM delivery yet.

**Parent:** [004-subscriptions.md](004-subscriptions.md). Followed by 004b and 004c.

## What lands

### New deps
- `poise = "0.6"` — pulls `serenity` transitively

### New env vars (see umbrella 004 for table)
- `COMMANDS_ENABLED`, `DISCORD_BOT_TOKEN`, `DISCORD_APPLICATION_ID`, `DISCORD_GUILD_ID`, `MAX_SUBSCRIPTIONS_PER_USER`

### New code

| Path | Purpose |
|---|---|
| `migrations/0002_subscriptions.sql` | `subscriptions` table (no reward_events / delegator_events yet — those are 004b) |
| `src/domains/subscriptions/mod.rs` + `repo.rs` | `SqliteSubscriptionsRepo` with `insert`, `delete`, `list_for_user`, `count_for_user`, `find_for_orchestrator` |
| `src/providers/discord_gateway.rs` | Owns the poise `Framework` + serenity `Client`. Registers commands globally or per-guild. Drives the gateway to shutdown signal. |
| `src/domains/commands/mod.rs` | `BotData` struct, `CommandContext`/`CommandError` type aliases, `all_commands()` aggregator |
| `src/domains/commands/subscribe.rs` | `/subscribe <orchestrator>` — validates address via explorer, enforces cap, inserts row |
| `src/domains/commands/unsubscribe.rs` | `/unsubscribe <orchestrator>` |
| `src/domains/commands/subscriptions.rs` | `/subscriptions` — lists the invoking user's subscriptions |
| `src/domains/commands/orchestrator.rs` | `/orchestrator delegators|rewards|tickets` — three subcommands in one file |

### Runtime change

`src/runtime.rs` spawns a 4th task that calls `discord_gateway::run(...)` when `COMMANDS_ENABLED=true`. When disabled, the task is not spawned and the bot runs in webhook-only mode (unchanged from v0).

### Doc changes

- `docs/product-specs/messages.md` gains a new section: "Command response embeds." These are built with `serenity::all::CreateEmbed` (not `serde_json::Value`) and are ephemeral (`flags: 64`) by default.
- `.env.example` gets the five new variables documented.

## Slash command contract

| Command | Args | Replies with |
|---|---|---|
| `/subscribe <orchestrator>` | address (string) | embed: confirm + N/cap usage; or error if invalid / over cap / already subscribed |
| `/unsubscribe <orchestrator>` | address (string) | embed: confirm or "you weren't subscribed" |
| `/subscriptions` | — | embed listing user's orchs (name + truncated addr) or empty-state |
| `/orchestrator delegators <orchestrator>` | address | embed: top-10 delegators by stake, with %-of-total footer |
| `/orchestrator rewards <orchestrator> <period>` | address + enum (daily/weekly/monthly) | embed: per-orch reward summary for the period (filtered from leaderboard) |
| `/orchestrator tickets <orchestrator> <period>` | address + enum (daily/weekly/monthly) | embed: per-orch ticket summary for the period |

All replies are ephemeral by default. Period summaries use the last complete UTC day/week/month, not today-so-far.

## Out of scope for 004a (deferred to 004b / 004c)

- `DmSender` provider (added in 004b alongside reward_poller)
- `reward_events` / `delegator_events` / `delegator_history` tables
- Per-event DM delivery
- 15-min subscriber digest
- 403 → DM-blocked state (`dm_failure_count` column exists, but nothing increments it yet)

## Verification

1. `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cargo build` — all green
2. With `COMMANDS_ENABLED=false`, binary starts and runs the existing three loops as before
3. With `COMMANDS_ENABLED=true` + valid bot creds, slash commands appear in Discord after registration and respond as specified
