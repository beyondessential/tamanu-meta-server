-- Close-side grace ("linger"). When an incident's last effective failure
-- leaves, the incident lingers — `closing_at` records when — instead of
-- closing outright, so a failure returning within the window continues the
-- same incident rather than opening (and re-notifying) a new one. The
-- linger sweep closes incidents whose `closing_at` has outlived their
-- group's `slack_close_delay`, backdating `closed_at` to `closing_at`.
ALTER TABLE incidents
	ADD COLUMN closing_at TIMESTAMPTZ;

-- Per-group linger window, alongside `slack_open_delay`. Set to
-- `INTERVAL '0'` to close (and ship resolves) immediately, as before.
ALTER TABLE server_groups
	ADD COLUMN slack_close_delay INTERVAL NOT NULL DEFAULT INTERVAL '5 minutes'
		CHECK (slack_close_delay >= INTERVAL '0');
