ALTER TABLE upgrade_plans
	DROP CONSTRAINT upgrade_plans_end_needs_start;

ALTER TABLE upgrade_plans
	DROP COLUMN planned_end_time;
