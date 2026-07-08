DROP INDEX incidents_open_global;

-- Canopy-wide incidents have no group to return to; they must go before
-- the column can be NOT NULL again.
DELETE FROM incident_issues WHERE incident_id IN (
	SELECT id FROM incidents WHERE server_group_id IS NULL
);
DELETE FROM incident_notes WHERE incident_id IN (
	SELECT id FROM incidents WHERE server_group_id IS NULL
);
DELETE FROM incidents WHERE server_group_id IS NULL;

ALTER TABLE incidents ALTER COLUMN server_group_id SET NOT NULL;
