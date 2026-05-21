-- Reverse the range-based schema back to the single-version_id model.
-- Issues whose min_* doesn't match any versions row are dropped; this
-- is a best-effort rollback and a no-op for the typical workflow where
-- min_* came from a versions row in the first place.

ALTER TABLE version_known_issues
	ADD COLUMN version_id UUID REFERENCES versions (id) ON DELETE CASCADE ON UPDATE CASCADE;

UPDATE version_known_issues k
SET version_id = v.id
FROM versions v
WHERE k.min_major = v.major
  AND k.min_minor = v.minor
  AND k.min_patch = v.patch;

DELETE FROM version_known_issues WHERE version_id IS NULL;

ALTER TABLE version_known_issues
	ALTER COLUMN version_id SET NOT NULL,
	DROP CONSTRAINT max_consistency,
	DROP CONSTRAINT max_same_minor,
	DROP CONSTRAINT max_above_min,
	DROP CONSTRAINT resolved_consistency,
	ADD CONSTRAINT resolved_consistency CHECK (
		(resolved_at IS NULL AND resolved_by IS NULL AND resolution_message IS NULL)
		OR (resolved_at IS NOT NULL AND resolved_by IS NOT NULL AND resolution_message IS NOT NULL)
	),
	DROP COLUMN min_major,
	DROP COLUMN min_minor,
	DROP COLUMN min_patch,
	DROP COLUMN max_major,
	DROP COLUMN max_minor,
	DROP COLUMN max_patch;

DROP INDEX IF EXISTS version_known_issues_min;
DROP INDEX IF EXISTS version_known_issues_open_minor;

CREATE INDEX version_known_issues_version_created
	ON version_known_issues (version_id, created_at DESC);

CREATE INDEX version_known_issues_open
	ON version_known_issues (version_id)
	WHERE resolved_at IS NULL;
