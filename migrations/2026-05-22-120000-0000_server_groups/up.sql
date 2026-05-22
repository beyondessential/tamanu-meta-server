-- Flatten the server hierarchy into server groups.
--
-- Before: `servers.parent_server_id` formed a tree, and the root server was
-- the unit incidents rolled up to.
--
-- After: a first-class `server_groups` table; each server has a nullable
-- `group_id` pointing to its group. Incidents rekey from `server_id` to
-- `server_group_id`. Both servers and groups gain a `notes` text field and
-- a `tags` jsonb map (string→string, enforced at the API layer).

CREATE TABLE server_groups (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	name TEXT NOT NULL,
	notes TEXT NOT NULL DEFAULT '',
	tags JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(tags) = 'object')
);

SELECT diesel_manage_updated_at('server_groups');

-- New columns on servers. group_id stays nullable: an "ungrouped" server is
-- a real state, surfaced as its own UI tab.
ALTER TABLE servers
	ADD COLUMN group_id UUID REFERENCES server_groups (id) ON DELETE SET NULL ON UPDATE CASCADE,
	ADD COLUMN notes TEXT NOT NULL DEFAULT '',
	ADD COLUMN tags JSONB NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(tags) = 'object');

CREATE INDEX servers_group_id ON servers (group_id) WHERE group_id IS NOT NULL;

-- Data migration: derive groups from the parent_server_id tree.
--
-- Create a group for each root server (parent_server_id IS NULL) that either
-- has at least one descendant or already has any incident attached to it.
-- Standalone roots with no children and no incidents stay ungrouped.
WITH roots_needing_groups AS (
	SELECT DISTINCT s.id, s.name
	FROM servers s
	WHERE s.parent_server_id IS NULL
	  AND s.id <> '00000000-0000-0000-0000-000000000000'::uuid
	  AND (
		EXISTS (SELECT 1 FROM servers c WHERE c.parent_server_id = s.id)
		OR EXISTS (SELECT 1 FROM incidents i WHERE i.server_id = s.id)
	  )
),
inserted AS (
	INSERT INTO server_groups (name)
	SELECT COALESCE(r.name, 'server-' || substr(r.id::text, 1, 8))
	FROM roots_needing_groups r
	RETURNING id, name
),
-- Pair each root's id with the newly-inserted group id by name. Group names
-- come from the root's name (or an id-derived fallback when the root is
-- unnamed); both halves of the join produce the same string in the same row
-- order so the mapping is unambiguous.
root_to_group AS (
	SELECT r.id AS server_id, ig.id AS group_id
	FROM roots_needing_groups r
	JOIN inserted ig
		ON ig.name = COALESCE(r.name, 'server-' || substr(r.id::text, 1, 8))
)
-- Walk the tree from each root, assigning group_id to the root itself and
-- every descendant.
UPDATE servers
SET group_id = chain.group_id
FROM (
	WITH RECURSIVE descendants AS (
		SELECT rtg.server_id AS id, rtg.group_id
		FROM root_to_group rtg
		UNION ALL
		SELECT s.id, d.group_id
		FROM servers s
		JOIN descendants d ON s.parent_server_id = d.id
	)
	SELECT id, group_id FROM descendants
) AS chain
WHERE servers.id = chain.id;

-- The hierarchy column is no longer needed.
DROP INDEX IF EXISTS servers_parent_server_id;
ALTER TABLE servers DROP COLUMN parent_server_id;

-- Rekey incidents from server_id to server_group_id.
ALTER TABLE incidents
	ADD COLUMN server_group_id UUID REFERENCES server_groups (id) ON DELETE CASCADE ON UPDATE CASCADE;

UPDATE incidents
SET server_group_id = s.group_id
FROM servers s
WHERE incidents.server_id = s.id;

-- Defensive: any incident whose server didn't end up with a group_id (would
-- happen if the data migration logic above missed a root). The
-- roots_needing_groups CTE explicitly includes "root with any incident", so
-- this should be a no-op — but better to fail loud than carry a NULL.
DO $$
BEGIN
	IF EXISTS (SELECT 1 FROM incidents WHERE server_group_id IS NULL) THEN
		RAISE EXCEPTION 'incidents.server_group_id is NULL for some rows after migration; data migration is incomplete';
	END IF;
END
$$;

ALTER TABLE incidents ALTER COLUMN server_group_id SET NOT NULL;

DROP INDEX IF EXISTS incidents_open_by_server;
DROP INDEX IF EXISTS incidents_server_opened;
ALTER TABLE incidents DROP COLUMN server_id;

CREATE INDEX incidents_group_opened ON incidents (server_group_id, opened_at DESC);
CREATE UNIQUE INDEX incidents_open_by_group ON incidents (server_group_id) WHERE closed_at IS NULL;
