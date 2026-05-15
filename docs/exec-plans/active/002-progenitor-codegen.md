# Exec plan 002 — Progenitor codegen from openapi.json

**Goal:** replace the hand-written types in `src/domains/explorer/types.rs` with code generated from `docs/generated/openapi.json` so schema changes are caught at compile time, not at the first failed parse.

**Status:** pending. Scaffold currently uses hand-written types; this plan replaces them.

## Approach

1. Add `progenitor` as a `build-dependency` in `Cargo.toml`.
2. Write `build.rs`:
   - Read `docs/generated/openapi.json`
   - Run `progenitor::Generator::default().generate_tokens(&spec)`
   - Emit the output to `$OUT_DIR/explorer_generated.rs`
3. Add `pub mod generated { include!(concat!(env!("OUT_DIR"), "/explorer_generated.rs")); }` inside `src/domains/explorer/mod.rs`.
4. Re-export the few types the rest of the bot uses (`EventRow`, `OrchestratorProfileRow`, `GatewayProfileRow`, `PayoutSummaryResponse`, `PayoutLeaderboardRow`) from `src/domains/explorer/types.rs` as type aliases pointing into `generated::types::*`.
5. Add `scripts/sync-openapi.sh` that re-downloads the live spec into `docs/generated/openapi.json`. Run by hand when the explorer ships a new release.
6. Add `tests/openapi_drift.rs`:
   - Fetch live spec
   - `pretty_assertions::assert_eq!` against the committed spec
   - Fails CI on any drift, forcing an explicit human ack via re-running the sync script

## Risks

- `progenitor` may choke on specific patterns in this OpenAPI spec (e.g. `oneOf`, `anyOf`, polymorphism). If it does, the path forward is to file an issue upstream and pin to a working version, not to fork.
- Build times will go up. If `progenitor` adds more than ~5s, gate codegen behind a feature flag so dev builds stay fast.

## Exit criteria

- [ ] `cargo build` regenerates types from `docs/generated/openapi.json`
- [ ] All hand-written types in `src/domains/explorer/types.rs` are deleted or re-exported from `generated::types`
- [ ] `tests/openapi_drift.rs` exists and passes
- [ ] CI calls `scripts/sync-openapi.sh --check` to verify no drift
