-- Destructive revert: groups, group-level notes/tags, and the group-keyed
-- incidents are dropped. Server-level notes/tags are also dropped. Existing
-- incidents are reattached to an arbitrary server in their group
-- (preferring kind = 'central') so the FK survives the revert; if a group
-- has no servers (shouldn't happen), the incident is deleted.

ALTER TABLE incidents ADD COLUMN server_id UUID REFERENCES servers (id) ON DELETE CASCADE ON UPDATE CASCADE;

UPDATE incidents
SET server_id = picked.server_id
FROM (
	SELECT DISTINCT ON (s.group_id) s.group_id, s.id AS server_id
	FROM servers s
	WHERE s.group_id IS NOT NULL
	ORDER BY s.group_id, CASE WHEN s.kind = 'central' THEN 0 ELSE 1 END, s.name NULLS LAST, s.id
) picked
WHERE incidents.server_group_id = picked.group_id;

DELETE FROM incidents WHERE server_id IS NULL;

ALTER TABLE incidents ALTER COLUMN server_id SET NOT NULL;
DROP INDEX IF EXISTS incidents_open_by_group;
DROP INDEX IF EXISTS incidents_group_opened;
ALTER TABLE incidents DROP COLUMN server_group_id;
CREATE INDEX incidents_server_opened ON incidents (server_id, opened_at DESC);
CREATE UNIQUE INDEX incidents_open_by_server ON incidents (server_id) WHERE closed_at IS NULL;

ALTER TABLE servers ADD COLUMN parent_server_id UUID REFERENCES servers (id);
CREATE INDEX servers_parent_server_id ON servers (parent_server_id);

DROP INDEX IF EXISTS servers_group_id;
ALTER TABLE servers
	DROP COLUMN tags,
	DROP COLUMN notes,
	DROP COLUMN group_id;

DROP TABLE server_groups;
