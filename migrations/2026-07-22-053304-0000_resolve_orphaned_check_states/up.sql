-- Close orphaned check-states: `issues` check-state rows whose
-- (source, check_name) has no `check_policies` catalog row at all. These
-- arose from legacy/superseded sources (e.g. bestool-alertd) whose catalog
-- rows were removed while their states were left behind. An orphan is
-- invisible in the healthchecks settings (which reads the catalog), yet the
-- server-detail view and health rollup surfaced it as a failing check with
-- no way for an operator to silence or decommission it.
--
-- The read paths now ignore check-states with no live catalog row, but
-- these rows are still unresolved. Resolve them so they are properly closed
-- (out of issue lists, incident eval, and staleness). None are members of
-- open incidents, so no incident re-evaluation is needed here.
UPDATE issues
SET
	resolved_at = now(),
	resolved_by = 'system',
	resolved_reason = 'decommissioned'
WHERE check_name IS NOT NULL
	AND resolved_at IS NULL
	AND NOT EXISTS (
		SELECT 1 FROM check_policies cp
		WHERE cp.source = issues.source AND cp.check_name = issues.check_name
	);
