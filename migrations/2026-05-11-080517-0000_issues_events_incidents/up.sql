-- PagerDuty/Sentry-shaped issue tracking: long-lived issues grouped under
-- incidents (a server-group rollup) with an append-only event log per issue.
-- See docs/plans/issues-events-incidents.md for the full design.

CREATE TABLE incidents (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	server_id UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE ON UPDATE CASCADE,
	opened_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	closed_at TIMESTAMP WITH TIME ZONE,
	-- Human ack and resolution; orthogonal to closed_at (which is auto).
	acknowledged_at TIMESTAMP WITH TIME ZONE,
	acknowledged_by TEXT,
	resolved_at TIMESTAMP WITH TIME ZONE,
	resolved_by TEXT,
	-- Reason enum validated at the API layer; see commons-types::issue::ResolvedReason.
	resolved_reason TEXT
);

SELECT diesel_manage_updated_at('incidents');

CREATE INDEX incidents_server_opened ON incidents (server_id, opened_at DESC);
-- "Any open incident for this group?" — partial index lets the lookup hit only open rows.
CREATE INDEX incidents_open_by_server ON incidents (server_id) WHERE closed_at IS NULL;

CREATE TABLE issues (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	server_id UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE ON UPDATE CASCADE,
	-- NULL for operator-submitted (source = 'manual') issues.
	device_id UUID REFERENCES devices (id) ON DELETE SET NULL ON UPDATE CASCADE,
	source TEXT NOT NULL,
	"ref" TEXT NOT NULL,
	-- Severity follows RFC 5424; validated as an enum at the API layer.
	severity TEXT NOT NULL DEFAULT 'error',
	description TEXT,
	message TEXT NOT NULL,
	active BOOLEAN NOT NULL,
	first_seen TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	last_seen TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	-- Human ack and resolution. Cleared on device reopen for resolved_*.
	acknowledged_at TIMESTAMP WITH TIME ZONE,
	acknowledged_by TEXT,
	resolved_at TIMESTAMP WITH TIME ZONE,
	resolved_by TEXT,
	resolved_reason TEXT,
	-- Silence-until (snooze) for flappy issues. While in the future, the issue
	-- can't open or join incidents. Cleared once expired (lazy: queries treat
	-- past values as if unset).
	snoozed_until TIMESTAMP WITH TIME ZONE,
	UNIQUE (server_id, source, "ref")
);

SELECT diesel_manage_updated_at('issues');

CREATE INDEX issues_server_last_seen ON issues (server_id, last_seen DESC);
CREATE INDEX issues_device ON issues (device_id) WHERE device_id IS NOT NULL;

CREATE TABLE events (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	-- Server-side receive time.
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	-- Client-supplied "when the thing happened" (optional).
	occurred_at TIMESTAMP WITH TIME ZONE,
	issue_id UUID NOT NULL REFERENCES issues (id) ON DELETE CASCADE ON UPDATE CASCADE,
	severity TEXT NOT NULL,
	description TEXT,
	message TEXT NOT NULL,
	active BOOLEAN NOT NULL,
	-- SHA-256 over (severity, active, message, description_or_empty); 32 bytes.
	hash BYTEA NOT NULL,
	occurrences INTEGER NOT NULL DEFAULT 1,
	-- Latest effective time across coalesced pushes into this row.
	last_seen TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX events_issue_created ON events (issue_id, created_at DESC);

CREATE TABLE incident_issues (
	incident_id UUID NOT NULL REFERENCES incidents (id) ON DELETE CASCADE ON UPDATE CASCADE,
	issue_id UUID NOT NULL REFERENCES issues (id) ON DELETE CASCADE ON UPDATE CASCADE,
	joined_at TIMESTAMP WITH TIME ZONE NOT NULL,
	left_at TIMESTAMP WITH TIME ZONE,
	PRIMARY KEY (incident_id, issue_id, joined_at)
);

CREATE INDEX incident_issues_open_by_issue ON incident_issues (issue_id) WHERE left_at IS NULL;
CREATE INDEX incident_issues_open_by_incident ON incident_issues (incident_id) WHERE left_at IS NULL;
