-- Per-state stability record (CHK "Stability"): a fixed-size summary of a
-- check's observed behaviour, updated on every filing. One row per check
-- state (the issues row), from *observed* results so policy edits and any
-- damping built on top never feed back into the statistics.
--
-- `transitions` is a bounded ring (newest last) of healthy<->degraded
-- transitions: [{"at": <RFC3339>, "degraded": bool}, ...].
-- `duty_cycle` is 168 hour-of-week buckets (UTC, Monday 00:00 first):
-- [[observations, degraded], ...]; both counters in a bucket are halved
-- when the bucket's observations cross a cap, so the profile leans
-- towards recent weeks without unbounded growth.
--
-- Attaching the foreign key adds triggers on `issues`, which needs a
-- SHARE ROW EXCLUSIVE lock there. That's instant to hold but must queue
-- behind any long-running transaction touching issues — and while queued
-- it blocks ingestion writes behind it. Fail fast instead of piling up;
-- a failed migrate run is simply retried.
SET LOCAL lock_timeout = '5s';

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

-- One-shot marker for the status-history backfill, which runs on the
-- monitor pod (per-server, short transactions) rather than as a data
-- migration: a single fleet-wide INSERT..SELECT would hold FK row locks
-- (FOR KEY SHARE) on most live issues rows until commit, blocking every
-- concurrent filing's SELECT .. FOR UPDATE for the whole run — ingestion
-- downtime. The pod inserts a row here when the backfill has completed.
CREATE TABLE check_stability_backfill (
	done_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
