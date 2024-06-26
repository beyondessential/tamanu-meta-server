CREATE TABLE servers (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	name TEXT NOT NULL,
	rank TEXT NOT NULL,
	host TEXT NOT NULL
);

ALTER TABLE servers ADD CONSTRAINT servers_host_key UNIQUE (host);
SELECT diesel_manage_updated_at('servers');
