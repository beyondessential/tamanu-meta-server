CREATE TABLE statuses (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	server_id UUID NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
	latency_ms INT4,
	version TEXT,
	error TEXT
);
