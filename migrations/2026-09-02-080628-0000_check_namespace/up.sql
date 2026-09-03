-- A check is identified by a namespace and a name.
--
-- `version` is why. A box's version, a Tamanu's version and another product's
-- version are unrelated conditions colliding on one word, and grading them
-- against a single ceiling grades none of them.
--
-- THE TRAP, which this card has already fallen into twice: the namespace is
-- NOT the target a result is filed at. Reachability is one check filed at
-- every target in the fleet — many targets, one identity. Filing scope stays
-- `issues::Scope` and is untouched here.
--
-- Namespacing follows control over names, not subjects. Canopy curates its own
-- names, so `canopy` and `manual` checks are identified by name alone and take
-- a NULL subject. Names arriving over the device API are not curated, so they
-- carry the subject they assert about, and an application's name is further
-- qualified by the type that reported it.
--
-- The qualified form an operator sees, `<type>.<check>`, is presentation. The
-- name is stored on its own; nothing here concatenates.
--
-- See CHK (.workhorse/specs/monitoring/checks.md) and
-- `commons_types::namespace::Namespace`, which is the Rust side of this.

-- ── The namespace columns ───────────────────────────────────────────────────
--
-- Two columns rather than one, because the application case carries a type and
-- the other two do not. The CHECK admits exactly the three shapes, so a row
-- outside them is impossible rather than a case the readers must interpret.

ALTER TABLE check_policies
	ADD COLUMN subject TEXT,
	ADD COLUMN application_type TEXT;

ALTER TABLE check_policies ADD CONSTRAINT check_policies_namespace CHECK (
	(subject IS NULL AND application_type IS NULL)
	OR (subject = 'machine' AND application_type IS NULL)
	OR (subject = 'application' AND application_type IS NOT NULL)
);

ALTER TABLE scoped_check_policies
	ADD COLUMN subject TEXT,
	ADD COLUMN application_type TEXT;

ALTER TABLE scoped_check_policies ADD CONSTRAINT scoped_check_policies_namespace CHECK (
	(subject IS NULL AND application_type IS NULL)
	OR (subject = 'machine' AND application_type IS NULL)
	OR (subject = 'application' AND application_type IS NOT NULL)
);

-- ── check_policies gains a surrogate key ────────────────────────────────────
--
-- Its primary key was (source, check_name), which the namespace widens. A
-- primary key cannot span nullable columns and both namespace columns are
-- nullable by design, so identity moves to a unique index (created at the end,
-- once the fan-out has stopped producing collisions) and the table takes a
-- surrogate id — the shape `scoped_check_policies` has always had.

ALTER TABLE check_policies DROP CONSTRAINT check_policies_pkey;
ALTER TABLE check_policies ADD COLUMN id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE check_policies ADD PRIMARY KEY (id);

-- ── Which names describe the box ────────────────────────────────────────────
--
-- A snapshot of MACHINE_SUBJECT_CHECKS in crates/commons-types/src/subject.rs,
-- taken deliberately: a migration runs once, against the fleet as it was, and
-- freezing the list is what makes it reproducible.
-- `the_migration_snapshot_matches_the_subject_list`, beside that list, holds
-- the two together until this ships — it parses the VALUES below, so keep the
-- quoting as it is.
--
-- Whole names, never prefixes. `caddy_certs` is an application's while the
-- rest of caddy describes the install, and `ips`/`ips_errors` share a prefix
-- and nothing else.

CREATE TABLE machine_subject_checks (check_name TEXT PRIMARY KEY);
INSERT INTO machine_subject_checks (check_name) VALUES
	('billing_tags'),
	('btrfs'),
	('caddy_resolvers'),
	('caddy_version'),
	('caddyfile_version'),
	('canopy_registration'),
	('disk_free'),
	('external_users'),
	('held_captures'),
	('inodes'),
	('ips'),
	('load'),
	('memory'),
	('munin'),
	('tailscale'),
	('tailscale_config'),
	('time_sync'),
	('uptime');

-- ── The catalog fans out ────────────────────────────────────────────────────
--
-- Flat sources keep a NULL subject and are not touched at all.
--
-- A machine-subject name is a re-key, not a fan-out: one box, one entry,
-- however many workloads present the check.

UPDATE check_policies
SET subject = 'machine'
WHERE source NOT IN ('canopy', 'manual')
  AND check_name IN (SELECT check_name FROM machine_subject_checks);

