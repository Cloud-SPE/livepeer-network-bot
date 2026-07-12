-- 0007_reward_watch.sql
--
-- Per-(round, orchestrator) state for the reward-call watcher. A row is
-- created the first time an orchestrator is alerted (or resolved/missed) in
-- a round; keying on the round number makes the "reset each round" semantics
-- automatic. `alerts_sent` counts ladder DMs so restarts and missed poller
-- ticks never re-send a rung. The one-shot public delinquency digest is
-- tracked in the existing `cursors` table (name `reward_watch_digest`), as is
-- the last round fully processed for missed-reward DMs
-- (`reward_watch_missed_done`).

CREATE TABLE IF NOT EXISTS reward_watch_state (
    round              INTEGER NOT NULL,
    orch_address       TEXT NOT NULL,
    alerts_sent        INTEGER NOT NULL DEFAULT 0,
    resolved_at        TEXT,
    missed_notified_at TEXT,
    updated_at         TEXT NOT NULL,
    PRIMARY KEY (round, orch_address)
);
