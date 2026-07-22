-- Best-effort reversal: clear the review stamp only where it looks
-- system-applied (reviewed_by equals the reserved source, as the up
-- migration and register() both stamp it). Rows an operator reviewed
-- carry their email in reviewed_by and are left untouched.
UPDATE check_policies
SET
	reviewed_at = NULL,
	reviewed_by = NULL
WHERE source IN ('canopy', 'manual')
	AND reviewed_by = source;
