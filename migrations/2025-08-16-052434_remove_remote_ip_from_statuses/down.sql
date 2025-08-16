-- Re-add the remote_ip column to statuses table
ALTER TABLE statuses ADD COLUMN remote_ip INET;

-- Recreate the index on remote_ip
CREATE INDEX statuses_remote_ip ON statuses (remote_ip);
