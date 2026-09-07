-- Back to a window over the box. An application's window has no target in a
-- machine-only model, so it ends now rather than widening to the box and
-- quieting the workloads beside it that nobody declared over.
DROP INDEX maintenance_windows_application;
DROP INDEX maintenance_windows_one_open_per_application;

UPDATE maintenance_windows SET ended_at = NOW(), updated_at = NOW()
	WHERE ended_at IS NULL AND application_id IS NOT NULL;

ALTER TABLE maintenance_windows
	DROP CONSTRAINT maintenance_windows_one_target,
	DROP COLUMN application_id;
ALTER TABLE maintenance_windows
	ADD CONSTRAINT maintenance_windows_one_target
	CHECK (num_nonnulls(machine_id, server_group_id) = 1);
