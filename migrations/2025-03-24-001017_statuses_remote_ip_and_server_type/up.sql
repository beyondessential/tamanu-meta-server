ALTER TABLE statuses
	ADD COLUMN remote_ip INET,
	ADD COLUMN server_type TEXT,
	ADD COLUMN extra JSONB NOT NULL DEFAULT '{}';
CREATE INDEX statuses_remote_ip ON statuses (remote_ip);
CREATE INDEX statuses_server_type ON statuses (server_type);
CREATE INDEX statuses_extra ON statuses USING gin (extra);
