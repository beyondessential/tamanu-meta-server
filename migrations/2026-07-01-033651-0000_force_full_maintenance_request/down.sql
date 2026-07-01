alter table server_group_backup_config
    drop column force_full_maintenance_at,
    drop column force_full_maintenance_by;
