-- Canopy's own checks and operator-raised manual conditions register
-- through file_check/register, and the CHK spec says they register
-- "already reviewed". register() previously never stamped reviewed_at, so
-- existing rows for these sources sit as pending review.
--
-- Grading now hard-caps a pending-review policy's effective result at
-- warning (a never-vetted check must not open incidents). Left pending,
-- these already-intended alerts — backup staleness, restore failures,
-- tailscale key expiry, manual conditions — would be silenced. Stamp them
-- reviewed to preserve their alerting, as they should have been all along.
--
-- Device-reported checks and never-vetted alertd checks are deliberately
-- left pending: they are capped at warning until an operator reviews them.
UPDATE check_policies
SET
	reviewed_at = COALESCE(reviewed_at, first_seen),
	reviewed_by = COALESCE(reviewed_by, source)
WHERE source IN ('canopy', 'manual')
	AND reviewed_at IS NULL;
