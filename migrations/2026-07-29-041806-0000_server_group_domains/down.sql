ALTER TABLE servers
	DROP COLUMN may_manage_tls,
	DROP COLUMN may_manage_dns;

DROP TABLE server_group_domains;
