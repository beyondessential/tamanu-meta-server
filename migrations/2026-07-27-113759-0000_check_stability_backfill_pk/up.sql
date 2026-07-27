-- The backfill marker shipped without a primary key. Diesel's schema printer
-- refuses any table that has none, and it refuses by aborting the whole
-- regeneration rather than skipping the one table — so `just migrate` has
-- been leaving schema.rs stale since this table landed.
--
-- The marker holds at most one row, written once on completion, so its
-- timestamp is a fine key. schema.rs already declares it as one.
ALTER TABLE check_stability_backfill ADD PRIMARY KEY (done_at);
