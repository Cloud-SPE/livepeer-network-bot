-- ops/explorer-name-overrides.sql
--
-- Run this against the **protocol explorer's** Postgres database (NOT this
-- bot's SQLite). It UPSERTs rows into `name_avatar_overrides`, which the
-- explorer's gateway and orchestrator endpoints consult via:
--
--   COALESCE(o.display_name, e.ens_name)  AS display_name
--   COALESCE(o.avatar_url,   e.ens_avatar_url) AS avatar_url
--
-- The override wins over ENS-derived names. See:
--   livepeer-protocol-explorer/migrations/029_create_name_avatar_overrides.up.sql
--   livepeer-protocol-explorer/crates/livepeer-api/src/routes/profiles.rs
--
-- Address must be lowercase. chain_id 42161 == Arbitrum One.

BEGIN;

INSERT INTO name_avatar_overrides
    (chain_id, address, display_name, avatar_url, notes, updated_by)
VALUES
    (42161, lower('0xREPLACE_ME_GATEWAY_OR_ORCH_ADDRESS'),
     'Friendly Name',
     'https://example.com/avatar.png',
     'manual override for Discord embeds',
     'ops@livepeer'),
    (42161, lower('0xANOTHER_ADDRESS'),
     'Another Friendly Name',
     NULL,
     NULL,
     'ops@livepeer')
ON CONFLICT (chain_id, address) DO UPDATE
SET display_name = EXCLUDED.display_name,
    avatar_url   = EXCLUDED.avatar_url,
    notes        = EXCLUDED.notes,
    updated_at   = now(),
    updated_by   = EXCLUDED.updated_by;

COMMIT;

-- To remove an override:
--   DELETE FROM name_avatar_overrides
--   WHERE chain_id = 42161
--     AND address = lower('0xADDRESS');
