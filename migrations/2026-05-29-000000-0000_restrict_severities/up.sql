-- Severity catalog restricted to debug / info / warning / error / critical.
-- emergency and alert collapse into critical; notice collapses into info.
-- See docs note in commons-types::issue::Severity.
--
-- The five tables/columns carrying a severity value:
--   - issues.severity           (TEXT)
--   - events.severity           (TEXT, append-only log)
--   - healthcheck_severities.severity (TEXT, has a CHECK constraint)
--   - healthcheck_severities.rules     (JSONB if-ladder, severities at odd indices)
--   - slack_outbox.payload      (JSONB, the rendered notification — left as-is;
--                                already-delivered rows are historical, the
--                                drainer doesn't re-read severities from them)

-- Data migration.

UPDATE issues
SET severity = CASE severity
	WHEN 'emergency' THEN 'critical'
	WHEN 'alert' THEN 'critical'
	WHEN 'notice' THEN 'info'
	ELSE severity
END
WHERE severity IN ('emergency', 'alert', 'notice');

UPDATE events
SET severity = CASE severity
	WHEN 'emergency' THEN 'critical'
	WHEN 'alert' THEN 'critical'
	WHEN 'notice' THEN 'info'
	ELSE severity
END
WHERE severity IN ('emergency', 'alert', 'notice');

UPDATE healthcheck_severities
SET severity = CASE severity
	WHEN 'emergency' THEN 'critical'
	WHEN 'alert' THEN 'critical'
	WHEN 'notice' THEN 'info'
	ELSE severity
END
WHERE severity IN ('emergency', 'alert', 'notice');

-- Migrate severity strings embedded in the rules JsonLogic if-ladder.
-- Severities appear at odd indices of the `if` array; lower-severity
-- string-typed *values* in conditions could theoretically also be
-- "emergency"/"alert"/"notice", but in practice operators predicate on
-- bestool fields (semver, numbers, hostnames) — not on the severity
-- vocabulary itself — so the textual replace is safe.
UPDATE healthcheck_severities
SET rules = regexp_replace(
	regexp_replace(
		regexp_replace(rules::text, '"emergency"', '"critical"', 'g'),
		'"alert"', '"critical"', 'g'
	),
	'"notice"', '"info"', 'g'
)::jsonb
WHERE rules IS NOT NULL
	AND rules::text ~ '"(emergency|alert|notice)"';

-- Drop the existing CHECK constraint on healthcheck_severities.severity
-- and replace with the narrower set.

ALTER TABLE healthcheck_severities
	DROP CONSTRAINT healthcheck_severities_severity_check;

ALTER TABLE healthcheck_severities
	ADD CONSTRAINT healthcheck_severities_severity_check
	CHECK (severity IN ('debug', 'info', 'warning', 'error', 'critical'));
