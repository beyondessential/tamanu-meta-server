ALTER TABLE server_group_backup_config
	DROP COLUMN mode,
	DROP COLUMN last_init_error,
	DROP COLUMN escrow_acked_at,
	DROP COLUMN escrow_acked_by;
