-- Operator-requested one-off full maintenance run for a group, honored by the
-- scheduler on its next tick (bypassing the cadence jitter slot). At most one
-- pending request per group; cleared when the run is spawned or cancelled.
alter table server_group_backup_config
    add column force_full_maintenance_at timestamptz,
    add column force_full_maintenance_by text;
