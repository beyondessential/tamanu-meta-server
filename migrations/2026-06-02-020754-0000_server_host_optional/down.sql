DROP INDEX servers_host;
ALTER TABLE servers ALTER COLUMN host SET NOT NULL;
CREATE UNIQUE INDEX servers_host_live ON servers (host) WHERE deleted_at IS NULL;
