-- An identity speaks for a box, not for a workload on it, so the device
-- belongs on `machines` and nowhere else. Every reader now resolves the
-- device through `applications.machine_id`, and enrolment binds the
-- machine outright, which leaves this column carrying nothing.
--
-- Dropping it also retires the unique index that made a device attachable
-- to exactly one application: `machines_device_id_unique` is the constraint
-- that means anything now, and it is already in place.

DROP INDEX IF EXISTS applications_device_id_unique;
DROP INDEX IF EXISTS servers_device;
ALTER TABLE applications DROP COLUMN device_id;
