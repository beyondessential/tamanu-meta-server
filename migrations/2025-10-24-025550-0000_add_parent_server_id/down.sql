DROP INDEX IF EXISTS servers_parent_server_id;
ALTER TABLE servers DROP COLUMN IF EXISTS parent_server_id;

CREATE OR REPLACE VIEW ordered_servers AS (
	select * from servers order by rank_order(rank), servers.name
);
