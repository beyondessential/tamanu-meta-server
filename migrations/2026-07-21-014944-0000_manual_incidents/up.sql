-- Manual incidents: support-team-recorded incident records, written after
-- the fact rather than derived from check state. Independent of the
-- issues/incidents machinery. See INC spec, "Manual incidents".
CREATE TABLE manual_incidents (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	title TEXT NOT NULL,
	description TEXT NOT NULL DEFAULT '',
	started_at TIMESTAMP WITH TIME ZONE NOT NULL,
	-- NULL while the incident is ongoing.
	ended_at TIMESTAMP WITH TIME ZONE,
	-- The affected group; NULL for fleet/canopy-wide incidents. Kept (not
	-- cascaded) when the group goes away: the record is history.
	server_group_id UUID REFERENCES server_groups (id) ON DELETE SET NULL ON UPDATE CASCADE,
	-- Who recorded it: a tailnet login or an MCP token name.
	created_by TEXT NOT NULL
);

SELECT diesel_manage_updated_at('manual_incidents');

CREATE INDEX manual_incidents_started ON manual_incidents (started_at DESC);
CREATE INDEX manual_incidents_group ON manual_incidents (server_group_id) WHERE server_group_id IS NOT NULL;
