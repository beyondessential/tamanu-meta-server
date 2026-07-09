CREATE TABLE server_silenced_refs (
	server_id UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE ON UPDATE CASCADE,
	source TEXT NOT NULL,
	ref TEXT NOT NULL,
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	created_by TEXT,
	PRIMARY KEY (server_id, source, ref)
);

CREATE TABLE server_group_silenced_refs (
	server_group_id UUID NOT NULL REFERENCES server_groups (id) ON DELETE CASCADE ON UPDATE CASCADE,
	source TEXT NOT NULL,
	ref TEXT NOT NULL,
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	created_by TEXT,
	PRIMARY KEY (server_group_id, source, ref)
);

-- Silences (skipped-ceiling scoped policies) go back to ref rows; other
-- scoped transforms have no downgrade representation and are dropped.
INSERT INTO server_silenced_refs (server_id, source, ref, created_at, created_by)
SELECT
	server_id, source,
	CASE WHEN source IN ('canopy', 'manual') THEN check_name ELSE 'health/' || check_name END,
	created_at, created_by
FROM scoped_check_policies
WHERE server_id IS NOT NULL AND ceiling = 'skipped'
ON CONFLICT DO NOTHING;

INSERT INTO server_group_silenced_refs (server_group_id, source, ref, created_at, created_by)
SELECT
	server_group_id, source,
	CASE WHEN source IN ('canopy', 'manual') THEN check_name ELSE 'health/' || check_name END,
	created_at, created_by
FROM scoped_check_policies
WHERE server_group_id IS NOT NULL AND ceiling = 'skipped'
ON CONFLICT DO NOTHING;

DROP TABLE scoped_check_policies;
