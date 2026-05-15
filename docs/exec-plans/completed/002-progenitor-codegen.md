# Exec plan 002 — Progenitor codegen from openapi.json

**Goal:** generate Rust types for the explorer API at build time so schema drift is a compile error, not a runtime parse failure.

**Status:** ✅ shipped — generated module compiles as a CI gate. Replacing the hand-written types at use sites is a deferred follow-up (see "What's not done" below).

## What landed

### `build.rs`
- Reads `docs/generated/openapi.json`
- Three preprocessing passes the explorer's spec needs before `openapiv3 = 2` will parse it:
  1. **OpenAPI version downgrade.** Rewrites `"openapi": "3.1.0"` → `"openapi": "3.0.3"`.
  2. **Nullable type-array downgrade.** Rewrites every `"type": ["X", "null"]` → `"type": "X", "nullable": true`. Recursive over the whole document.
  3. **Nullable `oneOf` collapse.** Rewrites every `oneOf: [{type: null}, X]` → `X + nullable: true`. progenitor's typify backend doesn't handle `{type: null}` as a `oneOf` member.
- **OperationId rewrite.** The explorer reuses generic IDs like `list` and `leaderboard` across paths; progenitor requires unique IDs. Every operation gets a path-derived ID: `<method>_<sanitized_path>` (e.g. `get_payouts_leaderboard`, `get_rewards_leaderboard`, `get_orchestrators_address_delegators`).
- Calls `progenitor::Generator::default().generate_tokens(&spec)`, parses with `syn`, formats with `prettyplease`, writes to `$OUT_DIR/explorer_generated.rs`.

### Module

`src/domains/explorer/generated.rs` `include!`s the generated file under `#![allow(...)]` so it actually compiles. That gates two things:

1. The OpenAPI spec is still consumable after upstream changes (build.rs panics if not).
2. The generated Rust is itself valid (rustc compiles it).

### Runtime dep

`progenitor-client = "0.9"` is in `[dependencies]` so the generated code's `use progenitor_client::{...}` resolves. The crate is tiny — just re-exports of `reqwest` types and a handful of helpers.

## What's not done

The hand-written types in `src/domains/explorer/types.rs` still drive the runtime — they're imported by every consumer (client, embed builders, schedulers, command handlers). Replacing them with re-exports from `domains::explorer::generated::types` is mechanical but touches a lot of files. The snapshot tests (006) lock the embed output, so swapping the underlying types should be safe; the risk is in field-name or visibility differences that surface only at compile time.

Tracked as **006-or-later** follow-up: "swap explorer::types::* to re-exports from explorer::generated::types::*; delete hand-written struct bodies."

## Build cost

Clean build added ~25 s for the progenitor toolchain (typify, schemars, syn-via-prettyplease) and ~5 s for compiling the 9.6 k-line generated file. Incremental rebuilds are unaffected — `rerun-if-changed=docs/generated/openapi.json` keeps the build cache hot.

## How to refresh the spec

```sh
curl -fsSL https://livepeer-network-api.cloudspe.com/openapi.json \
  -o docs/generated/openapi.json
cargo check
```

If `cargo check` fails inside `build.rs`, the explorer added a spec feature our preprocessing doesn't handle yet — fix `build.rs`, not the spec.
