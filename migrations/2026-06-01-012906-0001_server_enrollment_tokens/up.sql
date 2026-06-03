-- Single-use enrollment tokens. The plaintext token lives only in the blob
-- handed to the operator; we store only its SHA-256 hash. A token is "active"
-- while consumed_at IS NULL AND expires_at > now(). Reissue marks prior
-- un-consumed tokens consumed; the burn happens atomically with a successful
-- enrollment.
CREATE TABLE server_enrollment_tokens (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id   UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ
);
CREATE INDEX server_enrollment_tokens_server ON server_enrollment_tokens (server_id);
