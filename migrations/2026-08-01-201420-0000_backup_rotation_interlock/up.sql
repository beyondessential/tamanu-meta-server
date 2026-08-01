-- Interlock between passphrase rotation and device backups.
--
-- `kopia change-password` rewrites the repository's format blob, so the old
-- passphrase stops working the moment it lands. Devices hold that passphrase:
-- `GET /backup-target` hands it out and the device then runs a backup with
-- credentials good for an hour, so a rotation mid-flight kills a backup that
-- was already running — and the device only finds out when kopia fails.
--
-- Maintenance and inspection are already excluded by the worker's one-op-
-- per-group slot, but that slot is in-process and device backups never touch
-- it. This column is the cross-process half: set for the duration of a
-- rotation, and honoured by the public server's credential and target
-- endpoints, which refuse to hand out a passphrase that is about to change.
--
-- Deliberately timestamped rather than a boolean: a crashed rotation would
-- otherwise leave the flag set and block every backup for the group until the
-- next reconcile. Readers ignore a marker older than the rotation window and
-- log it, so the worst case is self-healing.
ALTER TABLE server_group_backup_config
	ADD COLUMN repo_password_rotating_since TIMESTAMPTZ;

COMMENT ON COLUMN server_group_backup_config.repo_password_rotating_since IS
	'Set while a passphrase rotation is in flight; credential/target issuance is refused meanwhile. NULL = not rotating. A value older than the rotation window is treated as a crashed rotation and ignored.';
