-- Drop the index on remote_ip
DROP INDEX IF EXISTS statuses_remote_ip;

-- Drop the remote_ip column from statuses table
ALTER TABLE statuses DROP COLUMN remote_ip;
