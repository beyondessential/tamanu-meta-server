-- Anchor for passphrase rotation scheduling.
--
-- The rotation scheduler had no record of when a group last rotated, so it
-- could only ask the stateless "is now inside this group's slot" question,
-- which fires on exactly one tick per period and never catches up. That let a
-- single missed tick defer a rotation by a whole period — and one was missed
-- every period, because rotation's slot landed on the same second as full
-- maintenance's and maintenance holds the group's in-flight lock.
--
-- With a persisted anchor the scheduler can use the deadline-with-catch-up
-- rule the other periodic jobs already use: due once the group's target has
-- passed, and staying due until a rotation actually happens.
--
-- NULL means "never rotated by this scheduler". Existing groups therefore
-- become due at their next target, which is the intended behaviour — their
-- passphrase is already older than a period.
ALTER TABLE server_group_backup_config
	ADD COLUMN repo_password_rotated_at TIMESTAMPTZ;

COMMENT ON COLUMN server_group_backup_config.repo_password_rotated_at IS
	'When the repo passphrase was last successfully rotated. NULL = not since this column was added. The rotation scheduler''s per-group cadence anchor.';
