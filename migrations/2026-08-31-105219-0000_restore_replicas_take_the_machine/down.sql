DROP INDEX migration_tests_application;
ALTER TABLE migration_tests DROP COLUMN application_id;

-- Back to naming an application. A machine's replica becomes one of the
-- applications on it; where a machine runs several, which one it lands on is
-- arbitrary, which is the ambiguity the forward migration exists to remove.
DROP INDEX backup_restore_checks_machine_type;

ALTER TABLE restore_replicas DROP CONSTRAINT restore_replicas_machine_id_fkey;
ALTER TABLE backup_restore_checks DROP CONSTRAINT backup_restore_checks_machine_id_fkey;

UPDATE restore_replicas r
SET machine_id = (
	SELECT a.id FROM applications a
	WHERE a.machine_id = r.machine_id
	ORDER BY a.created_at
	LIMIT 1
)
WHERE r.machine_id IS NOT NULL;

UPDATE backup_restore_checks c
SET machine_id = (
	SELECT a.id FROM applications a
	WHERE a.machine_id = c.machine_id
	ORDER BY a.created_at
	LIMIT 1
)
WHERE c.machine_id IS NOT NULL;

-- A machine with no application on it leaves nothing to point at. The column
-- is nullable on both tables, so these read as "whole group" and "the server
-- has gone" respectively rather than needing deletion.
ALTER TABLE restore_replicas RENAME COLUMN machine_id TO server_id;
ALTER TABLE backup_restore_checks RENAME COLUMN machine_id TO server_id;

ALTER TABLE restore_replicas
	ADD CONSTRAINT restore_replicas_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id);
ALTER TABLE backup_restore_checks
	ADD CONSTRAINT backup_restore_checks_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id);

CREATE INDEX backup_restore_checks_server_type
	ON backup_restore_checks (server_id, type, observed_at DESC);
