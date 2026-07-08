drop index if exists statuses_server_client_created_at_idx;

alter table statuses
	drop column client;
