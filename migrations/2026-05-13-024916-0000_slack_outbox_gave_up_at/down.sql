DROP INDEX slack_outbox_pending;
CREATE INDEX slack_outbox_pending
	ON slack_outbox (created_at)
	WHERE delivered_at IS NULL;

ALTER TABLE slack_outbox DROP COLUMN gave_up_at;
