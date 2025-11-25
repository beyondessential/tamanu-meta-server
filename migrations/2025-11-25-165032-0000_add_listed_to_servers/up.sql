ALTER TABLE servers ADD COLUMN listed BOOLEAN NOT NULL DEFAULT false;

UPDATE servers SET listed = true;

CREATE OR REPLACE VIEW ordered_servers AS (
	select * from servers order by rank_order(rank), servers.name
);
