-- The hour a deployment moves, alongside the day. The zone travels with it:
-- Canopy holds no timezone for a group, so a bare wall clock is readable only
-- by whoever typed it.
ALTER TABLE upgrade_plans
	ADD COLUMN planned_time TIME,
	ADD COLUMN planned_zone TEXT;

-- A time qualifies a day, and a wall clock is meaningless without its zone.
ALTER TABLE upgrade_plans
	ADD CONSTRAINT upgrade_plans_time_needs_date_and_zone CHECK (
		(planned_time IS NULL) = (planned_zone IS NULL)
		AND (planned_time IS NULL OR planned_for IS NOT NULL)
	);
