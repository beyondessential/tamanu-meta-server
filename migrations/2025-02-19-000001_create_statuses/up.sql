CREATE TYPE server_type AS ENUM (
	'Tamanu Sync Server',
	'Tamanu Metadata Server',
	'Tamanu LAN Server'
);
CREATE TABLE statuses (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	server_id UUID NOT NULL REFERENCES servers(id),
	latency_ms INTEGER,
	version TEXT,
	error TEXT,
	remote_ip INET,
	server_type server_type,
	CONSTRAINT fk_server FOREIGN KEY(server_id) REFERENCES servers(id) ON
	DELETE
		CASCADE
);