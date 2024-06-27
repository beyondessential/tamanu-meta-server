create index statuses_server_id on statuses (server_id);
create index latest_statuses_successes on statuses (server_id, created_at desc) where error is null;
create index latest_statuses_errors on statuses (server_id, created_at desc) where error is not null;
