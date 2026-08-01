ALTER TABLE backup_restore_checks
	DROP COLUMN redaction_outcome,
	DROP COLUMN redaction_manifest_version,
	DROP COLUMN redaction_columns_masked,
	DROP COLUMN redaction_columns_skipped,
	DROP COLUMN redaction_error;

ALTER TABLE restore_replicas
	DROP COLUMN redacts;
