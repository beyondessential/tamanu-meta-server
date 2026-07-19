-- Per-state stability record (CHK "Stability"): a fixed-size summary of a
-- check's observed behaviour, updated on every filing. One row per check
-- state (the issues row), from *observed* results so policy edits and any
-- damping built on top never feed back into the statistics.
--
-- `transitions` is a bounded ring (newest last) of healthy<->degraded
-- transitions: [{"at": <epoch seconds>, "degraded": bool}, ...].
-- `duty_cycle` is 168 hour-of-week buckets (UTC, Monday 00:00 first):
-- [[observations, degraded], ...]; both counters in a bucket are halved
-- when the bucket's observations cross a cap, so the profile leans
-- towards recent weeks without unbounded growth.
CREATE TABLE check_stability (
	issue_id UUID PRIMARY KEY REFERENCES issues (id) ON DELETE CASCADE,
	created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	observations BIGINT NOT NULL DEFAULT 0,
	degraded_observations BIGINT NOT NULL DEFAULT 0,
	last_observed_at TIMESTAMPTZ,
	last_observed_degraded BOOLEAN,
	transitions JSONB NOT NULL DEFAULT '[]',
	duty_cycle JSONB NOT NULL DEFAULT '[]'
);

SELECT diesel_manage_updated_at('check_stability');
