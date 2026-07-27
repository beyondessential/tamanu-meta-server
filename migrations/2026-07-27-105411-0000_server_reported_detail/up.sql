-- Each source's current server-wide detail, one row per (server, source).
--
-- The same facts live in statuses.extra, but that table is partitioned by
-- week and a predicate on server_id alone can't be pruned, so resolving a
-- figure means a bounded scan of recent partitions. That's affordable once
-- per page; it isn't affordable once per server on a fleet-wide view. This
-- table is the current-state projection: ingest keeps it fresh, and reads
-- that only want "what is this server running now" never touch history.
CREATE TABLE server_reported_detail (
	server_id UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
	source TEXT NOT NULL,
	-- The source's whole server-wide detail as last pushed. A push is the
	-- source's current truth, so this replaces rather than merges.
	extra JSONB NOT NULL DEFAULT '{}'::jsonb,
	-- The application version that push reported, so the fleet view spreads
	-- versions from the same read as everything else.
	version TEXT,
	reported_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	PRIMARY KEY (server_id, source)
);

-- Backfill from each (server, source) pair's most recent push in the last
-- 30 days. Deliberately not from all history: that read is the unbounded
-- partition scan this table exists to avoid. A server quiet for longer gets
-- its row back on its next push.
INSERT INTO server_reported_detail (server_id, source, extra, version, reported_at)
SELECT DISTINCT ON (server_id, source)
	server_id,
	source,
	COALESCE(extra, '{}'::jsonb),
	version,
	created_at
FROM statuses
WHERE created_at >= NOW() - INTERVAL '30 days'
	AND id != '00000000-0000-0000-0000-000000000000'
	AND source != 'canopy'
	AND server_id IN (SELECT id FROM servers)
ORDER BY server_id, source, created_at DESC;
