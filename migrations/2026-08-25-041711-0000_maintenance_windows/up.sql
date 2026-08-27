-- An operator's declaration that a server or a group is being worked on. While
-- a window suspends, every check on the target grades to skipped, so nothing on
-- it opens or joins an incident.
CREATE TABLE maintenance_windows (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	server_id UUID REFERENCES servers (id) ON DELETE CASCADE,
	server_group_id UUID REFERENCES server_groups (id) ON DELETE CASCADE,
	-- When the operator expects the work to finish. A window that reaches it
	-- ends itself, so a forgotten one never leaves a deployment unwatched.
	expected_end TIMESTAMPTZ NOT NULL,
	note TEXT,
	declared_by TEXT,
	declared_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	amended_by TEXT,
	amended_at TIMESTAMPTZ,
	-- Set when the window stops holding: the operator's lift, or the sweep
	-- stamping the expected end. `ended_by` distinguishes the two.
	ended_at TIMESTAMPTZ,
	ended_by TEXT,
	-- Stamped once the settle period after the end has elapsed and the target's
	-- issues have been re-evaluated, so that happens exactly once per window.
	settled_at TIMESTAMPTZ,
	created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	CONSTRAINT maintenance_windows_one_target CHECK (num_nonnulls(server_id, server_group_id) = 1)
);

-- At most one open window per target. Ended windows are the target's
-- maintenance history and accumulate freely, hence the partial index.
CREATE UNIQUE INDEX maintenance_windows_one_open_per_server
	ON maintenance_windows (server_id)
	WHERE ended_at IS NULL AND server_id IS NOT NULL;
CREATE UNIQUE INDEX maintenance_windows_one_open_per_group
	ON maintenance_windows (server_group_id)
	WHERE ended_at IS NULL AND server_group_id IS NOT NULL;

-- The two sweeps: windows that have reached their expected end, and ended
-- windows whose settle period has yet to be accounted for.
CREATE INDEX maintenance_windows_open
	ON maintenance_windows (expected_end)
	WHERE ended_at IS NULL;
CREATE INDEX maintenance_windows_unsettled
	ON maintenance_windows (ended_at)
	WHERE ended_at IS NOT NULL AND settled_at IS NULL;
CREATE INDEX maintenance_windows_server ON maintenance_windows (server_id, declared_at DESC);
CREATE INDEX maintenance_windows_group ON maintenance_windows (server_group_id, declared_at DESC);
