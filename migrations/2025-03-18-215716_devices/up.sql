CREATE TYPE device_role AS ENUM (
	'untrusted',
	'admin',
	'releaser',
	'server'
);

CREATE TABLE devices (
	public_key BYTEA PRIMARY KEY CONSTRAINT key_length CHECK (length(public_key) = 32),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	role device_role NOT NULL DEFAULT 'untrusted'
);

SELECT diesel_manage_updated_at('devices');
CREATE INDEX devices_create ON devices (created_at DESC);
CREATE INDEX devices_update ON devices (updated_at DESC);
CREATE INDEX devices_key ON devices USING HASH (public_key);

CREATE TABLE device_connections (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	device BYTEA NOT NULL REFERENCES devices (public_key) ON DELETE CASCADE ON UPDATE CASCADE,
	ip inet NOT NULL,
	tls_version TEXT,
	latency INTERVAL,
	user_agent TEXT,
	tamanu_version TEXT,
	status JSONB NOT NULL DEFAULT '{}'
);

CREATE INDEX device_connections_date ON device_connections (created_at DESC);
CREATE INDEX device_connections_device ON device_connections USING HASH (device);

ALTER TABLE servers
	ADD COLUMN owner BYTEA REFERENCES devices (public_key) ON DELETE SET NULL ON UPDATE CASCADE;

CREATE TABLE device_trust (
	device BYTEA NOT NULL REFERENCES devices (public_key) ON DELETE CASCADE ON UPDATE CASCADE,
	trusts BYTEA NOT NULL REFERENCES devices (public_key) ON DELETE CASCADE ON UPDATE CASCADE,
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

