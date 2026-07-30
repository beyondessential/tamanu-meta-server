-- A corrected date or a reworded note is the same plan better described, so it
-- is amended in place rather than superseded. Changing the target stays a
-- replacement, since where a deployment is going is what the history records.
ALTER TABLE upgrade_plans
	ADD COLUMN amended_by TEXT,
	ADD COLUMN amended_at TIMESTAMPTZ;
