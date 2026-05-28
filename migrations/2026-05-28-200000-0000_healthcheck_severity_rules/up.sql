-- Per-check conditional severity rules.
--
-- The whole rule ladder for a check lives in a single JsonLogic blob
-- (a constrained `{"if": [c1, s1, c2, s2, …, cN, sN]}` shape that
-- evaluates to a Severity string). NULL means "no conditional rules,
-- use the row's severity column directly" — the v1 behaviour.
--
-- Validation of the JSON shape lives in Rust at the API layer; the
-- column is a plain JSONB so a hand-edited row only degrades to the
-- base severity at evaluation time, never crashes the ingestion path.
--
-- See docs/plans/healthcheck-severity-rules-v2.md.

ALTER TABLE healthcheck_severities
	ADD COLUMN rules JSONB;
