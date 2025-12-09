-- Revert changes
ALTER TABLE artifacts
DROP CONSTRAINT version_or_range_pattern;

ALTER TABLE artifacts
ALTER COLUMN version_id SET NOT NULL;

ALTER TABLE artifacts
DROP COLUMN version_range_pattern;
