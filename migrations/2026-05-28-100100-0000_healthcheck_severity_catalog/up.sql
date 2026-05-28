-- Operator-owned catalog of healthcheck names → the severity that
-- failures get filed at. Until an operator reviews a row it is
-- considered "pending review" (reviewed_at IS NULL) with the default
-- severity of `warning`. The companion code change in
-- crates/public-server/src/statuses.rs upserts a row for every check
-- name seen on a status push, so the catalog grows automatically as
-- bestool introduces new checks.
--
-- v2 (future) will extend this with extra-data-conditional mappings
-- (e.g. raise to error when disk_space.free_pct < N); the schema here
-- stays narrow on purpose.

CREATE TABLE healthcheck_severities (
	check_name TEXT PRIMARY KEY,
	-- Severity to file the (status, health/<check_name>) issue at when
	-- the check is unhealthy. RFC 5424 — validated as commons_types::issue::Severity
	-- at the API layer.
	severity TEXT NOT NULL DEFAULT 'warning'
		CHECK (severity IN ('emergency','alert','critical','error','warning','notice','info','debug')),
	-- First time canopy saw this check name (set by the upsert that
	-- creates the row).
	first_seen TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	-- NULL ⇒ pending review. Set the first time an operator explicitly
	-- saves the row, even if the severity wasn't changed (just acking
	-- counts as a review).
	reviewed_at TIMESTAMP WITH TIME ZONE,
	reviewed_by TEXT,
	-- Free-form operator notes ("noisy, downgrade until issue X").
	notes TEXT,
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

SELECT diesel_manage_updated_at('healthcheck_severities');

-- Quick "what's pending review" lookup for the catalog UI.
CREATE INDEX healthcheck_severities_pending_review_idx
	ON healthcheck_severities (first_seen)
	WHERE reviewed_at IS NULL;
