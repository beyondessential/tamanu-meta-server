ALTER TABLE backup_runs
	DROP COLUMN s3_sent_raw_bytes,
	DROP COLUMN s3_sent_payload_bytes,
	DROP COLUMN s3_received_raw_bytes,
	DROP COLUMN s3_received_payload_bytes;
