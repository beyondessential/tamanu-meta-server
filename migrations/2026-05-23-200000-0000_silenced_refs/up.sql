-- Operator-managed silence list for issue refs. A silenced (source, ref)
-- tuple at server or group scope prevents the matching issues from
-- contributing to incidents — they still record (so the audit trail
-- works) but the incident workflow treats them as if they had left.
--
-- Two tables rather than one nullable-FK table so referential integrity
-- and uniqueness are enforced by Postgres without a CHECK gymnastics
-- pattern. The scope is implicit in which table the row lives in.

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

-- Secondary lookups by (source, ref) — when the incident-membership
-- re-evaluation asks "is this (server_id, source, ref) silenced at server
-- scope?", the primary key index is exactly what we need. The same is
-- true for the group-scoped table. So no extra indexes for now.
