DROP TRIGGER IF EXISTS statuses_upsert_device_server_association ON statuses;
DROP FUNCTION IF EXISTS upsert_device_server_association();
DROP TABLE IF EXISTS device_server_associations;
