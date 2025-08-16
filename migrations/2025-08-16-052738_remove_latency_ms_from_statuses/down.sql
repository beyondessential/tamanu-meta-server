-- Re-add the latency_ms column to statuses table
ALTER TABLE statuses ADD COLUMN latency_ms INTEGER;
