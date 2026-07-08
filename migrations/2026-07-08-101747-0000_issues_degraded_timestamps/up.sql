-- With passing checks about to get state rows, "when did this start
-- failing" and "has this ever been trouble" stop being derivable from
-- first_seen / row existence. degraded_since tracks the current
-- degradation streak (null while healthy); last_degraded_at never
-- clears, distinguishing a recovered issue from always-healthy state.
ALTER TABLE issues ADD COLUMN degraded_since TIMESTAMP WITH TIME ZONE;
ALTER TABLE issues ADD COLUMN last_degraded_at TIMESTAMP WITH TIME ZONE;

-- Every existing row was filed because it degraded at some point.
UPDATE issues SET last_degraded_at = last_seen;
UPDATE issues SET degraded_since = first_seen WHERE active;
