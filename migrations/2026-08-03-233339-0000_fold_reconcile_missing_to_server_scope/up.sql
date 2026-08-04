-- backup-reconcile-missing was filed group-scoped, on the reasoning that it
-- should page regardless of a member's monitoring state. But the finding is
-- about one server: every server in a group collided on the single
-- (server_group_id, source, 'backup-reconcile-missing:<type>') row, so one
-- server's failure overwrote another's, and a healthy server's recovery
-- cleared a broken server's alert. It is now an ordinary server-scoped check
-- with the same ref, subject to the monitoring gate like the other
-- per-server backup signals.
--
-- The ref is unchanged, so stored silences and the catalog rows carry over
-- untouched. The server a row was last about is recoverable from its check
-- detail, and for rows predating detail-carrying filings, from the message
-- text (which has always led with "Server <uuid> ").
--
-- Alerts the collision hid are not recoverable and are not recreated here:
-- the next sweep files them fresh against their own servers.

-- 1. Fold rows whose server still exists onto server scope. A server belongs
--    to one group and this ref was never filed server-scoped before, so
--    there is no issues_server_id_source_ref_key collision. The row keeps
--    its incident membership and its history; the next sweep rewrites its
--    message to name the server.
UPDATE issues AS i
SET
	server_id = a.server_id,
	server_group_id = NULL
FROM (
	SELECT
		id,
		COALESCE(
			NULLIF(detail ->> 'server_id', ''),
			substring(message FROM '^Server ([0-9a-fA-F-]{36}) ')
		)::uuid AS server_id
	FROM issues
	WHERE source = 'canopy'
		AND server_group_id IS NOT NULL
		AND ref LIKE 'backup-reconcile-missing:%'
		AND COALESCE(
			NULLIF(detail ->> 'server_id', ''),
			substring(message FROM '^Server ([0-9a-fA-F-]{36}) ')
		) ~ '^[0-9a-fA-F-]{36}$'
) AS a
WHERE i.id = a.id
	AND EXISTS (SELECT 1 FROM servers s WHERE s.id = a.server_id);

-- 2. Anything still group-scoped has no server to attribute it to: the
--    server was deleted, or the row predates both attribution paths. No code
--    path can reach such a row now, so it would hold its incident open
--    forever. Release it and resolve it.
CREATE TEMP TABLE orphaned_reconcile_missing ON COMMIT DROP AS
SELECT id FROM issues
WHERE source = 'canopy'
	AND server_group_id IS NOT NULL
	AND ref LIKE 'backup-reconcile-missing:%';

UPDATE incident_issues
SET left_at = now()
WHERE left_at IS NULL
	AND issue_id IN (SELECT id FROM orphaned_reconcile_missing);

UPDATE issues
SET
	active = false,
	resolved_at = now(),
	resolved_by = 'migration',
	resolved_reason = 'group-scoped reconcile-missing folded to server scope; no server to fold this row onto'
WHERE id IN (SELECT id FROM orphaned_reconcile_missing);

-- 3. Close any incident those releases emptied. Mirrors the leave path in
--    re_evaluate_incident_membership: an incident is held open by its
--    currently-failing contributors, so one with none left retires. No Slack
--    resolve is enqueued — a migration is not an event the fleet should be
--    paged about, and these incidents are about servers that no longer
--    exist.
UPDATE incidents AS inc
SET closed_at = now()
WHERE inc.closed_at IS NULL
	AND EXISTS (
		SELECT 1 FROM incident_issues il
		WHERE il.incident_id = inc.id
			AND il.issue_id IN (SELECT id FROM orphaned_reconcile_missing)
	)
	AND NOT EXISTS (
		SELECT 1
		FROM incident_issues il
		JOIN issues i ON i.id = il.issue_id
		WHERE il.incident_id = inc.id
			AND il.left_at IS NULL
			AND i.effective_result = 'failed'
	);
