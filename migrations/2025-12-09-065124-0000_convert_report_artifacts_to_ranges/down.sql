-- Revert the conversion: convert report artifacts back to exact versions
-- This finds all ranged report artifacts and converts them back to exact versions

-- For range patterns matching X.Y.x, find the corresponding version with patch=0
UPDATE artifacts
SET 
  version_id = v.id,
  version_range_pattern = NULL
FROM versions v
WHERE 
  artifacts.version_range_pattern IS NOT NULL
  AND artifacts.artifact_type LIKE 'report-%'
  AND v.patch = 0
  AND artifacts.version_range_pattern = v.major || '.' || v.minor || '.x';

