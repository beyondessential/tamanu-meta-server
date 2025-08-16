-- Drop indexes that reference the error field
DROP INDEX IF EXISTS latest_statuses_errors;
DROP INDEX IF EXISTS latest_statuses_successes;

-- Drop the latest_statuses view since it was designed around error/success distinction
DROP VIEW IF EXISTS latest_statuses;

-- Drop the error column from statuses table
ALTER TABLE statuses DROP COLUMN error;
