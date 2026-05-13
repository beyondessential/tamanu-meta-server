-- Slack returns 2xx for "trigger accepted", not "workflow ran successfully".
-- Capture the HTTP response body alongside delivery so postmortems can run
-- from the DB alone — a previous incident left a delivered_at-stamped row
-- with no Slack message in the channel and no way to reconstruct what
-- Slack actually said.
ALTER TABLE slack_outbox ADD COLUMN last_response TEXT;
