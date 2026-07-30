-- Where a deployment is going: the version a group intends to move to, and
-- optionally when. A statement of intent, not an instruction; nothing acts on
-- the date.
CREATE TABLE upgrade_plans (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	group_id UUID NOT NULL REFERENCES server_groups (id) ON DELETE CASCADE,
	target_version_id UUID NOT NULL REFERENCES versions (id) ON DELETE CASCADE,
	planned_for DATE,
	note TEXT,
	created_by TEXT,
	created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	-- Canopy decides a plan is met once the group's reported version reaches
	-- the target; nobody ticks it off.
	met_at TIMESTAMPTZ,
	-- A group goes one place next, so recording a new plan retires the old one
	-- rather than queueing behind it.
	superseded_at TIMESTAMPTZ
);

-- At most one open plan per group. Met and superseded plans are history and
-- accumulate freely, which is why this is partial rather than a plain unique.
CREATE UNIQUE INDEX upgrade_plans_one_open_per_group
	ON upgrade_plans (group_id)
	WHERE met_at IS NULL AND superseded_at IS NULL;

CREATE INDEX upgrade_plans_group ON upgrade_plans (group_id, created_at DESC);
