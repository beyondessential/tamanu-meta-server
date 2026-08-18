-- The hour a deployment moves, alongside the day. An upgrade that starts at
-- midnight local and one that starts at 7:30pm are different nights of work for
-- the people running them, and a date alone cannot say which.
--
-- The zone travels with the time because a wall clock without one is only
-- readable by whoever typed it: the fleet spans Fiji, Nauru and Pakistan, and
-- Canopy holds no timezone for a group to fall back on.
ALTER TABLE upgrade_plans
	ADD COLUMN planned_time TIME,
	ADD COLUMN planned_zone TEXT;

-- A time qualifies a day, and a wall clock is meaningless without its zone.
ALTER TABLE upgrade_plans
	ADD CONSTRAINT upgrade_plans_time_needs_date_and_zone CHECK (
		(planned_time IS NULL) = (planned_zone IS NULL)
		AND (planned_time IS NULL OR planned_for IS NOT NULL)
	);
