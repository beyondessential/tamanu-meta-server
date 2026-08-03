-- DESTRUCTIVE: withdrawn plans are dropped rather than reopened, since
-- restoring the narrower index would fail for a group that has withdrawn one
-- plan and recorded another.
DELETE FROM upgrade_plans WHERE withdrawn_at IS NOT NULL;

DROP INDEX upgrade_plans_one_open_per_group;

CREATE UNIQUE INDEX upgrade_plans_one_open_per_group
	ON upgrade_plans (group_id)
	WHERE met_at IS NULL AND superseded_at IS NULL;

ALTER TABLE upgrade_plans
	DROP COLUMN withdrawn_at,
	DROP COLUMN withdrawn_by;
