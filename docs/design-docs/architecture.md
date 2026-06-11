# Architecture

## One-paragraph summary

`livepeer-payout-bot` is a single Rust process that polls the Livepeer protocol explorer REST API, stores cursors and delivery state in SQLite, and sends Discord notifications. In its base shape it posts public webhook digests for `WinningTicketRedeemed` events plus daily/weekly/monthly network summaries. When `COMMANDS_ENABLED=true`, it also opens a Discord gateway connection, serves slash commands, stores user subscriptions, seeds subscription-scoped history, and delivers subscriber DMs for `Reward`, delegator activity, and reward-cut / fee-share changes.

## Architectural goals

- Keep upstream integration narrow: one explorer REST API contract, one SQLite file, one Discord surface.
- Preserve deterministic delivery semantics: persist first, mark sent after successful delivery.
- Make boundaries explicit and typed so contract drift fails in code review and tests, not at runtime.
- Keep deployment simple: one binary, no external scheduler, no separate database service.
- Allow the slash-command/DM feature set without contaminating the webhook-only core.

## System context

```mermaid
flowchart LR
    Explorer["Livepeer protocol explorer API"]
    Bot["livepeer-payout-bot"]
    Sqlite[("SQLite")]
    Webhook["Discord webhook"]
    Gateway["Discord gateway + bot HTTP API"]
    Users["Discord users"]

    Explorer --> Bot
    Bot <--> Sqlite
    Bot --> Webhook
    Bot <--> Gateway
    Gateway --> Users
```

## Deployment modes

### 1. Webhook-only mode

Enabled when `COMMANDS_ENABLED=false`.

Long-lived tasks:

- `event_poller`
- `digest_poster` (only when `WEBHOOK_POST_ENABLED=true`)
- `summary_poster` (only when `WEBHOOK_POST_ENABLED=true`)

Used for public-channel reporting only.

`WEBHOOK_POST_ENABLED` defaults to `true`. Setting it to `false` is a
dev-side safety flag: events still poll and persist, but the digest and
summary posters are not spawned, so nothing is sent to
`DISCORD_WEBHOOK_URL`. The flag exists so a dev process can share a
webhook URL with prod without double-posting.

### 2. Commands-enabled mode

Enabled when `COMMANDS_ENABLED=true`.

Includes everything from webhook-only mode, plus:

- startup seeding of `delegator_history`
- startup seeding of existing cut-change history into `cut_change_events`
- Discord gateway runtime for slash commands
- `reward_poller`
- `delegator_poller`
- `cut_change_poller`
- `subscriber_digest_poster`

Used for interactive subscriptions and direct-message fan-out.

## Component model

```mermaid
flowchart TD
    subgraph Process["Single Tokio process"]
        Main["main.rs\nbootstrap + tracing"]
        Runtime["runtime.rs\nobject graph + task spawning"]
        Config["config.rs\nenv parsing"]

        subgraph Providers["providers/"]
            Http["http.rs\nreqwest::Client"]
            Db["database.rs\nSQLite pool + migrations"]
            Webhook["discord.rs\nwebhook notifier"]
            Dm["discord_bot.rs\nDM sender"]
            Gateway["discord_gateway.rs\npoise + serenity runtime"]
        end

        subgraph Domains["domains/"]
            Explorer["explorer/\ntyped API client"]
            State["state/\npublic state repos"]
            Subs["subscriptions/\nsubscription repo"]
            Notify["notify/\nembed + DM builders"]
            Sched["scheduler/\npollers + posters"]
            Commands["commands/\nslash handlers"]
        end

        Seed["seed.rs\ncross-domain seeding"]
    end

    Main --> Config
    Main --> Runtime
    Runtime --> Http
    Runtime --> Db
    Runtime --> Webhook
    Runtime --> Dm
    Runtime --> Gateway
    Runtime --> Explorer
    Runtime --> State
    Runtime --> Subs
    Runtime --> Sched
    Runtime --> Commands
    Runtime --> Seed
    Sched --> Notify
    Commands --> Seed
```

