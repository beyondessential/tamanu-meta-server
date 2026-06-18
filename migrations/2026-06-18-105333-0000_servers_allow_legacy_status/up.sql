-- Per-server opt-in for the retired legacy `/status` format (a push with no
-- `health` array). Off by default: the new format is required, and a legacy
-- push is rejected with 400 unless an operator flips this on for a server
-- whose reporter hasn't been upgraded yet. When allowed, a legacy push only
-- refreshes reachability — it carries the server's last known healthchecks
-- forward rather than wiping them — so a server straddling both endpoints
-- doesn't flap its health issues. Drop the column (and the legacy path) once
-- every server is on the new format.
ALTER TABLE servers ADD COLUMN allow_legacy_status BOOLEAN NOT NULL DEFAULT FALSE;
