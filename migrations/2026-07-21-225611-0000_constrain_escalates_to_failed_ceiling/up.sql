-- Escalation only makes sense at a failed ceiling: it bypasses incident
-- grace on an effective failure, and only a failed ceiling lets an
-- effective result reach failed. Below that the flag is dead config, yet
-- the policy editor let operators set it there. Drop the dead flags, then
-- constrain the column so the meaningless combination cannot recur.
UPDATE check_policies
SET escalates = FALSE
WHERE escalates AND ceiling <> 'failed';

ALTER TABLE check_policies
	ADD CONSTRAINT check_policies_escalates_needs_failed_ceiling
	CHECK (NOT escalates OR ceiling = 'failed');
