-- Check state records whether its policy escalates: an effective failure
-- of an escalating check notifies immediately, bypassing incident grace.
-- Stamped from the catalog on every filing, so incident semantics can key
-- on (effective_result, escalates) instead of the severity vocabulary.
-- Backfill from the transitional mapping's only escalation severity.
ALTER TABLE issues ADD COLUMN escalates BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE issues SET escalates = TRUE WHERE severity = 'critical';
