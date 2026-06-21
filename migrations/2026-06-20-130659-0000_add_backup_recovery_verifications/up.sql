-- Records of the recovery vault verification ceremony: each time an operator proves
-- they can decrypt a Canopy-issued challenge with a held private key, we record
-- when and against which recipient set. The ceremony is due when there is no
-- record, the latest is over a year old, or the recipient set has changed.
CREATE TABLE backup_recovery_verifications (
    id BIGSERIAL PRIMARY KEY,
    verified_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- The recipient fingerprints (age1… public keys) this verification covered,
    -- as a JSON array of strings; compared against the live set to detect a
    -- changed key set.
    recipients JSONB NOT NULL,
    CONSTRAINT recipients_is_array CHECK (jsonb_typeof(recipients) = 'array')
);
