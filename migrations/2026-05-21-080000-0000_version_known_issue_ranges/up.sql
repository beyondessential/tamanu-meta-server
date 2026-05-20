-- Known issues used to attach to a single version_id. They now apply
-- to a half-open range [(min_major, min_minor, min_patch), (max_major,
-- max_minor, max_patch)) within a single minor branch. When raised, the
-- max columns are NULL and the issue covers every later patch in that
-- minor. When resolved, the max columns name the fix version (the first
-- unaffected patch).

ALTER TABLE version_known_issues
	ADD COLUMN min_major INT,
	ADD COLUMN min_minor INT,
	ADD COLUMN min_patch INT,
	ADD COLUMN max_major INT,
	ADD COLUMN max_minor INT,
	ADD COLUMN max_patch INT;

-- Backfill min_* from the version this issue used to point at.
UPDATE version_known_issues k
SET min_major = v.major,
	min_minor = v.minor,
	min_patch = v.patch
FROM versions v
WHERE k.version_id = v.id;

-- Legacy resolved rows did not record a fix version. Treat them as
-- "fixed in the next patch" — the closest narrow approximation. The
-- dataset is tiny and operators can re-raise with a more accurate fix
-- version if needed.
UPDATE version_known_issues
SET max_major = min_major,
	max_minor = min_minor,
	max_patch = min_patch + 1
WHERE resolved_at IS NOT NULL;

ALTER TABLE version_known_issues
	ALTER COLUMN min_major SET NOT NULL,
	ALTER COLUMN min_minor SET NOT NULL,
	ALTER COLUMN min_patch SET NOT NULL;

ALTER TABLE version_known_issues DROP COLUMN version_id;

-- Replace the old (version_id, created_at) and partial open indices.
DROP INDEX IF EXISTS version_known_issues_version_created;
DROP INDEX IF EXISTS version_known_issues_open;

CREATE INDEX version_known_issues_min
	ON version_known_issues (min_major, min_minor, min_patch);

CREATE INDEX version_known_issues_open_minor
	ON version_known_issues (min_major, min_minor)
	WHERE max_major IS NULL;

ALTER TABLE version_known_issues
	-- max_* are all-NULL (open) or all-NOT-NULL (resolved).
	ADD CONSTRAINT max_consistency CHECK (
		(max_major IS NULL AND max_minor IS NULL AND max_patch IS NULL)
		OR (max_major IS NOT NULL AND max_minor IS NOT NULL AND max_patch IS NOT NULL)
	),
	-- Resolved metadata co-occurs with max_*.
	DROP CONSTRAINT resolved_consistency,
	ADD CONSTRAINT resolved_consistency CHECK (
		(resolved_at IS NULL AND resolved_by IS NULL AND resolution_message IS NULL AND max_major IS NULL)
		OR (resolved_at IS NOT NULL AND resolved_by IS NOT NULL AND resolution_message IS NOT NULL AND max_major IS NOT NULL)
	),
	-- Max must stay within the same minor as min.
	ADD CONSTRAINT max_same_minor CHECK (
		max_major IS NULL OR (max_major = min_major AND max_minor = min_minor)
	),
	-- Max must be strictly above min.
	ADD CONSTRAINT max_above_min CHECK (
		max_patch IS NULL OR max_patch > min_patch
	);
