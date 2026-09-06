-- An incident targets one of a group's environments: the group's applications
-- at one rank. A group's own checks, and members of a group with no ranked
-- application, target the group itself, which is rank IS NULL.
ALTER TABLE incidents
	ADD COLUMN rank TEXT,
	ADD CONSTRAINT incidents_rank_needs_group
		CHECK (rank IS NULL OR server_group_id IS NOT NULL),
	ADD CONSTRAINT incidents_rank_check
		CHECK (rank IS NULL OR rank IN ('production', 'clone', 'demo', 'test', 'dev'));

-- One open incident per group, and one per environment of it.
DROP INDEX incidents_open_by_group;
CREATE UNIQUE INDEX incidents_open_by_group ON incidents (server_group_id)
	WHERE closed_at IS NULL AND server_group_id IS NOT NULL AND rank IS NULL;
CREATE UNIQUE INDEX incidents_open_by_environment ON incidents (server_group_id, rank)
	WHERE closed_at IS NULL AND server_group_id IS NOT NULL AND rank IS NOT NULL;

CREATE INDEX incidents_environment_opened ON incidents (server_group_id, rank, opened_at DESC);
