# Exec plan 003 — Discord rate limiting

**Goal:** replace the naive 500 ms blanket-sleep + 10 s-capped retry with a bucket-aware rate limiter that honors Discord's actual headers.

**Status:** ✅ shipped in `src/providers/discord.rs`. Remaining items are observability / multi-webhook follow-ups.

## What landed

- `DiscordNotifier` impl moved out of `domains/notify/service.rs` into a new provider `providers/discord.rs`. The `Notifier` trait stays in `domains/notify/service.rs` as the port; the new struct is named `DiscordWebhook` (the trait is the "notifier" — the impl is the webhook adapter).
- Bucket state lives in a `Mutex<BucketState>` held across the entire send. All sends through one `DiscordWebhook` are serialized; bucket math is race-free with no extra coordination.
- Each response is parsed for:
  - `x-ratelimit-remaining` (i64)
  - `x-ratelimit-reset-after` (f64 seconds)
  - `x-ratelimit-scope` (`"user"` | `"shared"` | `"global"`)
  - `retry-after` (f64 seconds; floats supported)
- When `remaining <= 0`, the next send blocks on `Instant::now() + reset_after` before firing.
- On 429:
  - Reads the JSON body for `retry_after` and `global`; body value preferred over header when present.
  - Detects global scope from either `x-ratelimit-scope: global` or `body.global == true`.
  - **No cap on the wait.** If Discord asks for 60 s we wait 60 s.
  - Updates the gate and retries, up to `MAX_ATTEMPTS = 3`. After the cap, returns Err and the caller defers to the next polling window (events stay `sent_to_discord=0`, so the same payload retries on the next 15-min tick with whatever extra tickets accumulated — no duplicate posts).
- The previous unconditional 500 ms post-send sleep is gone. Bucket math is the only thing pacing us now.

## Follow-ups landed in this plan

- ✅ **5xx retry.** Bounded retry loop with exponential backoff (base 1 s, cap 30 s, max 3 attempts).
- ✅ **Metrics.** Structured `tracing` events on the `discord_metrics` target with stable message names (`send_completed`, `gate_wait`, `ratelimited`, `server_error_retry`) and stable field names. Scrapers (Vector / Loki / promtail) can derive counters and histograms without us standing up a `/metrics` endpoint. Adding Prometheus directly is deferred until someone actually wants pull-based scraping.

## Remaining limitations (out of scope, documented for future readers)

- **Per-webhook buckets.** Today's `DiscordWebhook` has one mutex; a future multi-webhook deploy should key bucket state by `x-ratelimit-bucket` or URL.
- **Cross-process coordination.** Multiple bot replicas pointed at the same webhook would trip 429s on each other. Out of scope until we run replicas.
- **Invalid Request quota circuit-breaker.** Discord bans the IP at 10 000 4xx/429 in 10 minutes. We currently log + return Err. Add a circuit-breaker if abuse becomes a real risk.

## Where to read

- `src/providers/discord.rs` — the impl
- `src/domains/notify/service.rs` — the `Notifier` trait (the seam)
- `src/runtime.rs` — wires `DiscordWebhook` into `AppState` and shares the `Arc` with both poster tasks
- Discord docs: <https://discord.com/developers/docs/topics/rate-limits>
