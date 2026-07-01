-- The untrusted role is retired: devices are never auto-created, and a device
-- is only recorded when an operator provisions or attaches it, always at a real
-- role. Reassign any remaining untrusted rows to `server`:
--   * tailscale-precreated devices bound to a server become the servers they
--     were always meant to be;
--   * unbound leftovers keep a valid role but stay inert — with no active key
--     and no tailnet identity they cannot authenticate, and a server-role
--     device that isn't bound to any server can reach no endpoint.
-- (The earlier cleanup migration already deleted the unbound, active-keyed
-- auto-discovery cruft.)
UPDATE devices SET role = 'server' WHERE role = 'untrusted';

-- Every insert now specifies a role explicitly; drop the untrusted default so a
-- missing role fails loudly instead of silently minting an untrusted device.
ALTER TABLE devices ALTER COLUMN role DROP DEFAULT;
