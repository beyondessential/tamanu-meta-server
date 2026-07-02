-- The logical size of the snapshot this run produced, as observed by canopy's
-- own repo inspection (kopia `rootEntry.summ.size`), matched to the run by
-- snapshot id. Distinct from `bytes_uploaded` (what the device reported): kept
-- separate so the two can be cross-checked. Written once per run, since a
-- snapshot is immutable.
ALTER TABLE backup_runs ADD COLUMN snapshot_logical_bytes bigint;
