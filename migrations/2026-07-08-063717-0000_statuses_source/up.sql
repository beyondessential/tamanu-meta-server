-- Every status row records the source that pushed it. Pushes that name no
-- source are attributed to 'alertd' (the transitional default), and all
-- pre-source history was pushed by alertd's doctor sweep.
ALTER TABLE statuses ADD COLUMN source TEXT NOT NULL DEFAULT 'alertd';

-- Health-check issues and silences were filed under the fixed source
-- 'status'; they now live under the source that reports them, which for
-- everything to date is alertd.
UPDATE issues SET source = 'alertd' WHERE source = 'status';
UPDATE server_silenced_refs SET source = 'alertd' WHERE source = 'status';
UPDATE server_group_silenced_refs SET source = 'alertd' WHERE source = 'status';
