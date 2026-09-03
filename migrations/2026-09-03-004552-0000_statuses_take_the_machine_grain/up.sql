-- A status push describes a box and the applications on it, so the row it is
-- recorded as belongs to the box. A machine that runs no application Canopy
-- holds still reports, and had nowhere to be recorded while every status row
-- had to name an application.

ALTER TABLE statuses ADD COLUMN machine_id UUID;

ALTER TABLE statuses
    ADD CONSTRAINT statuses_machine_id_fkey
    FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE CASCADE;

ALTER TABLE statuses ALTER COLUMN server_id DROP NOT NULL;

-- Historical rows keep their application and are left without a machine
-- rather than backfilled. `statuses` is partitioned by week and carries the
-- better part of a million rows per application across ~100 partitions, so
-- stamping every one of them rewrites the whole table for a fact that is
-- already reachable by joining `applications`. Reads are per-application
-- today; the machine is written from here on, and an old row's machine is
-- its application's.
--
-- NOT VALID for the same reason: the constraint governs what is written from
-- now on, and every existing row satisfies it anyway by still having its
-- application.
ALTER TABLE statuses
    ADD CONSTRAINT statuses_has_a_target
    CHECK (server_id IS NOT NULL OR machine_id IS NOT NULL) NOT VALID;

CREATE INDEX statuses_machine_id_created_at_idx
    ON statuses (machine_id, created_at DESC);
