-- Silences (scoped skipped-ceiling policies) for dead checks — a
-- (source, check) with no live catalog row, i.e. decommissioned or orphaned
-- (no catalog row at all, e.g. superseded sources like bestool-alertd) —
-- linger forever. The check contributes to nothing, so the silence is dead
-- configuration cluttering the operator's silence list. Going forward,
-- decommission clears a check's silences and the silence list hides dead
-- ones; this clears the ones already stranded.
DELETE FROM scoped_check_policies scp
WHERE scp.ceiling = 'skipped'
	AND NOT EXISTS (
		SELECT 1 FROM check_policies cp
		WHERE cp.source = scp.source
			AND cp.check_name = scp.check_name
			AND cp.decommissioned_at IS NULL
	);
