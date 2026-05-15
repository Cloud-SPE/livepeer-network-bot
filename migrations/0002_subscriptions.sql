-- 0002_subscriptions.sql
--
-- v1 of the subscriptions schema. Only the subscriptions table itself —
-- event-stream tables (reward_events, delegator_events, delegator_history)
-- arrive in 0003 alongside the 004b pollers.
--
-- Discord user IDs and orchestrator addresses are both stored as TEXT.
-- Discord IDs are snowflakes (u64) but we keep them as strings to avoid the
-- u64-vs-i64 friction and because we never do arithmetic on them.
-- Orchestrator addresses are 0x-prefixed lowercase hex.

CREATE TABLE IF NOT EXISTS subscriptions (
    discord_user_id      TEXT NOT NULL,
    orchestrator_address TEXT NOT NULL,
    created_at           TEXT NOT NULL,
    dm_failure_count     INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (discord_user_id, orchestrator_address)
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_orch
    ON subscriptions(orchestrator_address);
