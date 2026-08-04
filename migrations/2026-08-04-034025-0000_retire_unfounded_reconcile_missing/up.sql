-- backup-reconcile-missing said a server "reported a successful backup but no
-- matching repo snapshot landed". It never looked a snapshot up: it compared
-- the repo inventory's age against twice the backup interval, and the inventory
-- can only ever be as fresh as the last inspection, which runs on a slower,
-- independent cadence. For any server backing up more than twice a day — the
-- normal case — that comparison was false whenever nobody had looked recently,
-- and the guard meant to catch that only asked whether an inspection had
-- happened in the last eight days, not whether one had happened since the run
-- it was contradicting. So the check fired on healthy servers.
--
-- The name now belongs to the finding it always claimed: the snapshot id a run
-- reported is absent from the snapshots inspection observed. That is decided
-- per run and per inspection, so it cannot re-raise these rows on its own, and
-- until each group is next inspected it has nothing to say about them at all.
-- Retire them rather than leave a fleet of unfounded warnings standing against
-- server health. Any that are real come back on the next inspection.
CREATE TEMP TABLE unfounded_reconcile_missing ON COMMIT DROP AS
SELECT id FROM issues
WHERE source = 'canopy'
	AND ref = 'backup-reconcile-missing'
	AND active;

UPDATE incident_issues
SET left_at = now()
WHERE left_at IS NULL
	AND issue_id IN (SELECT id FROM unfounded_reconcile_missing);

UPDATE issues
SET
	active = false,
	resolved_at = now(),
	resolved_by = 'migration',
	resolved_reason = 'raised by a comparison that could not establish a snapshot was missing'
WHERE id IN (SELECT id FROM unfounded_reconcile_missing);

-- Close any incident those releases emptied. Mirrors the leave path in
-- re_evaluate_incident_membership: an incident is held open by its
-- currently-failing contributors, so one with none left retires. No Slack
-- resolve is enqueued — these incidents should not have been opened.
UPDATE incidents AS inc
SET closed_at = now()
WHERE inc.closed_at IS NULL
	AND EXISTS (
		SELECT 1 FROM incident_issues il
		WHERE il.incident_id = inc.id
			AND il.issue_id IN (SELECT id FROM unfounded_reconcile_missing)
	)
	AND NOT EXISTS (
		SELECT 1
		FROM incident_issues il
		JOIN issues i ON i.id = il.issue_id
		WHERE il.incident_id = inc.id
			AND il.left_at IS NULL
			AND i.effective_result = 'failed'
	);
