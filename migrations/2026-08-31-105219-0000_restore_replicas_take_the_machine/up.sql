-- A restore replica is declared over a machine, and a restore report is about
-- one.
--
-- What gets restored is a snapshot, and a snapshot is what a machine backed up
-- (see BAK). Naming an application was the only target available before the
-- split: on a box running two workloads it made the choice of which one to
-- name arbitrary, and a whole-group declaration expanded over applications, so
-- a two-workload box got two replicas of the same backup.
--
-- Both columns named an application on exactly one machine, so the backfill is
-- that join. `migration_tests` is deliberately untouched: a candidate version
-- is an application's, which is the one place in this split where the grains
-- genuinely interleave (see RST, "Candidate versions").
ALTER TABLE restore_replicas RENAME COLUMN server_id TO machine_id;
ALTER TABLE backup_restore_checks RENAME COLUMN server_id TO machine_id;

ALTER TABLE restore_replicas DROP CONSTRAINT restore_replicas_server_id_fkey;
ALTER TABLE backup_restore_checks DROP CONSTRAINT backup_restore_checks_server_id_fkey;

UPDATE restore_replicas r
SET machine_id = a.machine_id
FROM applications a
WHERE r.machine_id = a.id;

UPDATE backup_restore_checks c
SET machine_id = a.machine_id
FROM applications a
WHERE c.machine_id = a.id;

-- A whole-group declaration used to expand over applications and now expands
-- over machines, so two declarations that named different workloads on one box
-- now name the same machine. They stay two replicas: a declaration is unique on
-- its name, not on its scope (see the named-not-scoped migration), and each
-- keeps its own reports, its own overdue bound, and its own check instance.
-- No ON DELETE clause on either, which is what these columns arrived with:
-- both targets are archived rather than deleted, so the rule never fires and
-- changing it here would be a behaviour change smuggled into a grain move.
ALTER TABLE restore_replicas
	ADD CONSTRAINT restore_replicas_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id);
ALTER TABLE backup_restore_checks
	ADD CONSTRAINT backup_restore_checks_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id);

DROP INDEX backup_restore_checks_server_type;
CREATE INDEX backup_restore_checks_machine_type
	ON backup_restore_checks (machine_id, type, observed_at DESC);

-- A migration test is the one place the two grains genuinely interleave: the
-- data under test is a machine's snapshot, while the candidate version is an
-- application's. The report above now names the machine, so the application
-- has to be named here or it could not be recovered on a box running two
-- workloads with different candidates.
--
-- Nullable, because tests recorded before the split have no application on
-- record. Where the machine ran exactly one application at the time, that is
-- the one it was for, so backfill those and leave the ambiguous ones null.
ALTER TABLE migration_tests
	ADD COLUMN application_id UUID REFERENCES applications (id) ON DELETE SET NULL;

UPDATE migration_tests t
SET application_id = sole.id
FROM backup_restore_checks c
JOIN LATERAL (
	SELECT a.id FROM applications a
	WHERE a.machine_id = c.machine_id
	LIMIT 2
) sole ON TRUE
WHERE t.check_id = c.id
	AND c.machine_id IS NOT NULL
	AND (SELECT COUNT(*) FROM applications a2 WHERE a2.machine_id = c.machine_id) = 1;

CREATE INDEX migration_tests_application ON migration_tests (application_id);
