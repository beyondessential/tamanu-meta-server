-- Self-alert notifications are not tied to an incident.
ALTER TABLE slack_outbox ALTER COLUMN incident_id DROP NOT NULL;
