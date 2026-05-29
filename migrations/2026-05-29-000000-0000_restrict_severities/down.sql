-- Best-effort revert: restore the 8-severity CHECK constraint on
-- healthcheck_severities.severity. The data normalization (emergency /
-- alert → critical, notice → info) is not reversed — the original
-- distinctions are lost.

ALTER TABLE healthcheck_severities
	DROP CONSTRAINT healthcheck_severities_severity_check;

ALTER TABLE healthcheck_severities
	ADD CONSTRAINT healthcheck_severities_severity_check
	CHECK (severity IN ('emergency','alert','critical','error','warning','notice','info','debug'));
