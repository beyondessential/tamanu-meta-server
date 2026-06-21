-- Escrow is removed: Canopy owns + regularly rotates every repo passphrase, so
-- there is no operator escrow/ack step. Drop the escrow_acked_* columns and the
-- `escrow_pending` status (from_birth now goes provisioning → ready directly).
-- No existing rows, so no data migration.
ALTER TABLE server_group_backup_config
	DROP COLUMN escrow_acked_at,
	DROP COLUMN escrow_acked_by,
	DROP CONSTRAINT server_group_backup_config_status_check,
	ADD CONSTRAINT server_group_backup_config_status_check
		CHECK (status IN ('provisioning', 'ready'));
