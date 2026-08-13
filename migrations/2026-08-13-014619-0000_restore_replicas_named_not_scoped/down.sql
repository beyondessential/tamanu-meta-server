ALTER TABLE backup_restore_checks
	DROP COLUMN replica_name;

-- The scope indexes only ever refused a declaration, so nothing downstream
-- needs them back. Reinstate them where the data still allows it, and leave
-- them off where an operator has since declared two replicas of one scope —
-- which is exactly what the up migration set out to permit. Dropping one of
-- their declarations to force the index back on would lose an operator's work
-- to a rollback.
DO $$
BEGIN
	IF NOT EXISTS (
		SELECT 1 FROM restore_replicas
		 WHERE server_id IS NOT NULL
		 GROUP BY consumer_device_id, group_id, type, intent, server_id
		HAVING count(*) > 1
	) THEN
		CREATE UNIQUE INDEX restore_replicas_scope_server
			ON restore_replicas (consumer_device_id, group_id, type, intent, server_id)
			WHERE server_id IS NOT NULL;
	ELSE
		RAISE NOTICE 'restore_replicas_scope_server left off: a scope has several declarations';
	END IF;

	IF NOT EXISTS (
		SELECT 1 FROM restore_replicas
		 WHERE server_id IS NULL
		 GROUP BY consumer_device_id, group_id, type, intent
		HAVING count(*) > 1
	) THEN
		CREATE UNIQUE INDEX restore_replicas_scope_group
			ON restore_replicas (consumer_device_id, group_id, type, intent)
			WHERE server_id IS NULL;
	ELSE
		RAISE NOTICE 'restore_replicas_scope_group left off: a scope has several declarations';
	END IF;
END
$$;
