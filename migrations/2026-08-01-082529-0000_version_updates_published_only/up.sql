-- Rank only published versions when reducing each minor line to one row.
--
-- The view exists to answer "what can this version update to", and reduced
-- each (major, minor) to its single highest-patch row with no status filter.
-- Callers then filter on status, which is too late: if the newest patch in a
-- line is draft or yanked, that row is the only one the view exposes for the
-- line, so the filter drops the whole minor rather than falling back to its
-- newest published patch. With 2.46.2 published and 2.46.3 draft, nothing in
-- the 2.46 line was offered as an update at all — even though the public
-- update endpoint, which filters before it reduces, offers 2.46.2.
CREATE OR REPLACE VIEW version_updates AS
WITH ranked_versions AS (
	SELECT *, ROW_NUMBER() OVER (PARTITION BY major, minor ORDER BY patch DESC) as rn
	FROM versions
	WHERE status = 'published'
)
SELECT id, major, minor, patch, status, changelog
FROM ranked_versions
WHERE rn = 1;
