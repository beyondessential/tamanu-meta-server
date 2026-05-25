-- Per-group slack open cooldown. An `incident_open` Slack notice is held
-- in the outbox for this long before the drainer is allowed to ship it,
-- so a flap that opens and resolves within the window can be cancelled
-- before we tell Slack anything happened (see `cancel_pending_open`).
--
-- Default 3 minutes — matches the previous hardcoded `OPEN_DELAY`. Set to
-- `INTERVAL '0'` to ship opens immediately.
ALTER TABLE server_groups
	ADD COLUMN slack_open_delay INTERVAL NOT NULL DEFAULT INTERVAL '3 minutes'
		CHECK (slack_open_delay >= INTERVAL '0');
