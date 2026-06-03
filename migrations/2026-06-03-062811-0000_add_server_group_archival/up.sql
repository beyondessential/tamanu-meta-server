-- Group archival (soft-delete), mirroring servers.deleted_at. NULL = live.
ALTER TABLE server_groups ADD COLUMN deleted_at TIMESTAMPTZ;
