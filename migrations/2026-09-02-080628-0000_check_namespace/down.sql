-- Collapsing the namespace back loses information, and there is no arrangement
-- of this file that does not: the fan-out turned one entry into several, each
-- of which an operator could since have graded differently, and the pre-split
-- shape has one place to put them. So the collapse picks a survivor by a stated
-- rule rather than pretending to merge.
--
-- The survivor is the namespace reported most recently, because that is the one
-- whose grading is answering to the fleet as it stands. Its ceiling, rules,
-- notes, documentation, review and decommissioning all carry. `first_seen` and
-- `last_seen` span the whole collapsed set, since the pre-split entry covered
-- all of it.

-- Rolling back does not bring the dropped entries back — they were dropped
-- because nothing named their namespace, and going the other way does not
-- restore that. The issue announcing it goes, though, since it names a
-- migration that is no longer applied.

DELETE FROM issues
WHERE source = 'manual'
  AND ref = 'check-namespace-migration'
  AND application_id IS NULL AND machine_id IS NULL AND server_group_id IS NULL;

-- ── The catalog collapses ───────────────────────────────────────────────────

DROP INDEX check_policies_identity;

CREATE TABLE check_namespace_collapse AS
SELECT DISTINCT ON (source, check_name)
	id AS survivor_id,
	source,
	check_name
FROM check_policies
ORDER BY source, check_name, last_seen DESC NULLS LAST, application_type NULLS FIRST;

UPDATE check_policies cp
SET first_seen = span.first_seen, last_seen = span.last_seen
FROM check_namespace_collapse c
JOIN LATERAL (
	SELECT min(o.first_seen) AS first_seen, max(o.last_seen) AS last_seen
	FROM check_policies o
	WHERE o.source = c.source AND o.check_name = c.check_name
) span ON TRUE
WHERE cp.id = c.survivor_id;

DELETE FROM check_policies cp
WHERE NOT EXISTS (
	SELECT 1 FROM check_namespace_collapse c WHERE c.survivor_id = cp.id
);

DROP TABLE check_namespace_collapse;

ALTER TABLE check_policies DROP CONSTRAINT check_policies_namespace;
ALTER TABLE check_policies DROP COLUMN subject, DROP COLUMN application_type;

ALTER TABLE check_policies DROP CONSTRAINT check_policies_pkey;
ALTER TABLE check_policies DROP COLUMN id;
ALTER TABLE check_policies ADD PRIMARY KEY (source, check_name);

-- ── Scoped policies collapse ────────────────────────────────────────────────
--
-- A scope's several namespaced transforms become the one that was written
-- most recently, on the same reasoning. `id` breaks a tie, so a re-run of
-- down/up/down lands in the same place.

DROP INDEX scoped_check_policies_application;
DROP INDEX scoped_check_policies_machine;
DROP INDEX scoped_check_policies_group;
DROP INDEX scoped_check_policies_global;

CREATE TABLE scoped_namespace_collapse AS
SELECT DISTINCT ON (application_id, machine_id, server_group_id, source, check_name)
	id AS survivor_id
FROM scoped_check_policies
ORDER BY
	application_id, machine_id, server_group_id, source, check_name,
	updated_at DESC, id;

DELETE FROM scoped_check_policies scp
WHERE NOT EXISTS (
	SELECT 1 FROM scoped_namespace_collapse c WHERE c.survivor_id = scp.id
);

DROP TABLE scoped_namespace_collapse;

ALTER TABLE scoped_check_policies DROP CONSTRAINT scoped_check_policies_namespace;
ALTER TABLE scoped_check_policies DROP COLUMN subject, DROP COLUMN application_type;

CREATE UNIQUE INDEX scoped_check_policies_application
	ON scoped_check_policies (application_id, source, check_name)
	WHERE application_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_machine
	ON scoped_check_policies (machine_id, source, check_name)
	WHERE machine_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_group
	ON scoped_check_policies (server_group_id, source, check_name)
	WHERE server_group_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_global
	ON scoped_check_policies (source, check_name)
	WHERE application_id IS NULL AND machine_id IS NULL AND server_group_id IS NULL;
