-- Backfill registered_at for servers enrolled before the column existed
-- (2026-06-01 add_server_archival introduced it with no backfill, so every
-- pre-existing server showed as "hasn't checked in yet" in the UI despite
-- reporting statuses).
--
-- Evidence of enrollment: an attached device, or any status row (covers
-- servers whose device was later deleted — that nulls device_id but the
-- statuses remain). Best-effort timestamp: first status push, else the
-- device's creation, else now.
UPDATE servers s
SET registered_at = COALESCE(
	(SELECT MIN(st.created_at) FROM statuses st WHERE st.server_id = s.id),
	(SELECT d.created_at FROM devices d WHERE d.id = s.device_id),
	NOW()
)
WHERE s.registered_at IS NULL
	AND (
		s.device_id IS NOT NULL
		OR EXISTS (SELECT 1 FROM statuses st WHERE st.server_id = s.id)
	);
