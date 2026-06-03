-- Recreate the pruner as it was before this migration. The deleted device rows
-- cannot be restored (one-way data cleanup).
CREATE OR REPLACE FUNCTION prune_untrusted_devices()
RETURNS void
LANGUAGE SQL
AS $$
	DELETE FROM devices
	WHERE devices.role = 'untrusted'
	AND created_at < (NOW() - '1 week'::interval);
$$;
