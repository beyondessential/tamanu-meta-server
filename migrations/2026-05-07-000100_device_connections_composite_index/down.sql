CREATE INDEX device_connections_device ON device_connections USING btree (device_id);

DROP INDEX device_connections_device_created_at;