## Domain layering and dependency rules

The repo does not use unconstrained “feature modules.” It uses stratified domains with specific import rules.

| Layer | Modules | Allowed cross-domain imports |
|---|---|---|
| Strict leaves | `explorer`, `subscriptions` | none |
| Persistence | `state` | `explorer::types` only |
| Formatters | `notify` | typed leaves as input |
| Composers | `scheduler`, `commands` | any domain as needed |
| Crate-root orchestration | `runtime.rs`, `seed.rs` | cross-domain composition |

Mechanical enforcement:

- `tests/architecture.rs` fails if strict-leaf domains import another domain.
- `tests/architecture.rs` fails if `state` imports anything beyond `explorer::types`.

This split is deliberate:

- `explorer` owns the upstream REST contract.
- `state` owns durable local state for public polling and summaries.
- `subscriptions` owns subscriber rows and DM failure counters.
- `notify` owns presentation contracts.
- `scheduler` owns background workflows.
- `commands` owns request/response style user interaction.
- `seed.rs` handles orchestration that crosses persistence domains but does not fit a single background loop.

## Repository layout and responsibility map

| Path | Responsibility |
|---|---|
| `src/main.rs` | loads `.env`, configures JSON tracing, parses config, enters runtime |
| `src/config.rs` | parses env vars once and fails on invalid input |
| `src/runtime.rs` | constructs providers/repos/clients and spawns all Tokio tasks |
| `src/seed.rs` | idempotent cold-start and per-subscribe seeding for subscription-scoped history |
| `src/providers/http.rs` | shared `reqwest::Client` |
| `src/providers/database.rs` | opens SQLite, enables WAL, runs migrations |
| `src/providers/discord.rs` | webhook sender with rate-limit awareness and retries |
| `src/providers/discord_bot.rs` | bot-authenticated DM sender |
| `src/providers/discord_gateway.rs` | `poise` / `serenity` gateway runtime and command registration |
| `src/domains/explorer/` | generated API types and typed client methods |
| `src/domains/state/` | repos for `events`, `cursors`, `summary_watermarks`, stream tables |
| `src/domains/subscriptions/` | repo for `subscriptions` table |
| `src/domains/notify/` | webhook embed builders and DM message builders |
| `src/domains/scheduler/` | recurring pollers and posters |
| `src/domains/commands/` | slash command handlers |
| `migrations/` | append-only SQLite schema history |
| `tests/embeds.rs` | snapshot tests for output contracts |
| `tests/architecture.rs` | structural architecture checks |

## Runtime startup flow

```mermaid
sequenceDiagram
    participant OS as Process start
    participant Main as main.rs
    participant Config as Config::from_env
    participant HTTP as providers/http
    participant DB as providers/database
    participant Runtime as runtime.rs
    participant Tasks as Tokio tasks

    OS->>Main: launch binary
    Main->>Main: load .env and init tracing
    Main->>Config: parse env vars
    Config-->>Main: Config
    Main->>Runtime: run(config)
    Runtime->>HTTP: build reqwest client
    Runtime->>DB: open SQLite + run migrations
    Runtime->>Runtime: construct clients/repos/providers
    Runtime->>Tasks: spawn always-on schedulers
    alt COMMANDS_ENABLED=true
        Runtime->>Runtime: seed subscription history
        Runtime->>Tasks: spawn reward/delegator/cut-change/subscriber tasks
        Runtime->>Tasks: spawn Discord gateway runtime
    end
    Runtime->>Runtime: wait for ctrl-c or unexpected core-task exit
```

## Data flow by feature

### Public payout digests

`WinningTicketRedeemed` is the primary public event stream.

