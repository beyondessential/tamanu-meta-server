-- Drop ack from both issues and incidents. It was tedious busywork that
-- didn't gate any actual state transition — resolution is what tracks
-- "this is dealt with". Anyone who wants to flag "I'm aware" can drop
-- a note instead.
ALTER TABLE issues DROP COLUMN acknowledged_at;
ALTER TABLE issues DROP COLUMN acknowledged_by;
ALTER TABLE incidents DROP COLUMN acknowledged_at;
ALTER TABLE incidents DROP COLUMN acknowledged_by;
