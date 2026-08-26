-- Drop the machine scope. Machine-scoped rows cannot be represented once the
-- column is gone, so they are deleted rather than reattributed: filing them
-- against an arbitrary application on the machine is exactly the wrong
-- attribution this grain exists to prevent.

DELETE FROM scoped_check_policies WHERE machine_id IS NOT NULL;
DELETE FROM issues WHERE machine_id IS NOT NULL;

DROP INDEX scoped_check_policies_global;
DROP INDEX scoped_check_policies_machine;
ALTER TABLE scoped_check_policies DROP CONSTRAINT scoped_check_policies_check;
ALTER TABLE scoped_check_policies DROP COLUMN machine_id;
ALTER TABLE scoped_check_policies ADD CONSTRAINT scoped_check_policies_check CHECK (
	application_id IS NULL OR server_group_id IS NULL
);
CREATE UNIQUE INDEX scoped_check_policies_global
	ON scoped_check_policies (source, check_name)
	WHERE application_id IS NULL AND server_group_id IS NULL;

DROP INDEX issues_global_source_ref;
DROP INDEX issues_machine_last_seen;
DROP INDEX issues_machine_source_ref;
ALTER TABLE issues DROP CONSTRAINT issues_scope_at_most_one;
ALTER TABLE issues DROP COLUMN machine_id;
ALTER TABLE issues ADD CONSTRAINT issues_scope_at_most_one CHECK (
	application_id IS NULL OR server_group_id IS NULL
);
CREATE UNIQUE INDEX issues_global_source_ref
	ON issues (source, "ref")
	WHERE application_id IS NULL AND server_group_id IS NULL;
