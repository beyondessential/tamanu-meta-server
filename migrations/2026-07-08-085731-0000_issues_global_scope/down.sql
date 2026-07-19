DROP INDEX issues_global_source_ref;

UPDATE issues SET server_id = '00000000-0000-0000-0000-000000000000'
	WHERE server_id IS NULL AND server_group_id IS NULL;

ALTER TABLE issues DROP CONSTRAINT issues_scope_at_most_one;
ALTER TABLE issues ADD CONSTRAINT issues_scope_exactly_one CHECK (
	(server_id IS NULL) <> (server_group_id IS NULL)
);
