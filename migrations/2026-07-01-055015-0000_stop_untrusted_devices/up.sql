-- The untrusted role is retired: devices are never auto-created, and a device
-- is only recorded when an operator provisions or attaches it, always at a real
-- role. Two steps reassign the remaining untrusted rows without granting any of
-- them access they didn't have.
--
-- First, revoke any credential an untrusted row still carries. An untrusted
-- device could authenticate nowhere, but the untrust action left keys active,
-- so a bound, active-keyed row would start working the moment it became a
-- server. Deactivating those keys makes the relabel below provably inert: with
-- no active key and no tailnet identity, the row cannot authenticate under any
-- role.
UPDATE device_keys SET is_active = false
WHERE is_active
  AND device_id IN (SELECT id FROM devices WHERE role = 'untrusted');

-- Then give every untrusted row a valid role. They are now inert history —
-- an unbound or credential-less server-role device reaches no endpoint.
UPDATE devices SET role = 'server' WHERE role = 'untrusted';

-- Every insert now specifies a role explicitly; drop the untrusted default so a
-- missing role fails loudly instead of silently minting an untrusted device.
ALTER TABLE devices ALTER COLUMN role DROP DEFAULT;
