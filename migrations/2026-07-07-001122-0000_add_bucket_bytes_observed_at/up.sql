-- Give the S3-metrics collector its own timestamp on backup_repo_stats, so
-- `observed_at` can belong solely to the repo-inspection writer. Backfill from
-- `observed_at` where a bucket figure exists: the collector runs daily and
-- bumped `observed_at` until now, so it's within a day of the real measurement.
ALTER TABLE backup_repo_stats ADD COLUMN bucket_bytes_observed_at TIMESTAMPTZ;
UPDATE backup_repo_stats SET bucket_bytes_observed_at = observed_at WHERE bucket_bytes IS NOT NULL;
