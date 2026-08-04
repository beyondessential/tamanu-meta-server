-- Backup checks never default to a failure. A failure in canopy means a live
-- service is down and draws a fast human response; a late, unreconciled, or
-- unverified backup is not that, and the fleet's backups are layered enough
-- that one missed run is not an emergency. Shipping these as failures made
-- tech support investigate service outages that were never happening.
--
-- Three keep failing because the backups are already gone, unrecoverable, or
-- unprotected rather than merely late: backup-corruption,
-- backup-rotation-broken, preflight-object-lock.
--
-- CheckPolicy::register only seeds a catalog row on first sight, so changing
-- the shipped defaults in code does nothing to rows that already exist. Reset
-- them here — but only the ones no operator has touched. register() stamps
-- reviewed_by with the source ('canopy'); any other value means an operator
-- set the policy deliberately, and raising a backup check to a failure in
-- consultation with tech support is exactly the call they are entitled to
-- make. Those rows are left alone.
UPDATE check_policies
SET
	ceiling = 'warning',
	escalates = false,
	reviewed_at = now()
WHERE source = 'canopy'
	AND reviewed_by = 'canopy'
	AND ceiling <> 'warning'
	AND (
		check_name LIKE 'backup-%'
		OR check_name LIKE 'preflight-%'
		OR check_name LIKE 'restore-verification%'
		OR check_name LIKE 'redaction%'
		OR check_name LIKE 'migration-test%'
	)
	AND check_name NOT LIKE 'backup-corruption%'
	AND check_name NOT LIKE 'backup-rotation-broken%'
	AND check_name NOT LIKE 'preflight-object-lock%';

-- Issues already open at a failure keep dragging their server's health rollup
-- down and holding incidents open until their check next files. Re-grade the
-- open ones to match the policy that now applies, and release any incident
-- they were holding open on their own.
CREATE TEMP TABLE regraded_backup_issues ON COMMIT DROP AS
SELECT i.id
FROM issues i
JOIN check_policies p
	ON p.source = i.source AND p.check_name = i.check_name
WHERE i.source = 'canopy'
	AND i.effective_result = 'failed'
	AND p.ceiling = 'warning'
	AND (
		i.check_name LIKE 'backup-%'
		OR i.check_name LIKE 'preflight-%'
		OR i.check_name LIKE 'restore-verification%'
		OR i.check_name LIKE 'redaction%'
		OR i.check_name LIKE 'migration-test%'
	);

UPDATE issues
SET effective_result = 'warning', escalates = false
WHERE id IN (SELECT id FROM regraded_backup_issues);

UPDATE incident_issues
SET left_at = now()
WHERE left_at IS NULL
	AND issue_id IN (SELECT id FROM regraded_backup_issues);

-- Close incidents that were only being held open by those failures. Mirrors
-- the leave path in re_evaluate_incident_membership: an incident is held open
-- by its currently-failing contributors. No Slack resolve is enqueued; these
-- incidents should not have been opened in the first place.
UPDATE incidents AS inc
SET closed_at = now()
WHERE inc.closed_at IS NULL
	AND EXISTS (
		SELECT 1 FROM incident_issues il
		WHERE il.incident_id = inc.id
			AND il.issue_id IN (SELECT id FROM regraded_backup_issues)
	)
	AND NOT EXISTS (
		SELECT 1
		FROM incident_issues il
		JOIN issues i ON i.id = il.issue_id
		WHERE il.incident_id = inc.id
			AND il.left_at IS NULL
			AND i.effective_result = 'failed'
	);
