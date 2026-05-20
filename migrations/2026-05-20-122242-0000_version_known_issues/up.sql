-- Known issues attached to a version. Operators add these to flag a
-- released version with caveats; a version with no unresolved known
-- issues is considered `ready` (surfaced as a boolean in API responses).
--
-- Rows are append-only: instead of editing or deleting a known issue,
-- an operator resolves it with a `resolution_message`. The combination
-- of (description, resolution_message) becomes a small audit trail of
-- what was wrong and how it was addressed (a fix in a later patch,
-- known-good workaround, false alarm, etc).

CREATE TABLE version_known_issues (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	version_id UUID NOT NULL REFERENCES versions (id) ON DELETE CASCADE ON UPDATE CASCADE,
	author TEXT NOT NULL,
	description TEXT NOT NULL,
	resolved_at TIMESTAMP WITH TIME ZONE,
	resolved_by TEXT,
	resolution_message TEXT,
	CONSTRAINT resolved_consistency CHECK (
		(resolved_at IS NULL AND resolved_by IS NULL AND resolution_message IS NULL)
		OR (resolved_at IS NOT NULL AND resolved_by IS NOT NULL AND resolution_message IS NOT NULL)
	)
);

CREATE INDEX version_known_issues_version_created
	ON version_known_issues (version_id, created_at DESC);

-- Fast lookup of "does this version have any unresolved known issues?"
CREATE INDEX version_known_issues_open
	ON version_known_issues (version_id)
	WHERE resolved_at IS NULL;
