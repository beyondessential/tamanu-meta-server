-- Strict 1:1 between server and the device that backs it.
-- Partial unique index so NULL device_id is still allowed for unprovisioned servers.
CREATE UNIQUE INDEX servers_device_id_unique ON servers (device_id) WHERE device_id IS NOT NULL;
