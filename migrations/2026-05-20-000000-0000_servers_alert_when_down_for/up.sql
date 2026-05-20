-- Replace the on/off `alert_when_down` boolean with a per-server downtime
-- threshold. Anything non-positive (`INTERVAL '0'`, negative intervals)
-- disables alerting entirely; positive durations are how long a server
-- must have failed to report for before the reachability sweep files an
-- issue.
--
-- Default 10 minutes — long enough that a typical restart or network
-- blip won't fire, short enough to catch real outages.
--
-- Backfill mapping:
--   alert_when_down = false → INTERVAL '0'           (disabled)
--   alert_when_down = true  → INTERVAL '10 minutes'  (default)
ALTER TABLE servers
	ADD COLUMN alert_when_down_for INTERVAL NOT NULL DEFAULT INTERVAL '10 minutes'
		CHECK (alert_when_down_for >= INTERVAL '0');

UPDATE servers SET alert_when_down_for = INTERVAL '0' WHERE NOT alert_when_down;

ALTER TABLE servers DROP COLUMN alert_when_down;
