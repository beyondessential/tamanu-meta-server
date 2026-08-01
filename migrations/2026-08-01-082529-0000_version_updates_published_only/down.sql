CREATE OR REPLACE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
)
SELECT id, major, minor, patch, status, changelog
FROM ranked_versions
WHERE rn = 1;
