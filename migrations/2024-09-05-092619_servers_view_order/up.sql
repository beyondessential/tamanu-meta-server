CREATE VIEW ordered_servers AS (
	select * from servers
	order by (CASE
		WHEN (servers.rank = 'live') THEN 0
		WHEN (servers.rank = 'prod') THEN 0
		WHEN (servers.rank = 'production') THEN 0
		WHEN (servers.rank = 'clone') THEN 1
		WHEN (servers.rank = 'demo') THEN 2
		WHEN (servers.rank = 'test') THEN 3
		WHEN (servers.rank = 'dev') THEN 4
		ELSE 5
	END), servers.name
);
