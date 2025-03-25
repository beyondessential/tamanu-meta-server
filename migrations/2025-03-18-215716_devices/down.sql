DROP FUNCTION prune_untrusted_devices();
ALTER TABLE servers DROP COLUMN device_id;
DROP TABLE device_connections;
DROP TABLE devices;
DROP TYPE device_role;

CREATE OR REPLACE VIEW ordered_servers AS (
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
