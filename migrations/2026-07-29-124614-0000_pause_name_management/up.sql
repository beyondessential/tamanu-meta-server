-- Pausing a server's name management (CRT).
--
-- While paused, Canopy makes no new changes on the server's behalf: no
-- certificate ordered or renewed, no address record changed. Nothing already in
-- place is withdrawn — a pause is for looking into something, and taking a
-- deployment off the air is not a neutral act to perform while looking.
--
-- Revoking one of a server's certificates sets this without being asked:
-- otherwise revocation and re-issuance chase each other, and a key that leaked
-- because the host was compromised has its replacement handed to the same
-- attacker within minutes by an agent doing exactly what it was built to do.
--
-- A timestamp rather than a flag, so "paused since when" is answerable — a pause
-- everyone has forgotten is how certificates quietly expire, and the age of the
-- pause is what makes that reportable.
ALTER TABLE servers
	ADD COLUMN name_management_paused_at TIMESTAMPTZ,
	ADD COLUMN name_management_paused_by TEXT,
	ADD COLUMN name_management_pause_reason TEXT;

-- The work queues all skip paused servers, and the forgotten-pause report walks
-- them by age.
CREATE INDEX servers_name_management_paused ON servers (name_management_paused_at)
	WHERE name_management_paused_at IS NOT NULL;
