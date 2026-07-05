-- Optional correlation id linking an issuance to the run it was minted for.
-- Newer clients stamp their run-uuid on the credential request so an issuance
-- pairs exactly with its reported run (for duration) and concurrent same-type
-- runs on one server stay distinct; older clients leave it NULL and fall back
-- to time-window heuristics.
ALTER TABLE backup_credential_issuances ADD COLUMN run_id uuid;
