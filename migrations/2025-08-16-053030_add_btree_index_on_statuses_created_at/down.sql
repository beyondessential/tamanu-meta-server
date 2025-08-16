-- This file should undo anything in `up.sql`
-- Drop the btree index on created_at column for statuses table
DROP INDEX IF EXISTS statuses_created_at_btree;
