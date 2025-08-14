ALTER TABLE statuses
  ADD COLUMN device_id UUID;

ALTER TABLE statuses
  ADD CONSTRAINT statuses_device_id_fkey
  FOREIGN KEY (device_id)
  REFERENCES devices(id)
  ON DELETE SET NULL;

CREATE INDEX statuses_device_id_idx ON statuses (device_id);
