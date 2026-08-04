-- The restore sweeps spelled the replica's type and intent into the check name:
-- restore-verification:tamanu-postgres:verify, redaction:tamanu-postgres:analytics,
-- migration-test:tamanu-postgres:migrate, and so on. Both dimensions are
-- open-ended strings, so that produced a check — and a catalog row, and a
-- listing entry — per (type, intent) pair a deployment happened to declare, for
-- one condition. A check name is a category an operator configures once;
-- anything that varies per instance belongs in the detail, where policy rules
-- read it as check.<field>. See the Names section of the CHK spec.
--
-- Each of these three checks is now filed once per server with its replicas as
-- instances, graded per replica and settling on the most urgent. Collapse the
-- stored state to match. This mirrors the collapse the backup checks went
-- through in 2026-08-04-022520, extended to names carrying two parameters
-- instead of one.

-- 1. Issues. Several per-(type, intent) rows collapse onto one per
--    (target, source, ref), so pick a survivor per target and drop the rest
--    rather than letting the uniqueness constraints reject the rename. The
--    survivor is the most urgent row, then the most recently seen: that keeps
--    the row an operator is most likely already looking at, with its incident
--    membership and history.
--
--    The survivor's per-replica detail is left as it is. It describes one
--    replica where the new shape carries every degraded instance, and the next
--    sweep overwrites it with the aggregate — within a minute, since the sweep
--    runs on the reachability tick.
CREATE TEMP TABLE restore_check_renames ON COMMIT DROP AS
SELECT
	id,
	base,
	row_number() OVER (
		PARTITION BY source, base, server_id, server_group_id
		ORDER BY
			CASE effective_result
				WHEN 'failed' THEN 0
				WHEN 'warning' THEN 1
				WHEN 'broken' THEN 2
				WHEN 'passed' THEN 3
				ELSE 4
			END,
			last_seen DESC,
			id
	) AS rank
FROM (
	SELECT i.*, split_part(i.ref, ':', 1) AS base
	FROM issues i
	WHERE i.source = 'canopy'
		AND i.ref LIKE '%:%'
		AND split_part(i.ref, ':', 1) IN (
			'restore-verification',
			'redaction',
			'migration-test'
		)
) AS parameterised;

-- Which incidents the departing rows were contributing to, captured before the
-- rows go so the emptied ones can be retired afterwards.
CREATE TEMP TABLE touched_restore_incidents ON COMMIT DROP AS
SELECT DISTINCT il.incident_id
FROM incident_issues il
WHERE il.left_at IS NULL
	AND il.issue_id IN (SELECT id FROM restore_check_renames WHERE rank > 1);

UPDATE incident_issues
SET left_at = now()
WHERE left_at IS NULL
	AND issue_id IN (SELECT id FROM restore_check_renames WHERE rank > 1);

DELETE FROM issues
WHERE id IN (SELECT id FROM restore_check_renames WHERE rank > 1);

UPDATE issues AS i
SET ref = r.base, check_name = r.base
FROM restore_check_renames r
WHERE i.id = r.id AND r.rank = 1;

-- Retire incidents the departures emptied of failing contributors. Mirrors the
-- leave path in re_evaluate_incident_membership: an incident is held open by its
-- currently-failing contributors. No Slack resolve is enqueued — the condition
-- didn't change, only how many rows describe it.
UPDATE incidents AS inc
SET closed_at = now()
WHERE inc.closed_at IS NULL
	AND inc.id IN (SELECT incident_id FROM touched_restore_incidents)
	AND NOT EXISTS (
		SELECT 1
		FROM incident_issues il
		JOIN issues i ON i.id = il.issue_id
		WHERE il.incident_id = inc.id
			AND il.left_at IS NULL
			AND i.effective_result = 'failed'
	);

-- 2. Scoped policies, which is where silences live. A silence on
--    restore-verification:tamanu-postgres:verify meant "silence restore
--    verification for that one replica". Under one check name the equivalent is
--    a rule that skips the instance the silence was written for, which is what
--    per-instance grading is for. Rewrite them so operators keep the exact
--    suppression they set up, instead of it silently widening to every replica of
--    the server or being dropped.
--
--    The rules column is an if-ladder: {"if": [condition, result, …]} (see
--    check_policies::IfLadder, and Condition's JsonLogic encoding). A Condition
--    is one operator over one variable and the DSL has no and/or, so two
--    conditions — check.type and check.intent — cannot be combined into a single
--    guard. Every instance therefore carries check.replica_key, the two
--    dimensions joined as "<type>:<intent>": one variable, and it identifies the
--    replica exactly, so the guard matches on that rather than either dimension
--    alone. A ceiling-only silence becomes a single-branch ladder and gives up
--    its ceiling; a row that already carried rules keeps them, with the guard
--    prepended so the guard wins.
UPDATE scoped_check_policies
SET
	check_name = split_part(check_name, ':', 1),
	ceiling = NULL,
	rules = jsonb_build_object(
		'if',
		jsonb_build_array(
			jsonb_build_object(
				'==',
				jsonb_build_array(
					jsonb_build_object('var', 'check.replica_key'),
					substring(check_name from position(':' in check_name) + 1)
				)
			),
			COALESCE(ceiling, 'skipped')
		) || COALESCE(rules -> 'if', '[]'::jsonb)
	)
WHERE source = 'canopy'
	AND check_name LIKE '%:%'
	AND split_part(check_name, ':', 1) IN (
		'restore-verification',
		'redaction',
		'migration-test'
	);

-- 3. Catalog rows: the thing that made an operator configure one condition once
--    per (type, intent) pair. Keep the most deliberately-set row's policy for the
--    collapsed name — an operator-reviewed row beats a canopy-seeded one, and
--    among equals the most urgent ceiling wins — and drop the rest. If nothing
--    survives, the next filing re-registers the collapsed name with its shipped
--    defaults.
CREATE TEMP TABLE restore_policy_renames ON COMMIT DROP AS
SELECT
	source,
	check_name,
	base,
	row_number() OVER (
		PARTITION BY source, base
		ORDER BY
			CASE WHEN reviewed_by IS DISTINCT FROM 'canopy' THEN 0 ELSE 1 END,
			CASE ceiling
				WHEN 'failed' THEN 0
				WHEN 'warning' THEN 1
				WHEN 'broken' THEN 2
				WHEN 'passed' THEN 3
				ELSE 4
			END,
			check_name
	) AS rank
FROM (
	SELECT p.*, split_part(p.check_name, ':', 1) AS base
	FROM check_policies p
	WHERE p.source = 'canopy'
		AND p.check_name LIKE '%:%'
		AND split_part(p.check_name, ':', 1) IN (
			'restore-verification',
			'redaction',
			'migration-test'
		)
) AS parameterised;

DELETE FROM check_policies p
USING restore_policy_renames r
WHERE p.source = r.source AND p.check_name = r.check_name AND r.rank > 1;

-- A bare collapsed row already existing wins outright; the parameterised one
-- goes rather than colliding on the rename.
DELETE FROM check_policies p
USING restore_policy_renames r
WHERE p.source = r.source
	AND p.check_name = r.check_name
	AND r.rank = 1
	AND EXISTS (
		SELECT 1 FROM check_policies q
		WHERE q.source = r.source AND q.check_name = r.base
	);

UPDATE check_policies p
SET check_name = r.base
FROM restore_policy_renames r
WHERE p.source = r.source AND p.check_name = r.check_name AND r.rank = 1;
