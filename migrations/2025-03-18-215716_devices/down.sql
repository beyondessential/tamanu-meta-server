DROP FUNCTION prune_untrusted_devices()
DROP TABLE device_trust;
ALTER TABLE servers DROP COLUMN owner;
DROP TABLE device_connections;
DROP TABLE devices;
DROP TYPE device_role;
