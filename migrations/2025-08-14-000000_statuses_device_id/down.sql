DROP INDEX IF EXISTS statuses_device_id_idx;

ALTER TABLE statuses
  DROP CONSTRAINT IF EXISTS statuses_device_id_fkey;

ALTER TABLE statuses
  DROP COLUMN IF EXISTS device_id;
