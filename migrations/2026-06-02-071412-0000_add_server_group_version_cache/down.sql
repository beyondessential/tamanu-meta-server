DROP TRIGGER IF EXISTS statuses_update_server_group_effective_version ON statuses;
DROP FUNCTION IF EXISTS update_server_group_effective_version();
DROP INDEX IF EXISTS server_groups_version_server_id;
ALTER TABLE server_groups
    DROP COLUMN IF EXISTS effective_version,
    DROP COLUMN IF EXISTS version_server_id;
