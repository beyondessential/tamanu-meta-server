-- Tokens embedded in the URL of the planned-upgrades calendar feed on the
-- public server. Only the SHA-256 digest of the token is stored; the feed URL
-- is shown once at minting. There is no expiry: a calendar subscription that
-- lapses stops updating without telling anyone, so a feed ends by being
-- revoked.
CREATE TABLE calendar_tokens (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	name TEXT NOT NULL,
	token_hash BYTEA NOT NULL UNIQUE,
	created_by TEXT NOT NULL,
	created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	revoked_at TIMESTAMPTZ,
	last_used_at TIMESTAMPTZ
);
