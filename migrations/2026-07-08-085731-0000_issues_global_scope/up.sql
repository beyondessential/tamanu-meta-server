-- Issues gain a third scope: canopy-wide (neither server nor group).
-- Self-alerts previously squatted on the nil "meta" server row; they
-- become true global-scope issues.
ALTER TABLE issues DROP CONSTRAINT issues_scope_exactly_one;
ALTER TABLE issues ADD CONSTRAINT issues_scope_at_most_one CHECK (
	server_id IS NULL OR server_group_id IS NULL
);

UPDATE issues SET server_id = NULL
	WHERE server_id = '00000000-0000-0000-0000-000000000000';

-- Global-scope issues coalesce per (source, ref), mirroring the
-- per-server and per-group unique keys.
CREATE UNIQUE INDEX issues_global_source_ref ON issues (source, ref)
	WHERE server_id IS NULL AND server_group_id IS NULL;
