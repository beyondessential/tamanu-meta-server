-- Drop the view that depends on the published column
DROP VIEW IF EXISTS version_updates;

-- Add the new status column as TEXT (nullable initially to allow migration)
ALTER TABLE versions
	ADD COLUMN status TEXT;

-- Migrate existing data: published=true → status='published', published=false → status='draft'
UPDATE versions
	SET status = CASE
		WHEN published = true THEN 'published'
		ELSE 'draft'
	END;

-- Make status NOT NULL with default
ALTER TABLE versions
	ALTER COLUMN status SET NOT NULL,
	ALTER COLUMN status SET DEFAULT 'draft';

-- Add check constraint to validate status values
ALTER TABLE versions
	ADD CONSTRAINT versions_status_check CHECK (status IN ('draft', 'published', 'yanked'));

-- Drop the old published column
ALTER TABLE versions
	DROP COLUMN published;

-- Recreate the view with status instead of published
CREATE OR REPLACE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
)
SELECT id, major, minor, patch, status, changelog
FROM ranked_versions
WHERE rn = 1;
