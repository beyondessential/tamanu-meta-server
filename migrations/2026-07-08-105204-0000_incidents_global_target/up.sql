-- Incidents generalise to per-target: a server group, or canopy as a
-- whole (server_group_id NULL). Canopy-wide issues (self-alerts) get the
-- full incident lifecycle instead of their bespoke direct-Slack path.
ALTER TABLE incidents ALTER COLUMN server_group_id DROP NOT NULL;

-- At most one open canopy-wide incident, mirroring the per-group rule.
CREATE UNIQUE INDEX incidents_open_global ON incidents ((1))
	WHERE server_group_id IS NULL AND closed_at IS NULL;
