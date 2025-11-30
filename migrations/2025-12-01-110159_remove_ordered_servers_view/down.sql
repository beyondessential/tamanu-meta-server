CREATE FUNCTION rank_order(rank text) RETURNS int
LANGUAGE SQL
IMMUTABLE
PARALLEL SAFE
RETURNS NULL ON NULL INPUT
AS $$
	SELECT CASE
		WHEN (rank = 'live') THEN 0
		WHEN (rank = 'prod') THEN 0
		WHEN (rank = 'production') THEN 0
		WHEN (rank = 'clone') THEN 1
		WHEN (rank = 'demo') THEN 2
		WHEN (rank = 'test') THEN 3
		WHEN (rank = 'dev') THEN 4
		ELSE 5
	END
$$;

CREATE VIEW ordered_servers AS (
	select * from servers order by rank_order(rank), servers.name
);
