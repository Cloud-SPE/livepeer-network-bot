# Exec plan 004 — Subscriptions (umbrella)

**Goal:** let Discord users opt-in to personalized notifications for one or more orchestrators. Public channel (existing) is unchanged; subscribers additionally receive private DMs scoped to the orchestrators they follow.

**Status:** in flight, split into three landings:

| Phase | What lands | Status |
|---|---|---|
| [004a](004a-slash-commands.md) | Slash commands + gateway runtime + subscriptions table | in progress |
| [004b](004b-event-pollers.md) | Reward + delegator pollers + DmSender provider + per-event reward DMs | pending |
| [004c](004c-subscriber-digest.md) | Subscriber digest poster (15-min delegator change DMs) + auto-unsubscribe on 403 | pending |

## Locked decisions

| Decision | Choice |
|---|---|
| DM cadence | Hybrid: Rewards per-event, delegator changes 15-min digest |
| Delegator event set | `Bond` + `Unbond` + `Rebond`, differentiate new vs. stake-change via local history |
| Subscribers receive `WinningTicketRedeemed` DMs | No — tickets stay public-channel only in v1 |
| Cap on subscriptions per user | 25 (`MAX_SUBSCRIPTIONS_PER_USER`) |
| Auto-unsubscribe on Discord 403 | After 3 consecutive DM failures per subscription |
| Library | `poise` 0.6 (built on `serenity`) |
| Slash command response embeds | `serenity::all::CreateEmbed` builder (separate from existing `serde_json::Value` webhook embeds) |
| Receive transport | Gateway WebSocket — no public HTTP ingress required |

## Out of scope for 004

- Per-subscriber cadence configuration
- Subscriber-channel routing (we do per-user DMs only)
- Slash commands for non-subscription-related operations beyond `/orchestrator delegators|rewards|tickets`
- Notifications for `EarningsClaimed`, `TransferBond`, `WithdrawStake`, `WithdrawFees`

## New env vars (introduced in 004a, used progressively)

| Var | Required when | Notes |
|---|---|---|
| `COMMANDS_ENABLED` | always (default `false`) | Master switch — webhook-only deploys keep working without bot infra |
| `DISCORD_BOT_TOKEN` | `COMMANDS_ENABLED=true` | bot user token from the developer portal |
| `DISCORD_APPLICATION_ID` | `COMMANDS_ENABLED=true` | numeric app ID for command registration |
| `DISCORD_GUILD_ID` | optional | when set, registers commands per-guild for instant updates during dev |
| `MAX_SUBSCRIPTIONS_PER_USER` | optional, default `25` | per-user cap enforced by `/subscribe` |
| `DM_FAILURE_AUTO_UNSUB` | optional, default `3` | consecutive 403s before auto-removing a subscription |
