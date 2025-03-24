DROP INDEX statuses_extra;
DROP INDEX statuses_server_type;
DROP INDEX statuses_remote_ip;

ALTER TABLE statuses
	DROP COLUMN remote_ip,
	DROP COLUMN server_type,
	DROP COLUMN extra;
