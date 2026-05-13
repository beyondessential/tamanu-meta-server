-- Terminal-state marker for slack_outbox rows the drainer has stopped
-- retrying. Previously `mark_failed` only bumped `attempts`, so a row that
-- had hit MAX_ATTEMPTS kept being re-claimed every tick — printing "giving
-- up" on every loop and growing `attempts` forever. With `gave_up_at` set,
-- the partial pending index excludes the row and `claim_pending` stops
-- seeing it.
ALTER TABLE slack_outbox ADD COLUMN gave_up_at TIMESTAMPTZ;

-- Pending index now excludes given-up rows too.
DROP INDEX slack_outbox_pending;
CREATE INDEX slack_outbox_pending
	ON slack_outbox (created_at)
	WHERE delivered_at IS NULL AND gave_up_at IS NULL;
