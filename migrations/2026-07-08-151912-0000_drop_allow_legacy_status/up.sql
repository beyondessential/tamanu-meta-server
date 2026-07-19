-- Legacy-format pushes are no longer gated: they transform into the
-- tamanu source's always-passing `tasks` heartbeat unconditionally, so
-- the opt-in flag has no job left.
ALTER TABLE servers DROP COLUMN allow_legacy_status;
