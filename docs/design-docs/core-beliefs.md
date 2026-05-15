# Core beliefs

These are the rules. They are short, mechanical, and enforced — by lints, structural tests, or doc-gardening. When a rule rots, fix it here first, then enforce it in code.

## 1. Parse at the boundary, never YOLO-probe

Every byte that crosses an external boundary (explorer API, env var, Discord response, SQLite row) is parsed into a typed struct before it touches business logic. `serde_json::Value` does not appear past `src/domains/explorer/client.rs`. Env vars are read once in `src/config.rs`; nothing else calls `std::env::var`.

## 2. Domains do not know about each other

`src/domains/explorer/` knows nothing about Discord. `src/domains/notify/` knows nothing about SQLite. `src/domains/scheduler/` is the only place that holds references to multiple domains, and it composes them through trait-bounded generics — never concrete types. Cross-cutting concerns live in `src/providers/`.

## 3. One way to do anything

If you find yourself writing a second helper that does roughly what an existing helper does, delete one. Duplicates rot independently. Prefer a slightly less ergonomic single function to two parallel ones.

## 4. The docs are the system of record

If a fact lives only in a Slack thread, a Google Doc, or your head, it does not exist. Encode it into `docs/` in the same change that uses it. Stale docs are bugs.

## 5. The embed JSON is a contract

The Discord embed shapes in `docs/product-specs/messages.md` are byte-for-byte ports of `livepeer-backend-rs`. They are not "implementation details" — they are observable to users. Format-string changes require a docs update and a justified PR.

## 6. Startup is strict

The binary fails loudly on bad config, missing migrations, or an unreachable explorer at boot. There is no "silent degraded mode." If something is wrong, we want to see it in logs immediately, not at 03:00 when a digest didn't post.

## 7. State changes are append-only and idempotent

A poller crash mid-batch must not lose events or post duplicates. Events are persisted before they are eligible to post; the `sent_to_discord` flag is set only after a successful 2xx from Discord. Summary watermarks are upserted atomically.

## 8. Comments explain *why*

Code names already say *what*. A comment that restates the code is noise. A comment that records a constraint, a workaround, or a non-obvious invariant earns its place.
