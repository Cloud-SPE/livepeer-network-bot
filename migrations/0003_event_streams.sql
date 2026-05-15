-- 0003_event_streams.sql
--
-- New event streams for the subscriptions feature (004b / 004c):
--
--   reward_events       — BondingManager.Reward, persisted with sent watermark
--   delegator_events    — Bond / Unbond / Rebond, persisted with sent flag
--   delegator_history   — first-seen marker per (delegator, orch); used to
--                         differentiate "new delegator" vs "stake change" on
--                         subsequent Bond events
--
-- Per the explorer's indexer (verified in backfill.rs):
--   Reward.transcoder           -> event.to_address  (LPT amount in amount_native)
--   Bond.delegator              -> event.from_address
--   Bond.newDelegate (orch)     -> event.to_address  (additionalAmount, not total)
--   Unbond.delegator            -> event.from_address
--   Unbond.delegate             -> event.to_address  (LPT amount unstaked)
--   Rebond.delegator            -> event.from_address
--   Rebond.delegate             -> event.to_address  (LPT amount restaked)

CREATE TABLE IF NOT EXISTS reward_events (
    id                     TEXT PRIMARY KEY,
    chain_id               TEXT NOT NULL,
    tx_hash                TEXT NOT NULL,
    log_index              INTEGER NOT NULL,
    block_number           TEXT NOT NULL,
    block_timestamp        TEXT NOT NULL,
    orch_address           TEXT NOT NULL,
    amount_native          TEXT,
    amount_usd             TEXT,
    native_usd_price       TEXT,
    fetched_at             TEXT NOT NULL,
    sent_to_subscribers_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_reward_events_orch
    ON reward_events(orch_address, block_timestamp);

CREATE INDEX IF NOT EXISTS idx_reward_events_pending
    ON reward_events(sent_to_subscribers_at, block_timestamp)
    WHERE sent_to_subscribers_at IS NULL;

CREATE TABLE IF NOT EXISTS delegator_events (
    id                  TEXT PRIMARY KEY,
    event_name          TEXT NOT NULL CHECK (event_name IN ('Bond', 'Unbond', 'Rebond')),
    chain_id            TEXT NOT NULL,
    tx_hash             TEXT NOT NULL,
    log_index           INTEGER NOT NULL,
    block_number        TEXT NOT NULL,
    block_timestamp     TEXT NOT NULL,
    delegator_address   TEXT NOT NULL,
    orch_address        TEXT NOT NULL,
    amount_native       TEXT,
    amount_usd          TEXT,
    fetched_at          TEXT NOT NULL,
    sent_to_subscribers INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_delegator_events_orch
    ON delegator_events(orch_address, block_timestamp);

CREATE INDEX IF NOT EXISTS idx_delegator_events_pending
    ON delegator_events(sent_to_subscribers, block_timestamp);

CREATE TABLE IF NOT EXISTS delegator_history (
    delegator_address TEXT NOT NULL,
    orch_address      TEXT NOT NULL,
    first_seen_at     TEXT NOT NULL,
    PRIMARY KEY (delegator_address, orch_address)
);
