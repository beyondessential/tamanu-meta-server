-- Create device_keys table to hold the key data separately from devices
CREATE TABLE device_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    device_id UUID NOT NULL REFERENCES devices (id) ON DELETE CASCADE ON UPDATE CASCADE,
    key_data BYTEA NOT NULL UNIQUE,
    name TEXT, -- Optional name/description for the key
    is_active BOOLEAN NOT NULL DEFAULT true
);

SELECT diesel_manage_updated_at('device_keys');
CREATE INDEX device_keys_create ON device_keys (created_at DESC);
CREATE INDEX device_keys_update ON device_keys (updated_at DESC);
CREATE INDEX device_keys_device ON device_keys (device_id);
CREATE INDEX device_keys_key ON device_keys USING HASH (key_data);
CREATE INDEX device_keys_active ON device_keys (is_active) WHERE is_active = true;

-- Migrate existing key_data from devices to device_keys
INSERT INTO device_keys (device_id, key_data, name)
SELECT id, key_data, 'Initial Key'
FROM devices
WHERE key_data IS NOT NULL;

-- Remove the key_data column from devices table
-- First drop the unique constraint and index
DROP INDEX devices_key;
ALTER TABLE devices DROP COLUMN key_data;

-- Rebuild the devices table indices without the key column
