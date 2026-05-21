-- Earliest time the drainer is allowed to ship this row. Open rows get
-- set this 3 minutes into the future so we can suppress the Slack notice
-- for incidents that flap open-and-resolved-immediately (very common for
-- transient probe failures). Resolve rows keep the default NOW() so they
-- ship straight away.
--
-- See `enqueue_slack_open` / the resolve enqueue path in
-- `crates/database/src/issues.rs` for the cancellation logic that pairs
-- with this: a resolve that arrives while the open is still in the
-- pre-deliver window cancels the open row outright and doesn't enqueue
-- itself either — we never told Slack about the incident, so there's
-- nothing to "resolve" there.
ALTER TABLE slack_outbox
	ADD COLUMN deliver_after TIMESTAMPTZ NOT NULL DEFAULT NOW();
