-- Replace the single-column device_id btree with a composite
-- (device_id, created_at DESC). Lets queries that fetch the latest N
-- connections for a device do an index-driven Limit and stop early, instead of
-- fetching every matching row across all partitions and sorting in memory.
-- The composite still serves device_id-only equality lookups (leading column),
-- so the prior single-column index is redundant.

CREATE INDEX device_connections_device_created_at
    ON device_connections USING btree (device_id, created_at DESC);

DROP INDEX device_connections_device;
