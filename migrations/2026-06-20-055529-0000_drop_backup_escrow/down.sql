ALTER TABLE server_group_backup_config
	DROP CONSTRAINT server_group_backup_config_status_check,
	ADD CONSTRAINT server_group_backup_config_status_check
		CHECK (status IN ('provisioning', 'escrow_pending', 'ready')),
	ADD COLUMN escrow_acked_at TIMESTAMPTZ,
	ADD COLUMN escrow_acked_by TEXT;
