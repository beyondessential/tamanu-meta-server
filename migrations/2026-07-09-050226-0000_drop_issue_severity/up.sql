-- The severity vocabulary is fully retired: incident semantics key on
-- (effective_result, escalates), and every surface speaks results.
-- Backfill rows that predate the check-state model (never stamped by a
-- filing) so their incident participation carries over, then drop.
UPDATE issues SET
	check_name = ref,
	observed_result = CASE
		WHEN NOT active THEN 'passed'
		WHEN severity IN ('critical', 'error') THEN 'failed'
		WHEN severity = 'warning' THEN 'warning'
		ELSE 'skipped'
	END,
	effective_result = CASE
		WHEN NOT active THEN 'passed'
		WHEN severity IN ('critical', 'error') THEN 'failed'
		WHEN severity = 'warning' THEN 'warning'
		ELSE 'skipped'
	END
	WHERE effective_result IS NULL;

ALTER TABLE issues DROP COLUMN severity;
