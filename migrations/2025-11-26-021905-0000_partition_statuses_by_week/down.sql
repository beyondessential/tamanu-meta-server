-- Revert partitioning of statuses table back to a regular table
--
-- This migration reverts the partitioned statuses table back to a single table.
-- It preserves all data during the conversion.

-- Step 1: Create temporary regular table
CREATE TABLE statuses_regular (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at timestamptz NOT NULL DEFAULT now(),
    server_id uuid NOT NULL,
    version text,
    extra jsonb NOT NULL DEFAULT '{}'::jsonb,
    device_id uuid
);

-- Step 2: Copy all data from partitioned table to regular table
INSERT INTO statuses_regular (id, created_at, server_id, version, extra, device_id)
SELECT id, created_at, server_id, version, extra, device_id
FROM statuses;

-- Step 3: Drop the partitioned table (this drops all partitions too)
DROP TABLE statuses;

-- Step 4: Rename the regular table to statuses
ALTER TABLE statuses_regular RENAME TO statuses;

-- Step 5: Add foreign key constraints
ALTER TABLE statuses ADD CONSTRAINT statuses_device_id_fkey
    FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE SET NULL;

ALTER TABLE statuses ADD CONSTRAINT statuses_server_id_fkey
    FOREIGN KEY (server_id) REFERENCES servers(id) ON DELETE CASCADE;

-- Step 6: Recreate all indexes
CREATE INDEX statuses_created_at ON statuses USING brin (created_at);
CREATE INDEX statuses_created_at_btree ON statuses USING btree (created_at);
CREATE INDEX statuses_device_id_idx ON statuses USING btree (device_id);
CREATE INDEX statuses_extra ON statuses USING gin (extra);
CREATE INDEX statuses_id_hash ON statuses USING hash (id);
CREATE INDEX statuses_server_id ON statuses USING btree (server_id);

-- Step 7: Analyze the table to update statistics
ANALYZE statuses;
