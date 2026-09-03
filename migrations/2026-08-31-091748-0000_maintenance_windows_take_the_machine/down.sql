-- Back to a window over one application. A machine's window becomes a window
-- over an application on it; where a machine runs several, the others lose
-- their cover, there being no single application a machine's window is about.
DROP INDEX maintenance_windows_machine;
DROP INDEX maintenance_windows_one_open_per_machine;

ALTER TABLE maintenance_windows
	DROP CONSTRAINT maintenance_windows_one_target;
ALTER TABLE maintenance_windows
	DROP CONSTRAINT maintenance_windows_machine_id_fkey;

UPDATE maintenance_windows w
SET machine_id = (
	SELECT a.id FROM applications a
	WHERE a.machine_id = w.machine_id
	ORDER BY a.created_at
	LIMIT 1
)
WHERE w.machine_id IS NOT NULL;

-- A machine with no application on it leaves nothing to point the window at.
DELETE FROM maintenance_windows WHERE machine_id IS NULL AND server_group_id IS NULL;

ALTER TABLE maintenance_windows RENAME COLUMN machine_id TO server_id;

ALTER TABLE maintenance_windows
	ADD CONSTRAINT maintenance_windows_server_id_fkey
	FOREIGN KEY (server_id) REFERENCES applications (id) ON DELETE CASCADE;

ALTER TABLE maintenance_windows
	ADD CONSTRAINT maintenance_windows_one_target
	CHECK (num_nonnulls(server_id, server_group_id) = 1);

CREATE UNIQUE INDEX maintenance_windows_one_open_per_server
	ON maintenance_windows (server_id)
	WHERE ended_at IS NULL AND server_id IS NOT NULL;
CREATE INDEX maintenance_windows_server
	ON maintenance_windows (server_id, declared_at DESC);
