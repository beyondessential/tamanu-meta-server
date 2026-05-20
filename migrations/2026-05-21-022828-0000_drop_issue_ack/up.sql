-- Drop ack on issues. The "acknowledged" state is kept on incidents
-- where it tracks "an operator is aware" of an incident-class outage,
-- but on individual issues it was just tedious busywork — and
-- orthogonal to resolution, which is what actually matters.
ALTER TABLE issues DROP COLUMN acknowledged_at;
ALTER TABLE issues DROP COLUMN acknowledged_by;
