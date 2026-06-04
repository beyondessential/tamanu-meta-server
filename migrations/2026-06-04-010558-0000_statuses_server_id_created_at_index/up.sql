-- Replace the single-column server_id btree with a composite
-- (server_id, created_at DESC), mirroring what device_connections got in
-- 2026-05-07. Lets latest-status-per-server lookups do an index-driven
-- Limit and stop early, instead of fetching every matching row across
-- all partitions and sorting in memory. The composite still serves
-- server_id-only equality lookups (leading column), so the prior
-- single-column index is redundant.

CREATE INDEX statuses_server_id_created_at
    ON statuses USING btree (server_id, created_at DESC);

DROP INDEX statuses_server_id;
