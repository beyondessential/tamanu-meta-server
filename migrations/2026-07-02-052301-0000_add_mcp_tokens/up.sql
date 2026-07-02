-- Bearer tokens for the public (internet-facing) MCP mount. Only the SHA-256
-- digest of the token is stored; the plaintext is shown once at minting.
CREATE TABLE mcp_tokens (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	name TEXT NOT NULL,
	token_hash BYTEA NOT NULL UNIQUE,
	created_by TEXT NOT NULL,
	created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	expires_at TIMESTAMPTZ NOT NULL,
	revoked_at TIMESTAMPTZ,
	last_used_at TIMESTAMPTZ
);
