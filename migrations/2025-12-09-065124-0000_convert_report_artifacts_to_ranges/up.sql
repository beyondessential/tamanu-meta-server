-- Convert report artifacts for patch=0 versions below 2.46.0 to use .x ranges
-- This allows these artifacts to apply to all patch versions within their minor release

-- For each artifact that matches our criteria:
-- 1. Find its version (must have patch=0)
-- 2. Check that version < 2.46.0
-- 3. Check that artifact_type starts with "report-"
-- 4. Convert to range pattern and clear version_id

UPDATE artifacts
SET 
  version_range_pattern = v.major || '.' || v.minor || '.x',
  version_id = NULL
FROM versions v
WHERE 
  artifacts.version_id = v.id
  AND v.patch = 0
  AND (v.major < 2 OR (v.major = 2 AND v.minor < 46))
  AND artifacts.artifact_type LIKE 'report-%';

