-- Per-config opt-out of the org retention floor. When true, the floor
-- (keep_daily 7 / weekly 4 / monthly 6) is neither validated on write nor
-- enforced when the policy is resolved — for backups taken for processing that
-- we're not authorised to keep beyond a few days. Defaults false (floor applies).
ALTER TABLE backup_type_defaults
    ADD COLUMN allow_below_floor BOOLEAN NOT NULL DEFAULT false;

ALTER TABLE server_group_backup_schedule
    ADD COLUMN allow_below_floor BOOLEAN NOT NULL DEFAULT false;
