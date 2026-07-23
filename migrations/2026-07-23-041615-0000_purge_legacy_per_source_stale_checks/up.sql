-- The old per-source staleness mechanism filed a `canopy`/`stale/<source>`
-- check per reporting source. It was folded into the single `reachability`
-- check, and `delete_legacy_stale_checks` removed the old rows — but
-- transitional code re-filed a batch during the deploy window, and no
-- current code manages `stale/<source>` any more. Those check-states have
-- lingered active/unresolved ever since, holding servers into a warning with
-- no catalog entry an operator can act on.
--
-- Resolve the stranded check-states and drop their catalog rows for good.
-- They grade to warning (never failed), so none can be holding an incident
-- open; no incident re-evaluation is needed.
UPDATE issues
SET
	active = false,
	resolved_at = now(),
	resolved_by = 'system',
	resolved_reason = 'decommissioned'
WHERE source = 'canopy'
	AND check_name LIKE 'stale/%'
	AND resolved_at IS NULL;

DELETE FROM check_policies
WHERE source = 'canopy' AND check_name LIKE 'stale/%';
