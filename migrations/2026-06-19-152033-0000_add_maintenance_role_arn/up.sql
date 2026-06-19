-- The maintenance role the backups pod assumes for maintenance/inspection/
-- s3-metrics. Distinct from target_role_arn (the device role): the device role
-- deliberately has no delete, so the maintenance path (which prunes) must use
-- this fuller role (s3:* + delete + CloudWatch). There are no existing config
-- rows, so NOT NULL with no default is clean.
ALTER TABLE server_group_backup_config
	ADD COLUMN maintenance_role_arn TEXT NOT NULL;
