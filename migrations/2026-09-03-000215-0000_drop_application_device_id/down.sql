ALTER TABLE applications ADD COLUMN device_id UUID REFERENCES devices (id) ON UPDATE CASCADE ON DELETE SET NULL;

-- Restore the column from the machine each application runs on. A box with
-- several workloads can only give its device to one of them, so the oldest
-- live application takes it and the rest stay null, which is the state the
-- column could hold before the split.
UPDATE applications a
SET device_id = m.device_id
FROM machines m
WHERE a.machine_id = m.id
  AND m.device_id IS NOT NULL
  AND a.id = (
    SELECT b.id FROM applications b
    WHERE b.machine_id = m.id AND b.deleted_at IS NULL
    ORDER BY b.created_at, b.id
    LIMIT 1
  );

CREATE UNIQUE INDEX applications_device_id_unique ON applications (device_id) WHERE device_id IS NOT NULL;
CREATE INDEX servers_device ON applications USING btree (device_id);
