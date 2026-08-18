ALTER TABLE upgrade_plans
	DROP CONSTRAINT upgrade_plans_time_needs_date_and_zone;

ALTER TABLE upgrade_plans
	DROP COLUMN planned_time,
	DROP COLUMN planned_zone;
