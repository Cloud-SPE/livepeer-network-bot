# Exec plan 006 — Embed snapshot tests

**Goal:** lock the byte-for-byte JSON shape of every embed builder so format-string regressions break CI instead of silently changing user-visible Discord posts.

**Status:** ✅ shipped.

## What landed

`tests/embeds.rs` with five snapshot tests, one per builder:

| Test | Builder | Output type |
|---|---|---|
| `single_ticket_embed_shape` | `domains::notify::embed::build_single_ticket` | `serde_json::Value` (webhook) |
| `digest_embed_shape` | `domains::notify::embed::build_digest` | `serde_json::Value` (webhook) |
| `summary_embed_shape` | `domains::notify::embed::build_summary` | `serde_json::Value` (webhook) |
| `reward_dm_shape` | `domains::notify::dm::build_reward_event_dm` | `serenity::all::CreateMessage` |
| `delegator_digest_dm_shape` | `domains::notify::dm::build_delegator_digest_dm` | `serenity::all::CreateMessage` |

Each test builds fixtures inline (deterministic addresses, timestamps, amounts) and compares the produced JSON to a literal `serde_json::json!({…})` expected value via `pretty_assertions::assert_eq`. Diffs render with color and clear field-level deltas on failure.

## Conventions

- **Webhook embeds.** Compared whole-payload — `username`, `avatar_url`, and `embeds` are all part of our contract.
- **DM CreateMessage.** Compared at `payload["embeds"][0]` only. Serenity's `CreateMessage` serializes envelope defaults (`attachments: []`, `tts: false`, `enforce_nonce: false`, `sticker_ids: []`) we don't care about. Embedded fields like `type: "rich"` and thumbnail `height/proxy_url/width: null` ARE included in the expected value so changes to those are caught.
- **Fixtures.** All addresses use `0xorch0001`, `0xbcst0001`, etc. — short, obvious, and distinct from real production addresses. Timestamps use `2026-05-15` (today, when this was written) for visual recognition during debugging.

## When a builder change is intentional

Update the format string AND the matching expected literal in the same change. CI fails noisily otherwise — that's the point.

## What's not covered

- Property-style edge cases (zero amounts, missing avatars, empty leaderboard rows). Each builder has one happy-path test; if specific edge behavior regresses we add a targeted test then.
- Error paths (e.g. malformed `amount_native`). The builders silently default to 0.0 via `parse_decimal`; if that's wrong, the user will see "0.0000 ETH" in production. Worth a property test later.
