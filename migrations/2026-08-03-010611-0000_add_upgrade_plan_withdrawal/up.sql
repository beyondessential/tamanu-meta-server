-- Withdrawing a plan says the deployment is no longer going there. That
-- decision is part of the upgrade history, so the plan is retained rather than
-- removed.
ALTER TABLE upgrade_plans
	ADD COLUMN withdrawn_at TIMESTAMPTZ,
	ADD COLUMN withdrawn_by TEXT;

-- A withdrawn plan is history, so it must not hold the group's one open slot.
DROP INDEX upgrade_plans_one_open_per_group;

CREATE UNIQUE INDEX upgrade_plans_one_open_per_group
	ON upgrade_plans (group_id)
	WHERE met_at IS NULL AND superseded_at IS NULL AND withdrawn_at IS NULL;
