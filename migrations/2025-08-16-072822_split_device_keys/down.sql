-- Reverse the device key split by adding key_data back to devices and removing device_keys table

-- Add key_data column back to devices
ALTER TABLE devices ADD COLUMN key_data BYTEA;

-- Migrate the first active key for each device back to the devices table
-- This assumes each device should have at most one key when rolling back
UPDATE devices
SET key_data = (
    SELECT dk.key_data
    FROM device_keys dk
    WHERE dk.device_id = devices.id
    AND dk.is_active = true
    ORDER BY dk.created_at ASC
    LIMIT 1
)
WHERE EXISTS (
    SELECT 1 FROM device_keys dk
    WHERE dk.device_id = devices.id
    AND dk.is_active = true
);

-- Recreate the unique constraint and index on key_data
ALTER TABLE devices ADD CONSTRAINT devices_key_data_key UNIQUE (key_data);
CREATE INDEX devices_key ON devices USING HASH (key_data);

-- Drop the device_keys table
DROP TABLE device_keys;
