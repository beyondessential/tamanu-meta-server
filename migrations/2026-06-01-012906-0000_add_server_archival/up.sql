-- Soft-delete (archive) and enrollment-tracking columns for servers.
ALTER TABLE servers ADD COLUMN deleted_at    TIMESTAMPTZ;  -- archived when set
ALTER TABLE servers ADD COLUMN registered_at TIMESTAMPTZ;  -- set on successful enrollment

-- host stays unique among live servers, but an archived row must not block
-- recreating a server at the same host.
ALTER TABLE servers DROP CONSTRAINT servers_host_key;
CREATE UNIQUE INDEX servers_host_live ON servers (host) WHERE deleted_at IS NULL;

CREATE INDEX servers_live ON servers (deleted_at) WHERE deleted_at IS NULL;
