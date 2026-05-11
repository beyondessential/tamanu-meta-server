-- Operator free-text notes attached to issues and incidents. Immutable
-- once written — only add and delete are supported. (No `updated_at`
-- column since nothing ever updates these rows.)

CREATE TABLE issue_notes (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	issue_id UUID NOT NULL REFERENCES issues (id) ON DELETE CASCADE ON UPDATE CASCADE,
	author TEXT NOT NULL,
	body TEXT NOT NULL
);

CREATE INDEX issue_notes_issue_created ON issue_notes (issue_id, created_at DESC);

CREATE TABLE incident_notes (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	incident_id UUID NOT NULL REFERENCES incidents (id) ON DELETE CASCADE ON UPDATE CASCADE,
	author TEXT NOT NULL,
	body TEXT NOT NULL
);

CREATE INDEX incident_notes_incident_created ON incident_notes (incident_id, created_at DESC);
