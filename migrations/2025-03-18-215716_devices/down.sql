DROP FUNCTION prune_untrusted_devices();
ALTER TABLE servers DROP COLUMN device_id;
DROP TABLE device_connections;
DROP TABLE devices;
DROP TYPE device_role;
