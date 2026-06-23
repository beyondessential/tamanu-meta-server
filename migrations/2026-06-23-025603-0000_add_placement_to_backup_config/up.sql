-- Distinguishes a BYO-account config (bucket + roles provisioned externally by
-- ops/pulumi, in the deployment's own account) from a shared-account config
-- (bucket auto-created by canopy in the shared backups account, using shared
-- roles + per-group session-scoped creds). Existing rows are all `external`.
-- Validation is in code via the `BackupPlacement` text enum (no DB CHECK,
-- matching `mode`/`status`).
ALTER TABLE server_group_backup_config
    ADD COLUMN placement TEXT NOT NULL DEFAULT 'external';