```mermaid
sequenceDiagram
    participant Poller as event_poller
    participant Explorer as Explorer API
    participant State as SqliteStateRepo
    participant Poster as digest_poster
    participant Notify as notify/embed
    participant Discord as Discord webhook

    loop every EVENT_POLL_INTERVAL_SECS
        Poller->>State: read cursor(events:WinningTicketRedeemed)
        Poller->>Explorer: GET /api/v1/events?event_name=WinningTicketRedeemed
        Explorer-->>Poller: typed EventRow page
        Poller->>State: INSERT OR IGNORE events
        Poller->>State: advance cursor
    end

    loop every digest boundary
        Poster->>State: fetch unsent events
        Poster->>Explorer: fetch orchestrator + gateway profiles
        Poster->>Notify: build single-ticket or grouped digest embeds
        Notify-->>Poster: JSON payload
        Poster->>Discord: POST webhook
        alt 2xx
            Poster->>State: mark sent_to_discord=1
        else failure
            Poster->>Poster: leave rows pending for retry
        end
    end
```

Important semantics:

- Ingestion and posting are decoupled by SQLite.
- Events are inserted before they become eligible to post.
- A failed webhook send does not lose data; the rows remain pending.
- Digest grouping is by orchestrator and job type (`ai` vs non-`ai` gateway).

### Network summaries

```mermaid
sequenceDiagram
    participant Summary as summary_poster
    participant State as SqliteStateRepo
    participant Explorer as Explorer API
    participant Notify as notify/embed
    participant Discord as Discord webhook

    loop every SUMMARY_POLL_INTERVAL_SECS
        Summary->>Summary: compute last closed daily/weekly/monthly periods
        Summary->>State: check summary_watermarks
        alt not yet posted
            Summary->>Explorer: GET /payouts/summary/{period}/{date}
            Summary->>Explorer: GET /payouts/leaderboard
            Summary->>Notify: build summary embed
            Summary->>Discord: POST webhook
            Summary->>State: insert summary watermark
        end
    end
```

Important semantics:

- Summaries always cover the last fully closed UTC period.
- Watermarks prevent duplicate daily/weekly/monthly posts across restarts.

### Subscription commands and DM delivery

Commands mode introduces two independent but related flows: command handling and background DM delivery.

#### Slash commands

```mermaid
sequenceDiagram
    participant User as Discord user
    participant Gateway as discord_gateway
    participant Cmd as commands/*
    participant Explorer as Explorer API
    participant Subs as SqliteSubscriptionsRepo
    participant Seed as seed.rs
    participant Streams as EventStreamsRepo

    User->>Gateway: /subscribe 0x...
    Gateway->>Cmd: invoke handler
    Cmd->>Explorer: validate orchestrator exists
    Cmd->>Subs: INSERT OR IGNORE subscription
    alt new subscription
        Cmd->>Seed: seed_one(orch)
        Seed->>Explorer: list orchestrator delegators
        Seed->>Streams: record first-seen delegators
        Cmd->>Seed: seed_cut_history_one(orch)
        Seed->>Explorer: list transcoder params history
        Seed->>Streams: mark existing cut changes sent
    end
    Cmd-->>User: ephemeral success/error embed
```

Command surface today:

- `/subscribe`
- `/unsubscribe`
- `/subscriptions`
- `/orchestrator delegators`
- `/orchestrator rewards`
- `/orchestrator tickets`
- `/orchestrator cuts`

#### Reward DMs

```mermaid
sequenceDiagram
    participant Reward as reward_poller
    participant Explorer as Explorer API
    participant State as SqliteStateRepo
    participant Streams as EventStreamsRepo
    participant Subs as SqliteSubscriptionsRepo
    participant DM as BotDmSender
    participant User as Subscriber

    loop every REWARD_POLL_INTERVAL_SECS
        Reward->>State: read cursor(events:Reward)
        Reward->>Explorer: list Reward events
        Reward->>Streams: insert reward_events
        Reward->>State: advance cursor
        Reward->>Streams: fetch unsent reward_events
        Reward->>Subs: find subscribers by orch
        Reward->>Explorer: get orchestrator profile
        Reward->>DM: send per-event DM
        DM-->>User: Discord DM
        Reward->>Subs: clear or increment dm_failure_count
        Reward->>Streams: mark reward event sent
    end
```

#### Delegator activity DMs

