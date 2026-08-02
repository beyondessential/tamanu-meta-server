-- Whether a declared replica is to be served de-identified. Canopy resolves
-- the masking manifest itself, so this flag is the whole of the operator's
-- say in it.
ALTER TABLE restore_replicas
	ADD COLUMN redacts BOOLEAN NOT NULL DEFAULT FALSE;

-- What the consumer's redaction did, carried alongside the restore health it
-- is reported with. All null for a report from a replica that doesn't redact.
-- `redaction_error` is distinct from `error`: the restore can succeed and the
-- redaction that follows it fail.
ALTER TABLE backup_restore_checks
	ADD COLUMN redaction_outcome TEXT,
	ADD COLUMN redaction_manifest_version TEXT,
	ADD COLUMN redaction_columns_masked BIGINT,
	ADD COLUMN redaction_columns_skipped BIGINT,
	ADD COLUMN redaction_error TEXT;
