-- Data backfill only; the table itself is dropped by its own migration.
-- Reverting deletes nothing: live-recorded rows are indistinguishable by
-- then, and keeping them is harmless.
SELECT 1;