```mermaid
sequenceDiagram
    participant Poller as delegator_poller
    participant Digest as subscriber_digest_poster
    participant Explorer as Explorer API
    participant State as SqliteStateRepo
    participant Streams as EventStreamsRepo
    participant Subs as SqliteSubscriptionsRepo
    participant DM as BotDmSender

    loop every DELEGATOR_POLL_INTERVAL_SECS
        Poller->>State: read cursor per event name
        Poller->>Explorer: list Bond / Unbond / Rebond events
        Poller->>Streams: insert delegator_events
        opt new Bond
            Poller->>Streams: record_first_seen(delegator, orch)
        end
        Poller->>State: advance cursor
    end

    loop every SUBSCRIBER_DIGEST_INTERVAL_SECS
        Digest->>Streams: fetch unsent delegator_events
        Digest->>Subs: find subscribers by orch
        Digest->>Streams: classify new bond vs stake change
        Digest->>Explorer: get orchestrator profile
        Digest->>DM: send grouped DM per user + orch
        Digest->>Subs: clear or increment dm_failure_count
        Digest->>Streams: mark delegator events sent
    end
```

Important semantics:

- DM flows do not reuse the webhook sender; they use a bot-authenticated Discord HTTP client.
- DM-blocked state is driven by consecutive DM `403` failures; subscription rows are retained.
- Transient DM failures are logged but do not cause per-event replay, which avoids duplicate delivery to users who already received the message.

#### Cut-change DMs

```mermaid
sequenceDiagram
    participant Poller as cut_change_poller
    participant Explorer as Explorer API
    participant Streams as EventStreamsRepo
    participant Subs as SqliteSubscriptionsRepo
    participant DM as BotDmSender

    loop every CUT_CHANGE_POLL_INTERVAL_SECS
        Poller->>Subs: distinct subscribed orchestrators
        Poller->>Explorer: params history per subscribed orch
        Poller->>Streams: insert TranscoderUpdate rows
        Poller->>Streams: fetch unsent cut_change_events
        Poller->>Subs: find subscribers by orch
        Poller->>Explorer: get orchestrator profile
        Poller->>DM: send per-event DM
        Poller->>Subs: clear or increment dm_failure_count
        Poller->>Streams: mark cut-change event sent
    end
```

When an orchestrator is first observed by this poller, existing historical
TranscoderUpdate rows are inserted as already sent. This keeps first deploys
from backfilling old cut-change DMs. New `/subscribe` calls also seed existing
cut history immediately, before the next poller tick, for the same reason.

## Persistence model

### Tables

| Table | Purpose |
|---|---|
| `events` | persisted `WinningTicketRedeemed` rows plus webhook sent watermark |
| `cursors` | named opaque explorer cursors for pollers |
| `summary_watermarks` | one row per posted closed period |
| `subscriptions` | `(discord_user_id, orchestrator_address)` pairs and DM failure counters |
| `reward_events` | persisted `Reward` events with DM sent watermark |
| `delegator_events` | persisted `Bond`/`Unbond`/`Rebond` events with DM sent watermark |
| `delegator_history` | first-seen marker for `(delegator, orch)` |
| `cut_change_events` | persisted `TranscoderUpdate` rows with DM sent watermark |

### Why SQLite

- Embedded and operationally simple.
- Strong enough for the small event volume.
- Supports idempotent inserts, cursors, and queryable local state.
- WAL mode is enabled at open time for better concurrent reader/writer behavior.

### Persistence invariants

- Pollers use `INSERT OR IGNORE` for deduplication.
- Cursors advance only after the current page has been persisted.
- Public webhook rows are marked sent only after a successful 2xx webhook response.
- Summary posts are de-duplicated by `(period, period_date)`.
- DM stream events are marked sent after delivery attempts for the relevant subscriber set are complete.

## Upstream API boundary

The explorer API is the only upstream domain source.

Boundary rules:

