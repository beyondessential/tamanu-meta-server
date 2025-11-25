-- Drop the view that uses the status column
DROP VIEW IF EXISTS version_updates;

-- Drop the check constraint if it exists
ALTER TABLE versions
	DROP CONSTRAINT IF EXISTS versions_status_check;

-- Add back the published boolean column (nullable initially)
ALTER TABLE versions
	ADD COLUMN published BOOLEAN;

-- Migrate status back to published: status='published' → true, everything else → false
UPDATE versions
	SET published = CASE
		WHEN status = 'published' THEN true
		ELSE false
	END;

-- Make published NOT NULL with default
ALTER TABLE versions
	ALTER COLUMN published SET NOT NULL,
	ALTER COLUMN published SET DEFAULT true;

-- Drop the status column
ALTER TABLE versions
	DROP COLUMN status;

-- Recreate the view with published column
CREATE OR REPLACE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
)
SELECT id, major, minor, patch, published, changelog
FROM ranked_versions
WHERE rn = 1;
