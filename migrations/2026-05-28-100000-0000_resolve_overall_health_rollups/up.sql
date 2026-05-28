-- Retire the (status, health) roll-up issue that bestool's top-level
-- `healthy` flag used to drive. The companion code change in
-- crates/public-server/src/statuses.rs no longer files this issue; this
-- migration cleans up the rows that already exist so:
--   1. operators don't see permanently-stuck rollup issues that will
--      never receive another event,
--   2. incidents held open solely by a rollup auto-close,
--   3. the event log shows a clear retirement event rather than just
--      a quiet flip to active=false.
--
-- We use resolved_reason = 'expected' (the documented enum that best
-- fits "this issue category was retired by policy") with a
-- resolved_by attribution that points at this migration.

-- 1. Append a close event to every active rollup issue. The hash is a
--    sentinel 32-byte value rather than a real SHA-256: nothing more
--    will ever file against these issues (the code path is gone) so
--    coalescing semantics no longer matter, and the synthetic hash makes
--    it obvious in event-log inspections that this row came from the
--    migration.
INSERT INTO events (issue_id, severity, active, message, description, hash, occurrences, last_seen)
SELECT
	i.id,
	'info',
	false,
	'Overall-health roll-up retired; per-check issues remain.',
	NULL,
	'\x00000000000000000000000000000000000000000000000000000000d1e6acab'::bytea,
	1,
	NOW()
FROM issues i
WHERE i.source = 'status' AND i.ref = 'health' AND i.active = true;

-- 2. Deactivate and human-resolve the rollup issues themselves.
UPDATE issues
SET active = false,
	resolved_at = NOW(),
	resolved_by = 'migration:2026-05-28-resolve_overall_health_rollups',
	resolved_reason = 'expected',
	last_seen = NOW(),
	updated_at = NOW()
WHERE source = 'status' AND ref = 'health' AND active = true;

-- 3. Mark incident links as left for these issues, so the orphan-close
--    in step 4 sees an accurate "remaining contributors" count.
UPDATE incident_issues
SET left_at = NOW()
WHERE left_at IS NULL
	AND issue_id IN (
		SELECT id FROM issues
		WHERE source = 'status'
			AND ref = 'health'
			AND resolved_by = 'migration:2026-05-28-resolve_overall_health_rollups'
	);

-- 4. Close incidents that this migration just orphaned (i.e. had at
--    least one rollup contributor and no other live contributors). We
--    deliberately don't enqueue a Slack 'incident_resolve' here: this
--    is a code-retirement cleanup, not an operational recovery worth
--    paging about. Cancel any pending Slack opens for the same reason.
UPDATE incidents
SET closed_at = NOW(), updated_at = NOW()
WHERE closed_at IS NULL
	AND EXISTS (
		SELECT 1 FROM incident_issues ii
		JOIN issues i ON i.id = ii.issue_id
		WHERE ii.incident_id = incidents.id
			AND i.source = 'status'
			AND i.ref = 'health'
			AND i.resolved_by = 'migration:2026-05-28-resolve_overall_health_rollups'
	)
	AND NOT EXISTS (
		SELECT 1 FROM incident_issues ii
		WHERE ii.incident_id = incidents.id
			AND ii.left_at IS NULL
	);

-- 5. Cancel any pending Slack 'incident_open' notifications for
--    incidents this migration just closed — operators shouldn't be paged
--    about an incident that the migration silently closed at the same
--    instant. Mirrors `SlackOutbox::cancel_pending_open` (sets
--    gave_up_at + last_error). Leave delivered rows alone.
UPDATE slack_outbox
SET gave_up_at = NOW(),
	last_error = 'cancelled: incident closed by overall-health rollup retirement migration'
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
					AND i.source = 'status'
					AND i.ref = 'health'
					AND i.resolved_by = 'migration:2026-05-28-resolve_overall_health_rollups'
			)
	);
