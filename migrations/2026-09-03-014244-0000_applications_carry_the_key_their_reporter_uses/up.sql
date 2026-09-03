-- A reporter names each application it found by a key of its own choosing, and
-- Canopy correlates that key to its own record. The key is the reporter's, so
-- it is unique per machine rather than per fleet, and an application Canopy
-- created from a unified push has none until a reporter claims it.
ALTER TABLE applications ADD COLUMN reported_key TEXT;

CREATE UNIQUE INDEX applications_machine_reported_key_idx
	ON applications (machine_id, reported_key)
	WHERE reported_key IS NOT NULL AND deleted_at IS NULL;
