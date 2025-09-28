ALTER TABLE versions ADD COLUMN published BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE versions SET published = false WHERE status != 'published';

DROP VIEW version_updates;
ALTER TABLE versions DROP COLUMN status;

CREATE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
)
SELECT id, major, minor, patch, published, changelog
FROM ranked_versions
WHERE rn = 1;

DROP TYPE version_status;
