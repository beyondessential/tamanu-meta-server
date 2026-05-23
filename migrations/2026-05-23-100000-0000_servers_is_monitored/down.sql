-- Restore the "0 means off" encoding by folding `is_monitored = false` rows
-- back to `alert_when_down_for = 0`.

ALTER TABLE servers
	DROP CONSTRAINT servers_alert_when_down_for_check;
ALTER TABLE servers
	ADD CONSTRAINT servers_alert_when_down_for_check
	CHECK (alert_when_down_for >= INTERVAL '0');

UPDATE servers
SET alert_when_down_for = INTERVAL '0'
WHERE NOT is_monitored;

ALTER TABLE servers DROP COLUMN is_monitored;
