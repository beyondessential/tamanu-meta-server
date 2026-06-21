ALTER TABLE server_group_backup_config
	DROP CONSTRAINT server_group_backup_config_mode_check,
	ADD CONSTRAINT server_group_backup_config_mode_check
		CHECK (mode IN ('from_birth', 'import'));