-- An application-subject name becomes one entry per type that has actually
-- reported it. Deriving from `issues` rather than from the live fleet is what
-- keeps a decommissioned entry's namespaces: its states are resolved, not
-- absent.
--
-- WHAT CARRIES AND WHAT DOES NOT. The ceiling, the rules, the notes, the
-- documentation and the review stamp all carry, because the operator's review
-- already applied to exactly the fleet this fan-out covers — and because a
-- pending review hard-caps at warning (CHK), so marking the derived entries
-- unreviewed would silence every vetted check in the fleet at the instant of
-- migration.
--
-- `first_seen` and `last_seen` do NOT carry, and are re-derived per namespace.
-- Inheriting `last_seen` would make a namespace dead for months look
-- reported-today, and 7-day decommissioning would never fire for it.

CREATE TABLE check_namespace_fanout AS
SELECT
	i.source,
	i.check_name,
	a.type AS application_type,
	min(i.first_seen) AS first_seen,
	max(i.last_seen) AS last_seen
FROM issues i
JOIN applications a
	ON a.id = i.application_id
	AND a.id <> '00000000-0000-0000-0000-000000000000'
WHERE i.check_name IS NOT NULL
  AND i.source NOT IN ('canopy', 'manual')
  AND i.check_name NOT IN (SELECT check_name FROM machine_subject_checks)
GROUP BY i.source, i.check_name, a.type;

INSERT INTO check_policies (
	source, check_name, subject, application_type,
	ceiling, escalates, rules, notes,
	first_seen, reviewed_at, reviewed_by, updated_at,
	documentation, last_seen, decommissioned_at, decommissioned_by
)
SELECT
	cp.source, cp.check_name, 'application', f.application_type,
	cp.ceiling, cp.escalates, cp.rules, cp.notes,
	f.first_seen, cp.reviewed_at, cp.reviewed_by, cp.updated_at,
	cp.documentation, f.last_seen, cp.decommissioned_at, cp.decommissioned_by
FROM check_policies cp
JOIN check_namespace_fanout f
	ON f.source = cp.source AND f.check_name = cp.check_name
WHERE cp.subject IS NULL
  AND cp.source NOT IN ('canopy', 'manual');

-- The un-namespaced originals go, including the ones nothing fanned out from.
--
-- An entry with no fan-out is one whose only applications were hard-deleted,
-- cascading the states that would have named their type. There is nothing left
-- to derive a namespace from and no way to guess one, so it is dropped. What
-- the operator graded is genuinely unrecoverable — if the check reports again
-- it will mint a fresh entry, unreviewed and capped at warning, which is the
-- right conservative landing.
--
-- But it is not dropped quietly. A RAISE NOTICE would be: the migrate binary
-- does not surface Postgres notices at any log level, so it would only ever be
-- read by someone running the file by hand. So the loss is filed as a manual
-- issue instead, canopy-wide, naming what went — where an operator is already
-- looking, and where they resolve it once they have decided whether to restate
-- any of it.

DO $$
DECLARE
	lost TEXT[];
BEGIN
	SELECT array_agg(cp.source || '/' || cp.check_name ORDER BY cp.source, cp.check_name)
	INTO lost
	FROM check_policies cp
	WHERE cp.subject IS NULL
	  AND cp.source NOT IN ('canopy', 'manual')
	  AND NOT EXISTS (
		SELECT 1 FROM check_namespace_fanout f
		WHERE f.source = cp.source AND f.check_name = cp.check_name
	  );

	IF lost IS NOT NULL THEN
		RAISE NOTICE 'check namespace: dropped % catalog %: %',
			cardinality(lost),
			CASE WHEN cardinality(lost) = 1 THEN 'entry' ELSE 'entries' END,
			array_to_string(lost, ', ');

		INSERT INTO issues (source, ref, message, description, active)
		VALUES (
			'manual',
			'check-namespace-migration',
			format(
				'%s catalog %s dropped by the check namespace migration',
				cardinality(lost),
				CASE WHEN cardinality(lost) = 1 THEN 'entry was' ELSE 'entries were' END
			),
			format(
				'These checks had no reported application left to name a type, so their namespace could not be derived and their grading could not be carried: %s.'
				|| E'\n\n'
				|| 'Each will reappear as a fresh unreviewed entry if it reports again. Restate any grading that still matters, then resolve this.',
				array_to_string(lost, ', ')
			),
			TRUE
		)
		ON CONFLICT DO NOTHING;
	END IF;
END
$$;

DELETE FROM check_policies
WHERE subject IS NULL
  AND source NOT IN ('canopy', 'manual');

