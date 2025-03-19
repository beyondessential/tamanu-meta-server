CREATE TYPE device_role AS ENUM (
	'untrusted',
	'admin',
	'releaser',
	'server'
);

CREATE TABLE devices (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	key_data BYTEA NOT NULL UNIQUE,
	role device_role NOT NULL DEFAULT 'untrusted'
);

SELECT diesel_manage_updated_at('devices');
CREATE INDEX devices_create ON devices (created_at DESC);
CREATE INDEX devices_update ON devices (updated_at DESC);
CREATE INDEX devices_key ON devices USING HASH (key_data);

CREATE TABLE device_connections (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	device_id UUID NOT NULL REFERENCES devices (id) ON DELETE CASCADE ON UPDATE CASCADE,
	ip inet NOT NULL,
	user_agent TEXT
);

CREATE INDEX device_connections_date ON device_connections (created_at DESC);
CREATE INDEX device_connections_device ON device_connections USING HASH (device_id);

ALTER TABLE servers ADD COLUMN device_id UUID REFERENCES devices (id) ON DELETE SET NULL ON UPDATE CASCADE;
CREATE INDEX servers_device ON servers USING HASH (device_id);

CREATE TABLE device_trust (
	device UUID NOT NULL REFERENCES devices (id) ON DELETE CASCADE ON UPDATE CASCADE,
	trusts UUID NOT NULL REFERENCES devices (id) ON DELETE CASCADE ON UPDATE CASCADE,
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	PRIMARY KEY (device, trusts)
);

CREATE FUNCTION prune_untrusted_devices()
RETURNS void
LANGUAGE SQL
AS $$
	DELETE FROM devices
	WHERE devices.role = 'untrusted'
	AND created_at < (NOW() - '1 week'::interval);
$$;

