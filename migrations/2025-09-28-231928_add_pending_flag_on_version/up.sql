CREATE TYPE version_status AS ENUM (
	'pending',
	'published',
	'yanked'
);

ALTER TABLE versions ADD COLUMN status version_status NOT NULL DEFAULT 'published';
UPDATE versions SET status = 'pending' WHERE NOT published;
ALTER TABLE versions ALTER COLUMN status DROP DEFAULT;

DROP VIEW version_updates;
ALTER TABLE versions DROP COLUMN published;

CREATE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
)
SELECT id, major, minor, patch, status, changelog
FROM ranked_versions
WHERE rn = 1;
