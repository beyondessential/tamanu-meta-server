-- A status is attributed to a named client (the reporting agent). Existing
-- agents name none and keep reporting as `bestool`, so their streams are
-- unchanged; a second agent (e.g. seedling) reports under its own name and is
-- kept distinct per (server, client).
alter table statuses
	add column client text not null default 'bestool';

-- Latest-per-stream lookups key off (server, client); the descending
-- created_at lets "most recent for this client" be an index-only range scan.
create index statuses_server_client_created_at_idx
	on statuses (server_id, client, created_at desc);
