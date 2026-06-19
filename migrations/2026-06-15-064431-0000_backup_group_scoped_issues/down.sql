DROP INDEX IF EXISTS issues_group_last_seen;
DROP INDEX IF EXISTS issues_group_source_ref;
ALTER TABLE issues DROP CONSTRAINT IF EXISTS issues_scope_exactly_one;
-- Drop group-scoped rows that can't satisfy the restored NOT NULL.
DELETE FROM issues WHERE server_id IS NULL;
ALTER TABLE issues
	DROP COLUMN server_group_id,
	ALTER COLUMN server_id SET NOT NULL;
