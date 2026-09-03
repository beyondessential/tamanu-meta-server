-- A plan is for one of a group's environments: its servers at one rank. A
-- site's clone can move ahead of its production, so each environment goes its
-- own place next.
ALTER TABLE upgrade_plans ADD COLUMN rank TEXT;

-- Existing plans were about the environment the group's headline version
-- comes from, which is the rank of its canonical member. Servers still carry
-- the older spellings of production and clone, which the plan must not.
UPDATE upgrade_plans p
	SET rank = CASE s.rank
		WHEN 'live' THEN 'production'
		WHEN 'prod' THEN 'production'
		WHEN 'staging' THEN 'clone'
		ELSE s.rank
	END
	FROM server_groups g
	LEFT JOIN servers s ON s.id = g.version_server_id
	WHERE g.id = p.group_id;
UPDATE upgrade_plans SET rank = 'production' WHERE rank IS NULL;

ALTER TABLE upgrade_plans
	ALTER COLUMN rank SET NOT NULL,
	ADD CONSTRAINT upgrade_plans_rank_check
		CHECK (rank IN ('production', 'clone', 'demo', 'test', 'dev'));

-- At most one open plan per environment.
DROP INDEX upgrade_plans_one_open_per_group;

CREATE UNIQUE INDEX upgrade_plans_one_open_per_environment
	ON upgrade_plans (group_id, rank)
	WHERE met_at IS NULL AND superseded_at IS NULL AND withdrawn_at IS NULL;
