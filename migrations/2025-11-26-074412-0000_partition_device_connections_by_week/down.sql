-- Revert partitioning of device_connections table back to a regular table
--
-- This migration reverts the partitioned device_connections table back to a single table.
-- It preserves all data during the conversion.

-- Step 1: Create temporary regular table
CREATE TABLE device_connections_regular (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at timestamptz NOT NULL DEFAULT now(),
    device_id uuid NOT NULL,
    ip inet NOT NULL,
    user_agent text NOT NULL
);

-- Step 2: Copy all data from partitioned table to regular table
INSERT INTO device_connections_regular (id, created_at, device_id, ip, user_agent)
SELECT id, created_at, device_id, ip, user_agent
FROM device_connections;

-- Step 3: Drop the partitioned table (this drops all partitions too)
DROP TABLE device_connections;

-- Step 4: Rename the regular table to device_connections
ALTER TABLE device_connections_regular RENAME TO device_connections;

-- Step 5: Add foreign key constraint
ALTER TABLE device_connections ADD CONSTRAINT device_connections_device_id_fkey
    FOREIGN KEY (device_id) REFERENCES devices(id) ON UPDATE CASCADE ON DELETE CASCADE;

-- Step 6: Recreate all indexes
CREATE INDEX device_connections_date ON device_connections USING brin (created_at);
CREATE INDEX device_connections_device ON device_connections USING btree (device_id);

-- Step 7: Analyze the table to update statistics
ANALYZE device_connections;
