-- Initial schema for livepeer-payout-bot.
--
-- Three tables:
--   events              vendored copy of explorer WinningTicketRedeemed rows
--                       plus bot-local sent-to-discord state
--   cursors             named opaque cursors for paginated polling
--   summary_watermarks  one row per (period, period_date) that's been posted,
--                       prevents double-posting daily/weekly/monthly embeds

CREATE TABLE IF NOT EXISTS events (
    id                TEXT PRIMARY KEY,
    chain_id          TEXT NOT NULL,
    tx_hash           TEXT NOT NULL,
    log_index         INTEGER NOT NULL,
    block_number      TEXT NOT NULL,
    block_timestamp   TEXT NOT NULL,
    contract_address  TEXT NOT NULL,
    contract_name     TEXT NOT NULL,
    event_name        TEXT NOT NULL,
    event_signature   TEXT,
    asset             TEXT,
    amount_native     TEXT,
    amount_usd        TEXT,
    native_usd_price  TEXT,
    from_address      TEXT,
    to_address        TEXT,
    finality          TEXT NOT NULL,
    is_canonical      INTEGER NOT NULL,
    fetched_at        TEXT NOT NULL,
    sent_to_discord   INTEGER NOT NULL DEFAULT 0,
    sent_at           TEXT
);

CREATE INDEX IF NOT EXISTS idx_events_to_address_timestamp
    ON events(to_address, block_timestamp);

CREATE INDEX IF NOT EXISTS idx_events_pending
    ON events(sent_to_discord, block_timestamp);

CREATE INDEX IF NOT EXISTS idx_events_event_name
    ON events(event_name, block_timestamp);

CREATE TABLE IF NOT EXISTS cursors (
    name          TEXT PRIMARY KEY,
    cursor_value  TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS summary_watermarks (
    period       TEXT NOT NULL,
    period_date  TEXT NOT NULL,
    posted_at    TEXT NOT NULL,
    PRIMARY KEY (period, period_date)
);
