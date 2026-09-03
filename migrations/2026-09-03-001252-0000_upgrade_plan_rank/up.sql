-- A plan is for one of a group's environments: its servers at one rank. A
-- site's clone can move ahead of its production, so each environment goes its
-- own place next.
ALTER TABLE upgrade_plans ADD COLUMN rank TEXT;

-- Existing plans were about the environment the group's headline version
-- comes from, which is the rank of its canonical member.
UPDATE upgrade_plans p
	SET rank = COALESCE(s.rank, 'production')
	FROM server_groups g
	LEFT JOIN servers s ON s.id = g.version_server_id
	WHERE g.id = p.group_id;

ALTER TABLE upgrade_plans ALTER COLUMN rank SET NOT NULL;

-- At most one open plan per environment.
DROP INDEX upgrade_plans_one_open_per_group;

CREATE UNIQUE INDEX upgrade_plans_one_open_per_environment
	ON upgrade_plans (group_id, rank)
	WHERE met_at IS NULL AND superseded_at IS NULL AND withdrawn_at IS NULL;
