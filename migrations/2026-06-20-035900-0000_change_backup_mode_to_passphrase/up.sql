-- Canopy now owns every repo passphrase Secret: drop the import-an-existing-
-- Secret mode in favour of `passphrase` (operator supplies the passphrase,
-- Canopy stores it). No existing rows, so no data migration is needed.
ALTER TABLE server_group_backup_config
	DROP CONSTRAINT server_group_backup_config_mode_check,
	ADD CONSTRAINT server_group_backup_config_mode_check
		CHECK (mode IN ('from_birth', 'passphrase'));
