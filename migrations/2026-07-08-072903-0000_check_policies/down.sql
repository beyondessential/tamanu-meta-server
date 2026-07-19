-- Best-effort reversal: ceilings map back to base severities (escalating
-- failures to critical), rules branches map back to severity strings, and
-- per-source entries collapse to one row per check name (alertd wins,
-- otherwise an arbitrary source's entry).
CREATE TABLE healthcheck_severities (
	check_name TEXT PRIMARY KEY,
	severity TEXT NOT NULL DEFAULT 'warning',
	first_seen TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	reviewed_at TIMESTAMP WITH TIME ZONE,
	reviewed_by TEXT,
	notes TEXT,
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	rules JSONB
);

INSERT INTO healthcheck_severities
	(check_name, severity, first_seen, reviewed_at, reviewed_by, notes, updated_at, rules)
SELECT DISTINCT ON (check_name)
	check_name,
	CASE ceiling
		WHEN 'failed' THEN CASE WHEN escalates THEN 'critical' ELSE 'error' END
		WHEN 'warning' THEN 'warning'
		WHEN 'passed' THEN 'info'
		WHEN 'skipped' THEN 'debug'
		ELSE 'warning'
	END,
	first_seen,
	reviewed_at,
	reviewed_by,
	notes,
	updated_at,
	(
		SELECT jsonb_build_object(
			'if',
			jsonb_agg(
				CASE
					WHEN ord % 2 = 1 THEN elem
					ELSE to_jsonb(
						CASE elem #>> '{}'
							WHEN 'failed' THEN 'error'
							WHEN 'warning' THEN 'warning'
							WHEN 'passed' THEN 'info'
							WHEN 'skipped' THEN 'debug'
							ELSE 'warning'
						END
					)
				END
				ORDER BY ord
			)
		)
		FROM jsonb_array_elements(rules -> 'if') WITH ORDINALITY AS t(elem, ord)
	)
FROM check_policies
ORDER BY check_name, (source = 'alertd') DESC;

DROP TABLE check_policies;
