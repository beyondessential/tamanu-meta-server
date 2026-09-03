-- Rows recorded for a machine that runs no application cannot be expressed by
-- the old shape, so they go rather than being attributed to an arbitrary one.
DELETE FROM statuses WHERE server_id IS NULL;

DROP INDEX statuses_machine_id_created_at_idx;

ALTER TABLE statuses DROP CONSTRAINT statuses_has_a_target;

ALTER TABLE statuses ALTER COLUMN server_id SET NOT NULL;

ALTER TABLE statuses DROP CONSTRAINT statuses_machine_id_fkey;

ALTER TABLE statuses DROP COLUMN machine_id;
