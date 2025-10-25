-- Convert the role column from enum to text
ALTER TABLE devices
	ALTER COLUMN role TYPE TEXT USING role::TEXT;

-- Update the default value to use text
ALTER TABLE devices
	ALTER COLUMN role SET DEFAULT 'untrusted';

-- Drop the enum type (this will only work if no other tables use it)
DROP TYPE device_role;

-- Recreate the function that references the enum to use text instead
CREATE OR REPLACE FUNCTION prune_untrusted_devices()
RETURNS void
LANGUAGE SQL
AS $$
	DELETE FROM devices
	WHERE devices.role = 'untrusted'
	AND created_at < (NOW() - '1 week'::interval);
$$;
