-- Issues grow toward the check-state model: each row records which check
-- it tracks and both sides of the policy transform (what the source
-- observed, what policy made of it), plus the check's latest detail.
-- Nullable while the severity vocabulary is still authoritative; filing
-- populates them from here on.
ALTER TABLE issues ADD COLUMN check_name TEXT;
ALTER TABLE issues ADD COLUMN observed_result TEXT;
ALTER TABLE issues ADD COLUMN effective_result TEXT;
ALTER TABLE issues ADD COLUMN detail JSONB;

-- Backfill health-check rows: the check is the ref minus its prefix, and
-- the effective result is read back from the filed severity (the reverse
-- of the transitional severity mapping). The observed result is not
-- recoverable from an issue row; assume it matched the effective one.
UPDATE issues SET
	check_name = substring(ref from 8),
	observed_result = CASE
		WHEN NOT active THEN 'passed'
		WHEN severity IN ('critical', 'error') THEN 'failed'
		ELSE 'warning'
	END,
	effective_result = CASE
		WHEN NOT active THEN 'passed'
		WHEN severity IN ('critical', 'error') THEN 'failed'
		ELSE 'warning'
	END
	WHERE ref LIKE 'health/%';

-- Broken-thread rows observed a broken check; their fixed-Warning filing
-- is the brokenness itself counting as a warning.
UPDATE issues SET
	check_name = substring(ref from 15),
	observed_result = CASE WHEN active THEN 'broken' ELSE 'passed' END,
	effective_result = CASE WHEN active THEN 'broken' ELSE 'passed' END
	WHERE ref LIKE 'health-broken/%';
