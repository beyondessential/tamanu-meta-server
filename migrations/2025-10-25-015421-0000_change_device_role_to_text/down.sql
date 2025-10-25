-- Recreate the device_role enum type
CREATE TYPE device_role AS ENUM (
	'untrusted',
	'admin',
	'releaser',
	'server'
);

-- Convert the role column from text back to enum
ALTER TABLE devices
	ALTER COLUMN role TYPE device_role USING role::device_role;

-- Update the default value to use the enum
ALTER TABLE devices
	ALTER COLUMN role SET DEFAULT 'untrusted'::device_role;

-- Recreate the function with the enum type
CREATE OR REPLACE FUNCTION prune_untrusted_devices()
RETURNS void
LANGUAGE SQL
AS $$
	DELETE FROM devices
	WHERE devices.role = 'untrusted'
	AND created_at < (NOW() - '1 week'::interval);
$$;
