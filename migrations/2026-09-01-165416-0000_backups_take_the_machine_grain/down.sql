-- Back to the application grain. Lossless in practice: every machine carried
-- exactly one application until the forward migration ran. A box carrying two
-- workloads has no single application to attribute its backups to, so its rows
-- resolve to whichever sorts first — the arbitrary attribution the forward
-- migration exists to remove.
ALTER TABLE machine_backup_capabilities DROP CONSTRAINT machine_backup_capabilities_pkey;
ALTER TABLE machine_backup_capabilities DROP CONSTRAINT machine_backup_capabilities_machine_id_fkey;
UPDATE machine_backup_capabilities c SET machine_id = (
	SELECT a.id FROM applications a WHERE a.machine_id = c.machine_id ORDER BY a.id LIMIT 1
);
DELETE FROM machine_backup_capabilities WHERE machine_id IS NULL;
ALTER TABLE machine_backup_capabilities RENAME COLUMN machine_id TO server_id;
ALTER TABLE machine_backup_capabilities ADD CONSTRAINT server_backup_capabilities_pkey
	PRIMARY KEY (server_id, type);
ALTER TABLE machine_backup_capabilities ADD CONSTRAINT server_backup_capabilities_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id) ON DELETE CASCADE;
ALTER TABLE machine_backup_capabilities RENAME TO server_backup_capabilities;

ALTER TABLE backup_requests DROP CONSTRAINT backup_requests_pkey;
ALTER TABLE backup_requests DROP CONSTRAINT backup_requests_machine_id_fkey;
UPDATE backup_requests r SET machine_id = (
	SELECT a.id FROM applications a WHERE a.machine_id = r.machine_id ORDER BY a.id LIMIT 1
);
DELETE FROM backup_requests WHERE machine_id IS NULL;
ALTER TABLE backup_requests RENAME COLUMN machine_id TO server_id;
ALTER TABLE backup_requests ADD CONSTRAINT backup_requests_pkey
	PRIMARY KEY (server_id, type, purpose);
ALTER TABLE backup_requests ADD CONSTRAINT backup_requests_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id) ON DELETE CASCADE;

ALTER TABLE backup_repo_snapshots DROP CONSTRAINT backup_repo_snapshots_machine_id_fkey;
UPDATE backup_repo_snapshots s SET machine_id = (
	SELECT a.id FROM applications a WHERE a.machine_id = s.machine_id ORDER BY a.id LIMIT 1
);
ALTER TABLE backup_repo_snapshots RENAME COLUMN machine_id TO server_id;
ALTER TABLE backup_repo_snapshots ADD CONSTRAINT backup_repo_snapshots_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id) ON DELETE SET NULL;

ALTER TABLE backup_run_progress DROP CONSTRAINT backup_run_progress_machine_id_fkey;
UPDATE backup_run_progress p SET machine_id = (
	SELECT a.id FROM applications a WHERE a.machine_id = p.machine_id ORDER BY a.id LIMIT 1
);
ALTER TABLE backup_run_progress RENAME COLUMN machine_id TO server_id;
ALTER TABLE backup_run_progress ADD CONSTRAINT backup_run_progress_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id) ON DELETE SET NULL;

ALTER TABLE backup_runs DROP CONSTRAINT backup_runs_machine_id_fkey;
UPDATE backup_runs r SET machine_id = (
	SELECT a.id FROM applications a WHERE a.machine_id = r.machine_id ORDER BY a.id LIMIT 1
);
DROP INDEX backup_runs_machine_id_type_reported_at_idx;
ALTER TABLE backup_runs RENAME COLUMN machine_id TO server_id;
ALTER TABLE backup_runs ADD CONSTRAINT backup_runs_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id) ON DELETE SET NULL;
CREATE INDEX backup_runs_server_id_type_reported_at_idx
	ON backup_runs (server_id, type, reported_at DESC);
