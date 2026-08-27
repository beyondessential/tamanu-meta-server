-- Drop `device_server_associations`.
--
-- A many-to-many between devices and servers, trigger-maintained off every
-- status insert. The identity-to-machine link is a single column on the
-- machine, so the association table has nothing left to model.
--
-- ORDER MATTERS, and Postgres will not enforce it. Dropping the table first
-- succeeds without complaint: a PL/pgSQL body is text, not a parsed
-- dependency, so nothing links the function to the table it writes. The
-- trigger would then survive its own table and fail on the next status push —
-- every status push, for every reporter. Trigger, then function, then table.
--
-- `statuses` is partitioned and the trigger lives on the parent, so one DROP
-- covers every partition and any created later.

DROP TRIGGER statuses_upsert_device_server_association ON statuses;
DROP FUNCTION upsert_device_server_association();
DROP TABLE device_server_associations;
