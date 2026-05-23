-- Split the monitoring on/off toggle out of `alert_when_down_for`, so an
-- operator can mute a server temporarily without losing the timeout setting
-- they picked for it.
--
-- Before: `alert_when_down_for INTERVAL` doubled as the toggle. Any
-- non-positive value (typically `0`) meant "off" *and* lost whatever
-- threshold the operator had set. Re-enabling required picking the value
-- again from scratch.
--
-- After: `is_monitored BOOLEAN` carries the toggle, and
-- `alert_when_down_for` is constrained to a strictly-positive duration that
-- represents the threshold to use *when* monitored. Toggling off and back
-- on preserves the threshold.

ALTER TABLE servers ADD COLUMN is_monitored BOOLEAN NOT NULL DEFAULT TRUE;

-- Carry the existing "off" semantic over (alert_when_down_for <= 0).
UPDATE servers
SET is_monitored = FALSE
WHERE alert_when_down_for <= INTERVAL '0';

-- Give muted rows a sensible threshold so the new strictly-positive
-- constraint holds. 10 minutes mirrors the column default; operators can
-- raise it later, and it doesn't take effect until they re-enable
-- monitoring.
UPDATE servers
SET alert_when_down_for = INTERVAL '10 minutes'
WHERE alert_when_down_for <= INTERVAL '0';

-- Replace the old "non-negative" check with a strict "positive" one. A
-- zero threshold is no longer a meaningful state — the boolean owns
-- that case now.
ALTER TABLE servers
	DROP CONSTRAINT servers_alert_when_down_for_check;
ALTER TABLE servers
	ADD CONSTRAINT servers_alert_when_down_for_check
	CHECK (alert_when_down_for > INTERVAL '0');
