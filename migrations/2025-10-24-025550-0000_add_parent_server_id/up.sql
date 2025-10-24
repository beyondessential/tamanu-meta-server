ALTER TABLE servers ADD COLUMN parent_server_id UUID REFERENCES servers(id);
CREATE INDEX servers_parent_server_id ON servers (parent_server_id);

CREATE OR REPLACE VIEW ordered_servers AS (
	select * from servers order by rank_order(rank), servers.name
);
