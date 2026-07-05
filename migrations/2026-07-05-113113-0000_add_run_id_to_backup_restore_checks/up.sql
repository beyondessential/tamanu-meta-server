-- Optional correlation id linking a restore-health report to the credential
-- issuance it was minted for (the same run-uuid the consumer stamps on its
-- restore-credentials request), so a check pairs exactly with its issuance for
-- Canopy-measured duration. NULL for older consumers, which fall back to
-- time-window heuristics.
ALTER TABLE backup_restore_checks ADD COLUMN run_id uuid;
