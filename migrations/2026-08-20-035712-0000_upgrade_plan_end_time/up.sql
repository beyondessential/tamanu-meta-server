-- The hour the upgrade window closes, a wall clock in the same zone as the
-- start. An end before the start is the next morning: a window that opens at
-- 22:00 and closes at 02:00 is one night, not a plan that runs backwards.
ALTER TABLE upgrade_plans
	ADD COLUMN planned_end_time TIME;

-- An end qualifies a start, and one equal to it would be a window of no length
-- or a full day depending on which way the reader takes it.
ALTER TABLE upgrade_plans
	ADD CONSTRAINT upgrade_plans_end_needs_start CHECK (
		planned_end_time IS NULL
		OR (planned_time IS NOT NULL AND planned_end_time <> planned_time)
	);
