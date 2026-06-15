-- Operator-UI lifecycle columns on the per-group backup config.
--
-- `mode` distinguishes a from-birth repo (Canopy generates the passphrase →
-- escrow flow) from an imported one (operator already holds the passphrase →
-- straight to ready). `last_init_error` is set by the jobs-side init Job when
-- `kopia repository create` fails and cleared by the operator-UI on retry. The
-- `escrow_acked_*` columns stamp who acknowledged the Bitwarden-escrow reveal
-- and when, flipping the row from escrow_pending → ready.
ALTER TABLE server_group_backup_config
	ADD COLUMN mode            TEXT NOT NULL DEFAULT 'from_birth'
		CHECK (mode IN ('from_birth', 'import')),
	ADD COLUMN last_init_error TEXT,
	ADD COLUMN escrow_acked_at TIMESTAMPTZ,
	ADD COLUMN escrow_acked_by TEXT;
