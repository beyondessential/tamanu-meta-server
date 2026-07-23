-- Best-effort reversal: reopen the orphaned check-states this migration
-- closed, identified by the exact stamp it wrote and by still having no
-- catalog row. Rows resolved by an operator carry their login in
-- resolved_by and are left untouched.
UPDATE issues
SET
	resolved_at = NULL,
	resolved_by = NULL,
	resolved_reason = NULL
WHERE check_name IS NOT NULL
	AND resolved_by = 'system'
	AND resolved_reason = 'decommissioned'
	AND NOT EXISTS (
		SELECT 1 FROM check_policies cp
		WHERE cp.source = issues.source AND cp.check_name = issues.check_name
	);
