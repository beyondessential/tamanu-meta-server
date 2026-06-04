DROP INDEX statuses_server_id_created_at;

CREATE INDEX statuses_server_id ON statuses USING btree (server_id);
