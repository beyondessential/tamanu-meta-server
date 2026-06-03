DROP INDEX servers_live;
DROP INDEX servers_host_live;
ALTER TABLE servers ADD CONSTRAINT servers_host_key UNIQUE (host);
ALTER TABLE servers DROP COLUMN registered_at;
ALTER TABLE servers DROP COLUMN deleted_at;
