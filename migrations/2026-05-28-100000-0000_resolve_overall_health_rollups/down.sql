-- Best-effort revert: undo the issue / incident / outbox flips that the
-- up.sql performed. The synthetic close event (step 1 of up.sql) is
-- removed by matching its hash literal. We can't restore the prior
-- last_seen / updated_at values exactly — they had been overwritten by
-- NOW() during up.sql — but the resolved_* and active flags can be
-- cleared so callers see the issues as live again.

-- Remove the synthetic close events.
DELETE FROM events
WHERE hash = '\x00000000000000000000000000000000000000000000000000000000d1e6acab'::bytea
	AND message = 'Overall-health roll-up retired; per-check issues remain.';

-- Re-activate the rollup issues that we resolved.
UPDATE issues
SET active = true,
	resolved_at = NULL,
	resolved_by = NULL,
	resolved_reason = NULL,
	updated_at = NOW()
WHERE source = 'status'
	AND ref = 'health'
	AND resolved_by = 'migration:2026-05-28-resolve_overall_health_rollups';

-- Rejoin incident_issues links. We don't know the original joined_at,
-- so this is necessarily lossy — left_at is cleared on every link that
-- shares an issue we re-activated, on the assumption nobody else has
-- touched these in the meantime.
UPDATE incident_issues
SET left_at = NULL
WHERE issue_id IN (
		SELECT id FROM issues
		WHERE source = 'status' AND ref = 'health' AND active = true
	);

-- Reopen incidents that were closed solely by this migration. We
-- recognise them by the cancelled outbox row.
UPDATE incidents
SET closed_at = NULL, updated_at = NOW()
WHERE id IN (
		SELECT incident_id FROM slack_outbox
		WHERE kind = 'incident_open'
			AND last_error = 'cancelled: incident closed by overall-health rollup retirement migration'
	);

-- Un-cancel the Slack outbox rows we marked given-up.
UPDATE slack_outbox
SET gave_up_at = NULL, last_error = NULL
WHERE kind = 'incident_open'
	AND last_error = 'cancelled: incident closed by overall-health rollup retirement migration';
