DROP INDEX incidents_open_by_server;
CREATE INDEX incidents_open_by_server ON incidents (server_id) WHERE closed_at IS NULL;