-- ── Identity, now that nothing collides ─────────────────────────────────────
--
-- NULLS NOT DISTINCT is what lets one index cover all three shapes: the flat
-- case is two NULLs and must collide with itself, which the default treatment
-- of NULL would not do.

CREATE UNIQUE INDEX check_policies_identity
	ON check_policies (source, subject, application_type, check_name)
	NULLS NOT DISTINCT;

-- ── Scoped policies fan out too ─────────────────────────────────────────────
--
-- A scoped policy names a check, so it names a namespace. The machine re-key
-- is the same. The application case differs by scope: an application-scoped
-- row already knows its type, while a group-, machine- or fleet-scoped row on
-- an application-subject check covered every type at once and now covers one
-- each.
--
-- The types "present" in a scope are its live applications, not its issue
-- history: a scoped policy governs what is there now. Canopy's own nil
-- application is not one of them: it is where canopy-wide filings land, not a
-- workload a structured source reports for, so fanning a silence out to its
-- type would name a namespace nothing can ever file into.
--
-- The old indexes come off first. They key a scope's transforms by name alone,
-- which is exactly what the fan-out stops being true, so leaving them up would
-- reject the second type of every fanned-out row.

DROP INDEX scoped_check_policies_application;
DROP INDEX scoped_check_policies_machine;
DROP INDEX scoped_check_policies_group;
DROP INDEX scoped_check_policies_global;

UPDATE scoped_check_policies
SET subject = 'machine'
WHERE source NOT IN ('canopy', 'manual')
  AND check_name IN (SELECT check_name FROM machine_subject_checks);

UPDATE scoped_check_policies scp
SET subject = 'application', application_type = a.type
FROM applications a
WHERE a.id = scp.application_id
  AND scp.subject IS NULL
  AND scp.source NOT IN ('canopy', 'manual');

CREATE TABLE scoped_namespace_fanout AS
SELECT scp.id AS policy_id, a.type AS application_type
FROM scoped_check_policies scp
JOIN applications a
	ON a.deleted_at IS NULL
	AND a.id <> '00000000-0000-0000-0000-000000000000'
	AND (
		(scp.server_group_id IS NOT NULL AND a.group_id = scp.server_group_id)
		OR (scp.machine_id IS NOT NULL AND a.machine_id = scp.machine_id)
		OR (scp.server_group_id IS NULL AND scp.machine_id IS NULL AND scp.application_id IS NULL)
	)
WHERE scp.subject IS NULL
  AND scp.application_id IS NULL
  AND scp.source NOT IN ('canopy', 'manual')
GROUP BY scp.id, a.type;

INSERT INTO scoped_check_policies (
	created_at, updated_at, source, check_name,
	application_id, server_group_id, machine_id,
	ceiling, rules, created_by, subject, application_type
)
SELECT
	scp.created_at, scp.updated_at, scp.source, scp.check_name,
	scp.application_id, scp.server_group_id, scp.machine_id,
	scp.ceiling, scp.rules, scp.created_by, 'application', f.application_type
FROM scoped_check_policies scp
JOIN scoped_namespace_fanout f ON f.policy_id = scp.id;

-- A scope with no application of any type has nothing to fan out to, so its
-- row goes with the rest of the un-namespaced originals. Unlike the catalog,
-- nothing is lost an operator cannot restate: a silence over an empty scope
-- silences nothing.

DELETE FROM scoped_check_policies
WHERE subject IS NULL
  AND application_id IS NULL
  AND source NOT IN ('canopy', 'manual');

DROP TABLE scoped_namespace_fanout;
DROP TABLE check_namespace_fanout;
DROP TABLE machine_subject_checks;

-- One transform per (scope, source, namespace, check), so two types silenced
-- separately in one group are two rows rather than a conflict.

CREATE UNIQUE INDEX scoped_check_policies_application
	ON scoped_check_policies (application_id, source, subject, application_type, check_name)
	NULLS NOT DISTINCT
	WHERE application_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_machine
	ON scoped_check_policies (machine_id, source, subject, application_type, check_name)
	NULLS NOT DISTINCT
	WHERE machine_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_group
	ON scoped_check_policies (server_group_id, source, subject, application_type, check_name)
	NULLS NOT DISTINCT
	WHERE server_group_id IS NOT NULL;
CREATE UNIQUE INDEX scoped_check_policies_global
	ON scoped_check_policies (source, subject, application_type, check_name)
	NULLS NOT DISTINCT
	WHERE application_id IS NULL AND machine_id IS NULL AND server_group_id IS NULL;
