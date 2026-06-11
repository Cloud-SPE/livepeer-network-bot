-- 0006_cut_change_events.sql
--
-- Mirrors TranscoderUpdate rows from /transcoders/{addr}/params/history for
-- subscribed orchestrators. Existing history is seeded as already-sent when
-- an orchestrator is first observed, so subscribers only receive DMs for new
-- cut changes after the bot starts tracking that orchestrator.

CREATE TABLE IF NOT EXISTS cut_change_events (
    event_id               TEXT PRIMARY KEY,
    tx_hash                TEXT NOT NULL,
    log_index              INTEGER NOT NULL,
    block_number           TEXT NOT NULL,
    block_timestamp        TEXT NOT NULL,
    orch_address           TEXT NOT NULL,
    reward_cut_percent     TEXT NOT NULL,
    fee_share_percent      TEXT NOT NULL,
    fee_cut_percent        TEXT NOT NULL,
    fetched_at             TEXT NOT NULL,
    sent_to_subscribers_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_cut_change_events_orch
    ON cut_change_events(orch_address, block_timestamp);

CREATE INDEX IF NOT EXISTS idx_cut_change_events_pending
    ON cut_change_events(sent_to_subscribers_at, block_timestamp)
    WHERE sent_to_subscribers_at IS NULL;
