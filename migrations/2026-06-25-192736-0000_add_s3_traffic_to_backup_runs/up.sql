-- S3 traffic tallied by bestool's proxy during a run: raw counts the full HTTP
-- message (incl. SigV4 chunk framing), payload counts the decoded object data.
-- Nullable: older clients omit them; both backup and restore runs may report.
ALTER TABLE backup_runs
	ADD COLUMN s3_sent_raw_bytes         BIGINT,
	ADD COLUMN s3_sent_payload_bytes     BIGINT,
	ADD COLUMN s3_received_raw_bytes     BIGINT,
	ADD COLUMN s3_received_payload_bytes BIGINT;
