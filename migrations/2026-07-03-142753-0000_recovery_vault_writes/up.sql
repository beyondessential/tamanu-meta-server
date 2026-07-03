-- Records of successful recovery vault writes: each time the backups pod
-- encrypts and PUTs a fresh state.age snapshot to the vault bucket, we record
-- when and how large the ciphertext was. Surfaced in the recovery vault
-- settings page so operators can see the vault is actually being kept fresh.
CREATE TABLE recovery_vault_writes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    written_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    bytes BIGINT NOT NULL
);
