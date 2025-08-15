DROP INDEX IF EXISTS statuses_server_id;
CREATE INDEX statuses_server_id ON statuses USING hash (server_id);

DROP INDEX IF EXISTS statuses_created_at;
CREATE INDEX statuses_created_at ON statuses USING btree (created_at);

DROP INDEX IF EXISTS servers_device;
CREATE INDEX servers_device ON servers USING hash (device_id);

DROP INDEX IF EXISTS devices_create;
CREATE INDEX devices_create ON devices USING btree (created_at DESC);

DROP INDEX IF EXISTS device_connections_device;
CREATE INDEX device_connections_device ON device_connections USING hash (device_id);

DROP INDEX IF EXISTS device_connections_date;
CREATE INDEX device_connections_date ON device_connections USING btree (created_at DESC);
