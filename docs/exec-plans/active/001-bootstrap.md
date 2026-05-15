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

## Open follow-ups (separate exec-plans)

- [ ] Implement digest poster grouping + 24h-rollup query (`event_poller` and `summary_poster` are real; `digest_poster` is stubbed)
- [ ] Wire `progenitor` codegen — see `002-progenitor-codegen.md`
- [ ] Add `tests/architecture.rs` structural test enforcing cross-domain import rules
- [ ] Add `tests/openapi_drift.rs` that fetches the live spec and compares to committed
- [ ] Add gateway / orchestrator profile in-memory TTL cache

## Verification

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- --help   # binary should print usage
```
