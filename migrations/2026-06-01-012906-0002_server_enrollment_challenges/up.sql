-- Proof-of-possession challenges. `begin` issues a short-lived nonce bound to
-- the presented public key and the token it is for; `complete` verifies a
-- signature over the nonce against that key, then takes the challenge
-- (single-use) and consumes the token. The nonce is one-shot and not a reusable
-- secret, so it is stored and compared as-is.
CREATE TABLE server_enrollment_challenges (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id   UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL,
    public_key  BYTEA NOT NULL,
    nonce       BYTEA NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    used_at     TIMESTAMPTZ
);
CREATE INDEX server_enrollment_challenges_lookup ON server_enrollment_challenges (server_id, nonce);
