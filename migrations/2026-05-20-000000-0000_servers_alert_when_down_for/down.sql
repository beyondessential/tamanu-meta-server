ALTER TABLE servers
	ADD COLUMN alert_when_down BOOLEAN NOT NULL DEFAULT TRUE;

UPDATE servers SET alert_when_down = FALSE WHERE alert_when_down_for <= INTERVAL '0';

ALTER TABLE servers DROP COLUMN alert_when_down_for;
