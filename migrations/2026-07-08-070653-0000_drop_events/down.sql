-- Recreates the table shape only; the audit history is not recoverable.
CREATE TABLE events (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	occurred_at TIMESTAMP WITH TIME ZONE,
	issue_id UUID NOT NULL REFERENCES issues (id) ON DELETE CASCADE ON UPDATE CASCADE,
	severity TEXT NOT NULL,
	description TEXT,
	message TEXT NOT NULL,
	active BOOLEAN NOT NULL,
	hash BYTEA NOT NULL,
	occurrences INTEGER NOT NULL DEFAULT 1,
	last_seen TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX events_issue_created ON events (issue_id, created_at DESC);
