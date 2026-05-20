-- Enforce at most one open incident per server. Promotes the existing
-- partial index to UNIQUE so two concurrent event pushes for the same
-- server can no longer both insert a fresh incident row.

DROP INDEX incidents_open_by_server;
CREATE UNIQUE INDEX incidents_open_by_server ON incidents (server_id) WHERE closed_at IS NULL;
