DROP INDEX servers_name_management_paused;

ALTER TABLE servers
	DROP COLUMN name_management_pause_reason,
	DROP COLUMN name_management_paused_by,
	DROP COLUMN name_management_paused_at;
