DELETE FROM slack_outbox WHERE incident_id IS NULL;
ALTER TABLE slack_outbox ALTER COLUMN incident_id SET NOT NULL;
