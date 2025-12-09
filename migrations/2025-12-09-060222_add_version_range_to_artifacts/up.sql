-- Add version_range_pattern column to artifacts
-- Allows artifacts to apply to ranges of versions instead of just a single version
-- If set, this takes precedence over version_id for matching purposes
ALTER TABLE artifacts
ADD COLUMN version_range_pattern TEXT;

-- Add constraint: either version_id or version_range_pattern must be set, but not both
ALTER TABLE artifacts
ADD CONSTRAINT version_or_range_pattern CHECK (
  (version_id IS NOT NULL AND version_range_pattern IS NULL) OR
  (version_id IS NULL AND version_range_pattern IS NOT NULL)
);

-- Make version_id nullable (existing artifacts will keep their version_id)
ALTER TABLE artifacts
ALTER COLUMN version_id DROP NOT NULL;
