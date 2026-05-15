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

## What's still missing

- **Per-webhook buckets.** We have one webhook URL today. If the bot ever fans out to multiple URLs (per-channel routing, per-orchestrator subscriptions), the bucket state should be keyed by `x-ratelimit-bucket` (or by URL as a coarser proxy) instead of one mutex per `DiscordWebhook` instance. Today this is trivial — each instance has its own state — but if we ever share buckets across "shared" routes we'd need a coordinator.
- **Cross-process coordination.** If we ever run multiple bot replicas pointed at the same webhook (HA, blue/green), each replica computes bucket state independently and they'll trip 429s on each other. Out of scope until we actually need replicas.
- **Metrics.** Emit Prometheus counters: `discord_send_total{status}`, `discord_429_total{scope}`, histogram for `wait_for_gate` durations. Owners want to see when the limiter is actually pausing.
- **Invalid Request quota awareness.** Discord bans the IP for 1 hour at 10 000 4xx/429 in 10 minutes. Add a circuit-breaker that pauses all sends if non-429 4xx rate exceeds a threshold. Low risk today but cheap insurance.
- **5xx retry.** Currently any 5xx returns Err immediately. Could add 1-shot retry with backoff. Defer.

## Where to read

- `src/providers/discord.rs` — the impl
- `src/domains/notify/service.rs` — the `Notifier` trait (the seam)
- `src/runtime.rs` — wires `DiscordWebhook` into `AppState` and shares the `Arc` with both poster tasks
- Discord docs: <https://discord.com/developers/docs/topics/rate-limits>
