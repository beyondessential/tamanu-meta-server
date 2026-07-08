-- Retire the separate `health-broken/<check>` issue thread: brokenness
-- now lives on the check's own `health/<check>` state row (sticky —
-- retaining the previous definite result's contribution while broken).
-- The companion code change no longer files the separate ref; this
-- migration cleans up the rows that already exist, mirroring the
-- overall-health rollup retirement.

-- 1. Deactivate and resolve the broken-thread issues.
UPDATE issues
SET active = false,
	resolved_at = NOW(),
	resolved_by = 'migration:2026-07-08-merge_broken_thread',
	resolved_reason = 'expected',
	last_seen = NOW(),
	updated_at = NOW()
WHERE ref LIKE 'health-broken/%' AND active = true;

-- 2. Mark incident links as left for these issues, so the orphan-close
--    in step 3 sees an accurate "remaining contributors" count.
UPDATE incident_issues
SET left_at = NOW()
WHERE left_at IS NULL
	AND issue_id IN (
		SELECT id FROM issues
		WHERE ref LIKE 'health-broken/%'
			AND resolved_by = 'migration:2026-07-08-merge_broken_thread'
	);

-- 3. Close incidents this migration just orphaned. No Slack resolve —
--    this is a code-retirement cleanup, not an operational recovery.
UPDATE incidents
SET closed_at = NOW(), updated_at = NOW()
WHERE closed_at IS NULL
	AND EXISTS (
		SELECT 1 FROM incident_issues ii
		JOIN issues i ON i.id = ii.issue_id
		WHERE ii.incident_id = incidents.id
			AND i.ref LIKE 'health-broken/%'
			AND i.resolved_by = 'migration:2026-07-08-merge_broken_thread'
	)
	AND NOT EXISTS (
		SELECT 1 FROM incident_issues ii
		WHERE ii.incident_id = incidents.id
			AND ii.left_at IS NULL
	);

-- 4. Cancel pending Slack opens for incidents just closed.
UPDATE slack_outbox
SET gave_up_at = NOW(),
	last_error = 'cancelled: incident closed by broken-thread merge migration'
WHERE gave_up_at IS NULL
	AND delivered_at IS NULL
	AND kind = 'incident_open'
	AND incident_id IN (
		SELECT id FROM incidents
		WHERE closed_at IS NOT NULL
			AND EXISTS (
				SELECT 1 FROM incident_issues ii
				JOIN issues i ON i.id = ii.issue_id
				WHERE ii.incident_id = incidents.id
					AND i.ref LIKE 'health-broken/%'
					AND i.resolved_by = 'migration:2026-07-08-merge_broken_thread'
			)
	);

-- 5. Fold broken-thread silences into the check's own silence: a check
--    silenced only on its broken thread becomes silenced outright (the
--    two are one thread now), then the old rows go away.
INSERT INTO server_silenced_refs (server_id, source, ref, created_at, created_by)
SELECT server_id, source, 'health/' || substring(ref from 15), created_at, created_by
FROM server_silenced_refs
WHERE ref LIKE 'health-broken/%'
ON CONFLICT DO NOTHING;
DELETE FROM server_silenced_refs WHERE ref LIKE 'health-broken/%';

INSERT INTO server_group_silenced_refs (server_group_id, source, ref, created_at, created_by)
SELECT server_group_id, source, 'health/' || substring(ref from 15), created_at, created_by
FROM server_group_silenced_refs
WHERE ref LIKE 'health-broken/%'
ON CONFLICT DO NOTHING;
DELETE FROM server_group_silenced_refs WHERE ref LIKE 'health-broken/%';
