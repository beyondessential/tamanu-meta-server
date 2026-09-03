-- A maintenance window is declared over a machine, not over one workload on it.
--
-- Taking a box down to patch it stops everything running on it, so a window
-- naming one application left the others on the same host monitored and
-- alerting through work that was always going to stop them. Naming the machine
-- makes that one declaration with N consequences rather than N declarations.
--
-- Every window predating the split was over a server that is now an
-- application, and every such application sits on exactly one machine, so the
-- backfill is that join.
ALTER TABLE maintenance_windows RENAME COLUMN server_id TO machine_id;

ALTER TABLE maintenance_windows
	DROP CONSTRAINT maintenance_windows_server_id_fkey;

UPDATE maintenance_windows w
SET machine_id = a.machine_id
FROM applications a
WHERE w.machine_id = a.id;

-- No arm for a window whose application has gone: the key it arrived under
-- cascades on delete, so every non-null `server_id` named a live application,
-- and `applications.machine_id` is NOT NULL. The join above therefore lands
-- every window on a real machine.

ALTER TABLE maintenance_windows
	ADD CONSTRAINT maintenance_windows_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE CASCADE;

ALTER TABLE maintenance_windows
	DROP CONSTRAINT maintenance_windows_one_target;
ALTER TABLE maintenance_windows
	ADD CONSTRAINT maintenance_windows_one_target
	CHECK (num_nonnulls(machine_id, server_group_id) = 1);

DROP INDEX maintenance_windows_one_open_per_server;
CREATE UNIQUE INDEX maintenance_windows_one_open_per_machine
	ON maintenance_windows (machine_id)
	WHERE ended_at IS NULL AND machine_id IS NOT NULL;

DROP INDEX maintenance_windows_server;
CREATE INDEX maintenance_windows_machine
	ON maintenance_windows (machine_id, declared_at DESC);
