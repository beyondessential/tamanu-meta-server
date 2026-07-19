-- Events were an append-only audit log of issue state changes; nothing
-- derived state from them, and issue state itself is the durable record.
-- Statuses hold the per-check history. Unpartitioned and unpruned, the
-- table only grew; it goes away rather than gaining a retention scheme.
DROP TABLE events;
