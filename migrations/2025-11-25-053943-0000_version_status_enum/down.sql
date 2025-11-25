DROP VIEW IF EXISTS version_updates;

ALTER TABLE versions
	DROP CONSTRAINT IF EXISTS versions_status_check;

ALTER TABLE versions
	ADD COLUMN published BOOLEAN;

UPDATE versions
	SET published = CASE
		WHEN status = 'published' THEN true
		ELSE false
	END;

ALTER TABLE versions
	ALTER COLUMN published SET NOT NULL,
	ALTER COLUMN published SET DEFAULT true;

ALTER TABLE versions
	DROP COLUMN status;

CREATE OR REPLACE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
)
SELECT id, major, minor, patch, published, changelog
FROM ranked_versions
WHERE rn = 1;
