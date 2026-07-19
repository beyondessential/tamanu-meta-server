-- Scoped check policy: a result transform scoped to one target — a
-- server, a server group, or canopy-wide — applied after the fleet
-- catalog (fleet, then group, then server; each acting on the previous
-- effective result). Either side of the transform may be present: a
-- ceiling, a rules ladder, or both.
--
-- The operator-facing silence is a scoped ceiling of 'skipped': the
-- check keeps recording its observed results, but its effective result
-- is skipped so it raises nothing and counts nowhere. Arbitrary scoped
-- transforms are admitted by the model; the UI only offers silences.
CREATE TABLE scoped_check_policies (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	source TEXT NOT NULL,
	check_name TEXT NOT NULL,
	-- The scope: exactly one of server/group, or neither for canopy-wide.
	server_id UUID REFERENCES servers (id) ON DELETE CASCADE,
	server_group_id UUID REFERENCES server_groups (id) ON DELETE CASCADE,
	ceiling TEXT,
	rules JSONB,
	created_by TEXT,
	CHECK (server_id IS NULL OR server_group_id IS NULL),
	CHECK (ceiling IS NOT NULL OR rules IS NOT NULL)
);

-- One transform per (scope, source, check).
CREATE UNIQUE INDEX scoped_check_policies_server
	ON scoped_check_policies (server_id, source, check_name)
	WHERE server_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_group
	ON scoped_check_policies (server_group_id, source, check_name)
	WHERE server_group_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_global
	ON scoped_check_policies (source, check_name)
	WHERE server_id IS NULL AND server_group_id IS NULL;

-- Silences were (source, ref) rows in two sibling tables; they become
-- skipped-ceiling scoped policies keyed by check name (the ref minus
-- the health/ namespace prefix for source-reported checks).
INSERT INTO scoped_check_policies
	(source, check_name, server_id, ceiling, created_by, created_at, updated_at)
SELECT
	source,
	CASE WHEN ref LIKE 'health/%' THEN substring(ref from 8) ELSE ref END,
	server_id, 'skipped', created_by, created_at, created_at
FROM server_silenced_refs
ON CONFLICT DO NOTHING;

INSERT INTO scoped_check_policies
	(source, check_name, server_group_id, ceiling, created_by, created_at, updated_at)
SELECT
	source,
	CASE WHEN ref LIKE 'health/%' THEN substring(ref from 8) ELSE ref END,
	server_group_id, 'skipped', created_by, created_at, created_at
FROM server_group_silenced_refs
ON CONFLICT DO NOTHING;

DROP TABLE server_silenced_refs;
DROP TABLE server_group_silenced_refs;
