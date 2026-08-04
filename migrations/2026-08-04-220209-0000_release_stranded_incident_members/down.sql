-- Deliberately a no-op.
--
-- Nothing distinguishes a row this migration stamped from one the close paths
-- stamped afterwards, so any revert would clear legitimate leaves along with
-- the backfill and re-strand the memberships. Leaving `left_at` set is also
-- harmless to the older code, which only ever asked whether it was NULL.

SELECT 1;
