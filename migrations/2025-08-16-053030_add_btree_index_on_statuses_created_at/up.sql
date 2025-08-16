-- Add btree index on created_at column for statuses table
-- This complements the existing brin index for different query patterns
CREATE INDEX statuses_created_at_btree ON statuses USING btree (created_at);
