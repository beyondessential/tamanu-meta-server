-- The severity catalog becomes the check-policy catalog: keyed per
-- (source, check) now that checks are keyed per reporting source, and
-- speaking result transforms instead of severities. The ceiling is the
-- maximum effective result (failed = no cap, warning = failures grade as
-- warnings, passed = recorded but never alerting, skipped = the source
-- may stop running the check); escalates marks checks whose effective
-- failure notifies immediately, bypassing incident grace — the residue
-- of the Critical severity.
CREATE TABLE check_policies (
	source TEXT NOT NULL,
	check_name TEXT NOT NULL,
	ceiling TEXT NOT NULL DEFAULT 'warning',
	escalates BOOLEAN NOT NULL DEFAULT FALSE,
	rules JSONB,
	notes TEXT,
	first_seen TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	reviewed_at TIMESTAMP WITH TIME ZONE,
	reviewed_by TEXT,
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	PRIMARY KEY (source, check_name)
);

-- Existing catalog rows were all reported by alertd. Severity → policy:
-- Critical → failed + escalates, Error → failed, Warning → warning,
-- Info → passed, Debug → skipped. An entry whose rules ladder could
-- output critical also escalates (escalation is per-entry now, not
-- per-branch).
INSERT INTO check_policies
	(source, check_name, ceiling, escalates, rules, notes, first_seen, reviewed_at, reviewed_by, updated_at)
SELECT
	'alertd',
	check_name,
	CASE severity
		WHEN 'critical' THEN 'failed'
		WHEN 'error' THEN 'failed'
		WHEN 'warning' THEN 'warning'
		WHEN 'info' THEN 'passed'
		WHEN 'debug' THEN 'skipped'
		ELSE 'warning'
	END,
	severity = 'critical' OR EXISTS (
		SELECT 1
		FROM jsonb_array_elements(rules -> 'if') WITH ORDINALITY AS t(elem, ord)
		WHERE ord % 2 = 0 AND elem #>> '{}' = 'critical'
	),
	rules,
	notes,
	first_seen,
	reviewed_at,
	reviewed_by,
	updated_at
FROM healthcheck_severities;

-- Rules ladders output results now: rewrite each branch's severity
-- string (the even-ordinal elements of the "if" array) to the result it
-- maps to.
UPDATE check_policies
SET rules = (
	SELECT jsonb_build_object(
		'if',
		jsonb_agg(
			CASE
				WHEN ord % 2 = 1 THEN elem
				ELSE to_jsonb(
					CASE elem #>> '{}'
						WHEN 'critical' THEN 'failed'
						WHEN 'error' THEN 'failed'
						WHEN 'warning' THEN 'warning'
						WHEN 'info' THEN 'passed'
						WHEN 'debug' THEN 'skipped'
						ELSE 'warning'
					END
				)
			END
			ORDER BY ord
		)
	)
	FROM jsonb_array_elements(rules -> 'if') WITH ORDINALITY AS t(elem, ord)
)
WHERE rules IS NOT NULL;

DROP TABLE healthcheck_severities;
