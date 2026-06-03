-- Operator-first enrollment removed the mTLS-boundary auto-create path, which
-- used to mint an Untrusted device for any unknown client key. Those rows are
-- now orphaned cruft that nothing will ever promote (import_ticket is gone).
-- Delete them — but tightly scoped, because `untrusted` is also the role for:
--   * tailscale-precreated devices still bound to a server, awaiting enrollment
--     (excluded: they're referenced by servers.device_id);
--   * archival-released devices kept as history (excluded: their keys are
--     deactivated, is_active = false).
-- So the cruft is: untrusted, bound to no server, with at least one still-active
-- key. Devices that authored a version/artifact are excluded too — they aren't
-- cruft, and those FKs are ON DELETE NO ACTION so deleting them would error.
-- Child rows go with them via ON DELETE CASCADE (device_keys, device_connections,
-- device_server_associations); statuses/issues keep their rows with device_id
-- nulled (ON DELETE SET NULL).
DELETE FROM devices d
WHERE d.role = 'untrusted'
  AND NOT EXISTS (SELECT 1 FROM servers s WHERE s.device_id = d.id)
  AND EXISTS (SELECT 1 FROM device_keys k WHERE k.device_id = d.id AND k.is_active)
  AND NOT EXISTS (SELECT 1 FROM versions v WHERE v.device_id = d.id)
  AND NOT EXISTS (SELECT 1 FROM artifacts a WHERE a.device_id = d.id);

-- Drop the 7-day untrusted-device pruner. It predates operator-first enrollment;
-- now that `untrusted` also covers pending tailscale enrollments and archival-
-- released history, a blanket age-based sweep would delete legitimate rows. An
-- external cron still calls it, so removing the function turns that call into a
-- harmless "function does not exist" error instead of a destructive delete.
DROP FUNCTION IF EXISTS prune_untrusted_devices();
