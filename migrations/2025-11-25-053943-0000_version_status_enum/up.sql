DROP VIEW IF EXISTS version_updates;

ALTER TABLE versions
	ADD COLUMN status TEXT;

UPDATE versions
	SET status = CASE
		WHEN published = true THEN 'published'
		ELSE 'yanked'
	END;

ALTER TABLE versions
	ALTER COLUMN status SET NOT NULL,
	ALTER COLUMN status SET DEFAULT 'draft';

ALTER TABLE versions
	ADD CONSTRAINT versions_status_check CHECK (status IN ('draft', 'published', 'yanked'));

ALTER TABLE versions
	DROP COLUMN published;

CREATE OR REPLACE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
)
SELECT id, major, minor, patch, status, changelog
FROM ranked_versions
WHERE rn = 1;