- Generated OpenAPI-backed types are re-exported from `src/domains/explorer/types.rs`.
- Internal code consumes typed structs, not raw `serde_json::Value`.
- Helper enums and traits that do not exist in the OpenAPI contract, like `Cadence` and `GatewayProfileRowExt`, live alongside the re-exports.

Explorer endpoints used today:

- `GET /api/v1/events` for `WinningTicketRedeemed`, `Reward`, `Bond`, `Unbond`, `Rebond`
- `GET /api/v1/orchestrators/{address}`
- `GET /api/v1/transcoders/{address}/params/history`
- `GET /api/v1/orchestrators/{address}/delegators`
- `GET /api/v1/gateways/{address}/profile`
- `GET /api/v1/payouts/summary/{period}/{date}`
- `GET /api/v1/payouts/leaderboard`
- `GET /api/v1/rewards/leaderboard`

## Discord boundary

Two delivery channels exist on purpose.

### Webhook delivery

Used for public digest and summary embeds.

- Implemented in `src/providers/discord.rs`
- Shared `reqwest::Client`
- Bucket-aware rate limiting based on Discord response headers
- Retries bounded 429s and 5xx responses
- Leaves state pending on failure so the next poster run can retry

### Bot-authenticated delivery

Used for DMs and slash commands.

- `src/providers/discord_bot.rs` wraps `serenity::Http` for DMs
- `src/providers/discord_gateway.rs` owns the `poise` framework and gateway connection
- Command registration can be global or guild-scoped depending on `DISCORD_GUILD_ID`

## Output contracts

Message payloads are treated as product contracts, not incidental formatting.

- Public webhook embeds are built in `src/domains/notify/embed.rs`.
- DM payloads are built in `src/domains/notify/dm.rs`.
- Expected JSON lives in [../product-specs/messages.md](../product-specs/messages.md).
- `tests/embeds.rs` snapshot-tests the exact output shape.

## Configuration model

`src/config.rs` is the only place that reads environment variables.

Properties:

- required values fail fast with descriptive errors
- durations are parsed once into `std::time::Duration`
- command-mode config is grouped into `CommandsConfig`
- empty or invalid optional command values still fail when commands mode is enabled

This keeps “what environment is required to boot” explicit and testable.

## Failure handling and restart posture

- Invalid config: process exits during boot.
- Migration failure: process exits during boot.
- Explorer/API errors during scheduled work: iteration logs error; next tick retries.
- Webhook send failure: rows remain unsent and are retried later.
- DM `403`: increments failure counter and can mark the subscription DM-blocked.
- DM transient failure: logged, event still considered attempted to avoid duplicate sends.
- Unexpected exit of a core webhook task (`event_poller`, `digest_poster`, `summary_poster`): `runtime.rs` logs the exit and shuts the process down rather than limping in an unknown partial state.
- Commands-only infrastructure failures (most notably the Discord gateway runtime): logged and restarted in-process so webhook digests and summaries keep flowing.
- Containerized deploys must use an absolute SQLite path (for example `sqlite:///data/livepeer-payout-bot.db`) so the DB lives on the mounted volume instead of disappearing with the container filesystem.
- `WEBHOOK_POST_ENABLED=false` skips spawning `digest_poster` and `summary_poster`; the `event_poller` keeps running so the queue drains automatically when the flag is flipped back to `true`.

## Known tradeoffs

- `WinningTicketRedeemed` commission calculations use the orchestrator’s current `fee_cut_percent`, not a historical point-in-time value.
- DM delivery favors “no duplicate spam” over “perfect at-least-once replay” for transient subscriber-specific failures.
- The process is intentionally single-binary and single-DB; multi-tenant routing and horizontal sharding are out of scope.
- The commands-enabled runtime broadens the product beyond the original three-loop webhook bot, so docs and architecture tests need to stay current as features land.

## What to update before changing architecture

If a change alters runtime boundaries or responsibilities, update these docs in the same PR:

- `docs/design-docs/architecture.md`
- `docs/design-docs/core-beliefs.md` if the rule set changes
- `README.md` if setup, modes, or top-level features change
- `AGENTS.md` if the repo map or “what this project is” summary changes
