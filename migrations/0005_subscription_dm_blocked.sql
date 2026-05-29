-- 0005_subscription_dm_blocked.sql
--
-- Previously, a subscription whose owner could not be DM'd (Discord 403 —
-- DMs disabled, no mutual guild, or blocked bot) was DELETED after
-- DM_FAILURE_AUTO_UNSUB consecutive failures, silently discarding the user's
-- intent. Instead we now keep the row and raise this flag, surfacing the
-- blocked state in `/subscriptions` so the user can fix their DM settings.
-- The flag is cleared automatically on the next successful DM.
ALTER TABLE subscriptions
    ADD COLUMN dm_blocked INTEGER NOT NULL DEFAULT 0;
