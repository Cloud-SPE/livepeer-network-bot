# Exec plan 007 — Types swap + structural architecture gate

**Goal:** complete the work started in 002 by actually consuming the progenitor-generated types at every use site, and mechanically enforce the domain-stratification rules from `core-beliefs.md` so future contributors can't quietly break the layering.

**Status:** ✅ shipped.

## Part A — Generated types are now load-bearing

`src/domains/explorer/types.rs` shrunk from ~170 lines of hand-written struct definitions to a 50-line shim that:

1. Re-exports the relevant subset of `super::generated::types::*` so import paths across the codebase stay stable.
2. Defines the bot-internal `Cadence` enum (not in the OpenAPI spec).
3. Defines `GatewayProfileRowExt` — an extension trait providing `is_ai()` on the generated `GatewayProfileRow`. Inherent impls on generated code aren't an option because the type lives in a sibling module, so an extension trait is the cleanest way to add bot-specific methods.

### Two consumer fixes the swap forced

- `parse_fee_cut` in `scheduler/digest_poster.rs` — generated `fee_cut_percent` is `String` (required), so the `.as_deref().and_then(parse)` chain became just `.parse::<f64>()`.
- All `gateway.is_ai()` call sites — pull in `GatewayProfileRowExt`. No call-site changes beyond the import.

### Test fixture updates

The snapshot tests (`tests/embeds.rs`) build `OrchestratorProfileRow` and `GatewayProfileRow` inline. Several fields the explorer marks required in 3.1 are required in generated Rust too (`total_stake`, `fee_cut_percent`, `is_active`, `kind`, `latest_deposit`, …) — the fixtures now provide values instead of `None`. The expected JSON literals are unchanged — proof the swap is a pure refactor.

## Part B — Architecture gate

`tests/architecture.rs` has two tests, both lightweight string scans (comments stripped) that walk `src/domains/<leaf>/**/*.rs`:

| Test | What it asserts |
|---|---|
| `strict_leaf_domains_do_not_cross_import` | Nothing under `src/domains/explorer/` or `src/domains/subscriptions/` contains `crate::domains::<other>` |
| `state_only_imports_explorer_types` | `src/domains/state/` may reference `crate::domains::explorer::types` but not `::client` or `::generated`, and no other domain at all |

Substring matching is imperfect (string literals containing the path would false-positive) but the strings we look for (`crate::domains::X`) are vanishingly unlikely to appear in any production string. Good enough for v1.

### One file moved as a consequence

`src/domains/subscriptions/seed.rs` pulled from `explorer`, `state`, AND `subscriptions` — three domains for what is a clear cross-domain composition. Moved to `src/seed.rs` (sibling of `runtime.rs` and `config.rs`). All callers updated: `runtime::run` and `commands::subscribe`.

The seed module's doc comment now points future readers at this rule.

### Updated `core-beliefs.md`

The old "domains never import each other" wording oversold the rule (the real code already had legitimate cross-imports in `notify` and the composers). The new wording stratifies domains into strict-leaves / persistence / formatters / composers and is honest about which tiers can depend on which.

## What's not done

- `notify` and the composer domains aren't structurally checked. A future tightening could enforce "formatters import only `*::types` modules from leaves" but matching that with substring scans gets brittle. A proper syn-based AST walk would catch more.
- The progenitor codegen still keeps a parallel hand-written hierarchy via re-exports. If we wanted full elimination, we'd `pub use ... as EventRow` etc. directly from `client.rs` and delete `types.rs` entirely. The current shim is the cleanest cost/benefit point.
