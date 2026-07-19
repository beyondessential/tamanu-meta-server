-- Backfill stability records from the last 30 days of status history, so
-- the duty-cycle profiles and transition rings are useful immediately
-- instead of warming up over weeks of live filings.
--
-- Approximations, versus live recording:
-- - only checks *present* in a push's health array are replayed; live
--   recording also counts recovery-by-omission as a healthy observation.
-- - canopy's own checks (reachability, backups, key expiry, …) have no
--   status history and start cold.
-- Existing rows (live recording that started before this ran) are left
-- untouched.

WITH obs AS (
	SELECT
		s.server_id,
		s.source,
		e ->> 'check' AS check_name,
		s.created_at,
		CASE
			WHEN e ->> 'result' IN ('failed', 'warning', 'broken') THEN TRUE
			WHEN e ->> 'result' = 'passed' THEN FALSE
			-- Legacy entries carry a boolean instead of a result.
			WHEN NOT e ? 'result' AND jsonb_typeof(e -> 'healthy') = 'boolean'
				THEN NOT (e ->> 'healthy')::boolean
			-- 'skipped' and unparseable entries carry no signal.
			ELSE NULL
		END AS degraded
	FROM statuses s
	CROSS JOIN LATERAL jsonb_array_elements(s.health) e
	WHERE s.created_at > NOW() - INTERVAL '30 days'
		AND e ->> 'check' IS NOT NULL
),
signal AS (
	SELECT * FROM obs WHERE degraded IS NOT NULL
),
counts AS (
	SELECT
		server_id, source, check_name,
		COUNT(*) AS observations,
		COUNT(*) FILTER (WHERE degraded) AS degraded_observations,
		MAX(created_at) AS last_observed_at
	FROM signal
	GROUP BY 1, 2, 3
),
last_state AS (
	SELECT DISTINCT ON (server_id, source, check_name)
		server_id, source, check_name, degraded AS last_observed_degraded
	FROM signal
	ORDER BY server_id, source, check_name, created_at DESC
),
-- Hour-of-week buckets (UTC, Monday 00:00 = 0), capped like live
-- recording: counters scale down so a bucket never exceeds the cap.
bucket_counts AS (
	SELECT
		server_id, source, check_name,
		(EXTRACT(ISODOW FROM created_at AT TIME ZONE 'UTC')::int - 1) * 24
			+ EXTRACT(HOUR FROM created_at AT TIME ZONE 'UTC')::int AS bucket,
		COUNT(*) AS n,
		COUNT(*) FILTER (WHERE degraded) AS d
	FROM signal
	GROUP BY 1, 2, 3, 4
),
duty AS (
	SELECT
		k.server_id, k.source, k.check_name,
		jsonb_agg(
			jsonb_build_array(
				LEAST(COALESCE(b.n, 0), 512),
				CASE
					WHEN COALESCE(b.n, 0) > 512
						THEN ROUND(COALESCE(b.d, 0) * 512.0 / b.n)::bigint
					ELSE COALESCE(b.d, 0)
				END
			)
			ORDER BY g.bucket
		) AS duty_cycle
	FROM (SELECT DISTINCT server_id, source, check_name FROM signal) k
	CROSS JOIN generate_series(0, 167) AS g (bucket)
	LEFT JOIN bucket_counts b
		ON b.server_id = k.server_id
		AND b.source = k.source
		AND b.check_name = k.check_name
		AND b.bucket = g.bucket
	GROUP BY 1, 2, 3
),
-- Transition points: observations whose degraded-ness differs from the
-- previous observation's (the first observation counts, matching live
-- recording); keep the newest 32 per state, oldest first in the ring.
flips AS (
	SELECT
		server_id, source, check_name, created_at, degraded,
		LAG(degraded) OVER w AS prev
	FROM signal
	WINDOW w AS (PARTITION BY server_id, source, check_name ORDER BY created_at)
),
ring AS (
	SELECT server_id, source, check_name,
		jsonb_agg(
			jsonb_build_object('at', to_jsonb(created_at), 'degraded', degraded)
			ORDER BY created_at
		) AS transitions
	FROM (
		SELECT *,
			ROW_NUMBER() OVER (
				PARTITION BY server_id, source, check_name
				ORDER BY created_at DESC
			) AS newest
		FROM flips
		WHERE prev IS DISTINCT FROM degraded
	) t
	WHERE newest <= 32
	GROUP BY 1, 2, 3
)
INSERT INTO check_stability
	(issue_id, observations, degraded_observations, last_observed_at,
	 last_observed_degraded, transitions, duty_cycle)
SELECT
	i.id,
	c.observations,
	c.degraded_observations,
	c.last_observed_at,
	ls.last_observed_degraded,
	COALESCE(r.transitions, '[]'::jsonb),
	d.duty_cycle
FROM counts c
JOIN issues i
	ON i.server_id = c.server_id
	AND i.source = c.source
	AND i.ref = 'health/' || c.check_name
JOIN last_state ls
	ON ls.server_id = c.server_id
	AND ls.source = c.source
	AND ls.check_name = c.check_name
JOIN duty d
	ON d.server_id = c.server_id
	AND d.source = c.source
	AND d.check_name = c.check_name
LEFT JOIN ring r
	ON r.server_id = c.server_id
	AND r.source = c.source
	AND r.check_name = c.check_name
ON CONFLICT (issue_id) DO NOTHING;
