-- Group-scoped issues: an issue keyed to a server_group with no member server.
--
-- The incident model is server-keyed today: issues.server_id is NOT NULL and
-- every issue belongs to exactly one server. Group-level backup checks
-- (corruption, preflight, reconcile-missing, restore-verification) must page
-- regardless of any single server's is_monitored gate, so they need a
-- first-class issue that points at a GROUP rather than a server.
--
-- This makes server_id nullable and adds a nullable server_group_id. An issue
-- is exactly one of:
--   * server-scoped: server_id NOT NULL, server_group_id NULL  (every issue so far)
--   * group-scoped:  server_id NULL,     server_group_id NOT NULL
-- enforced by a CHECK. Incidents are already group-keyed, so the existing
-- find_or_open_incident path works unchanged; the only new piece is producing
-- an Issue that resolves its group directly and skips the per-server
-- is_monitored lookup.

ALTER TABLE issues
	ALTER COLUMN server_id DROP NOT NULL,
	ADD COLUMN server_group_id UUID REFERENCES server_groups (id) ON DELETE CASCADE ON UPDATE CASCADE;

-- Exactly-one-of: server-scoped XOR group-scoped.
ALTER TABLE issues
	ADD CONSTRAINT issues_scope_exactly_one CHECK (
		(server_id IS NOT NULL AND server_group_id IS NULL)
		OR (server_id IS NULL AND server_group_id IS NOT NULL)
	);

-- The existing UNIQUE (server_id, source, "ref") still covers server-scoped
-- issues (NULLs are distinct in a UNIQUE, so it no longer constrains
-- group-scoped rows). Add the matching uniqueness for group-scoped issues so
-- raise_group_event's find-or-create keys cleanly on (group, source, ref).
CREATE UNIQUE INDEX issues_group_source_ref
	ON issues (server_group_id, source, "ref")
	WHERE server_group_id IS NOT NULL;

CREATE INDEX issues_group_last_seen
	ON issues (server_group_id, last_seen DESC)
	WHERE server_group_id IS NOT NULL;
