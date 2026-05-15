# Exec plan 001 — Bootstrap

**Goal:** stand up the repository scaffolding so subsequent work can fill in business logic against a stable structure.

**Status:** ✅ scaffold complete; business logic stubbed where noted.

## Done

- [x] `git init`, `.gitignore`, `rust-toolchain.toml`
- [x] `Cargo.toml` with pinned deps (`tokio`, `reqwest`, `sqlx`, `serde`, `tracing`, `chrono`, `anyhow`, `thiserror`, `dotenvy`, `url`)
- [x] `AGENTS.md` (map only, ~80 lines)
- [x] `docs/design-docs/{index,architecture,core-beliefs}.md`
- [x] `docs/product-specs/messages.md` (embed templates verbatim from backend-rs)
- [x] `docs/generated/openapi.json` (vendored from live deploy)
- [x] `src/` module skeleton with trait boundaries and provider wiring
- [x] `migrations/0001_initial.sql` — `events`, `cursors`, `summary_watermarks` tables
- [x] `ops/explorer-name-overrides.sql` — operator SQL template
- [x] `.env.example` and `.github/workflows/ci.yml`

## Open follow-ups — closed-out as their own plans

- [x] **Digest poster grouping + 24h-rollup query** — shipped as part of 004a/004b
- [x] **`progenitor` codegen** — shipped in [002](../completed/002-progenitor-codegen.md), consumers swapped in [007](../completed/007-types-swap-and-architecture-gate.md)
- [x] **`tests/architecture.rs` structural cross-domain test** — shipped in [007](../completed/007-types-swap-and-architecture-gate.md)
- [x] **`tests/openapi_drift.rs`** — superseded by the `openapi-drift` job in `.github/workflows/ci.yml` (CI step diffs the live spec against `docs/generated/openapi.json`)
- [ ] **Gateway / orchestrator profile in-memory TTL cache** — not yet shipped. Each digest run fetches profiles fresh; with subscriber DM fan-out this can multiply request volume. Worth a future micro-plan if volumes grow.

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --help   # binary should print usage
```
