-- Fill in versions the first backfill couldn't see.
--
-- server_reported_detail was seeded from each (server, source) pair's *latest*
-- push. A source whose latest push carried no version — the agent reporting
-- while the application is down or mid-upgrade — therefore has a NULL version,
-- even though it reported one shortly before.
--
-- That was harmless while the column only fed the fleet view. It isn't now
-- that a group's headline version reads from here: the read it replaces looked
-- back to the last version-*bearing* status, so leaving these NULL would blank
-- those labels until the next versioned push. Ingest keeps the version sticky
-- from here on; this is the one-off catch-up.
--
-- Bounded to 90 days, matching the lookback of the read this replaces, and for
-- the same reason: statuses is partitioned by week and an unbounded lookback
-- can't be pruned to a slice of it.
UPDATE server_reported_detail AS detail
SET version = last_versioned.version
FROM (
	SELECT DISTINCT ON (server_id, source) server_id, source, version
	FROM statuses
	WHERE created_at >= NOW() - INTERVAL '90 days'
		AND version IS NOT NULL
		AND id != '00000000-0000-0000-0000-000000000000'
		AND source != 'canopy'
	ORDER BY server_id, source, created_at DESC
) AS last_versioned
WHERE detail.server_id = last_versioned.server_id
	AND detail.source = last_versioned.source
	AND detail.version IS NULL;
