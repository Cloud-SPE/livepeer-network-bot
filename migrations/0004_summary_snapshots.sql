-- Last-observed payout-summary values per (period, period_date). Used by the
-- summary poster's readiness gate to detect when a rollup has *stabilized*:
-- the same figures returned across two consecutive polls is the signal that
-- the explorer has finished indexing/enriching that period. Rows are written
-- on every non-ready poll and become moot once the period is watermarked in
-- summary_watermarks (a posted period is never re-fetched).
CREATE TABLE IF NOT EXISTS summary_snapshots (
    period                 TEXT NOT NULL,
    period_date            TEXT NOT NULL,
    ticket_count           TEXT NOT NULL,
    usd_rows_priced        TEXT NOT NULL,
    sum_face_value_native  TEXT NOT NULL,
    sum_commission_native  TEXT NOT NULL,
    observed_at            TEXT NOT NULL,
    PRIMARY KEY (period, period_date)
);
