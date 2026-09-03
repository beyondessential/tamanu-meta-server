-- A machine is a check target, a fourth grain alongside application, group,
-- and canopy-wide.
--
-- `disk_free`, `memory`, `load`, `time_sync`, `tailscale` and friends are not
-- application checks that happen to run on a host; they assert something about
-- the box. Canopy filed them against an application only because an
-- application was the only target available. With a machine grain they file
-- where they belong, and a two-workload host reports its disk once rather than
-- once per workload. See [CHK](.workhorse/specs/monitoring/checks.md).
--
-- This extends machinery that already exists rather than inventing any. Each
-- scope is a nullable FK column, with a CHECK that at most one is set and a
-- partial unique index keying find-or-create for that grain. Storage stays
-- nullable FK columns so Postgres keeps the ON DELETE CASCADE and uniqueness
-- that prevent orphaned check-states.
--
-- THE TRAP. The global-scope partial unique index matches on every *other*
-- scope column being null:
--
--     WHERE application_id IS NULL AND server_group_id IS NULL
--
-- A machine-scoped row has both of those null, so without widening it a
-- machine check would fall inside the global index and collide with a
-- canopy-wide issue on the same (source, ref) — a self-alert and a machine's
-- disk check silently fighting over one row. Both global indexes are therefore
-- recreated with `AND machine_id IS NULL`.
--
-- Whoever adds the next grain has to do the same. That is the hazard to carry
-- forward; it is not a licence to presuppose what the next grain is.

-- ── issues ──────────────────────────────────────────────────────────────────

ALTER TABLE issues
	ADD COLUMN machine_id UUID REFERENCES machines (id) ON DELETE CASCADE ON UPDATE CASCADE;

ALTER TABLE issues DROP CONSTRAINT issues_scope_at_most_one;
ALTER TABLE issues ADD CONSTRAINT issues_scope_at_most_one CHECK (
	(application_id IS NOT NULL)::int
	+ (machine_id IS NOT NULL)::int
	+ (server_group_id IS NOT NULL)::int
	<= 1
);

-- Find-or-create keys per machine, mirroring the per-application and per-group
-- unique keys.
CREATE UNIQUE INDEX issues_machine_source_ref
	ON issues (machine_id, source, "ref")
	WHERE machine_id IS NOT NULL;

CREATE INDEX issues_machine_last_seen
	ON issues (machine_id, last_seen DESC)
	WHERE machine_id IS NOT NULL;

-- Recreated to exclude machine-scoped rows (see THE TRAP above).
DROP INDEX issues_global_source_ref;
CREATE UNIQUE INDEX issues_global_source_ref
	ON issues (source, "ref")
	WHERE application_id IS NULL AND machine_id IS NULL AND server_group_id IS NULL;

-- ── scoped_check_policies ───────────────────────────────────────────────────

ALTER TABLE scoped_check_policies
	ADD COLUMN machine_id UUID REFERENCES machines (id) ON DELETE CASCADE ON UPDATE CASCADE;

ALTER TABLE scoped_check_policies DROP CONSTRAINT scoped_check_policies_check;
ALTER TABLE scoped_check_policies ADD CONSTRAINT scoped_check_policies_check CHECK (
	(application_id IS NOT NULL)::int
	+ (machine_id IS NOT NULL)::int
	+ (server_group_id IS NOT NULL)::int
	<= 1
);

CREATE UNIQUE INDEX scoped_check_policies_machine
	ON scoped_check_policies (machine_id, source, check_name)
	WHERE machine_id IS NOT NULL;

-- Recreated to exclude machine-scoped rows (see THE TRAP above).
DROP INDEX scoped_check_policies_global;
CREATE UNIQUE INDEX scoped_check_policies_global
	ON scoped_check_policies (source, check_name)
	WHERE application_id IS NULL AND machine_id IS NULL AND server_group_id IS NULL;
