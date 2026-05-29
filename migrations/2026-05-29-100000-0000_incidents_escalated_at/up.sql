-- Marks the first time an incident escalated from a lower severity to
-- Critical. NULL ⇒ never escalated. Used to gate the one-shot "open"
-- Slack message that fires on the first Critical contributor joining
-- an already-shipped incident.
ALTER TABLE incidents
    ADD COLUMN escalated_at TIMESTAMPTZ;
