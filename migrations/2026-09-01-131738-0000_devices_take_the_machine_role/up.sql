-- An identity's role names what it authenticates, and a box's identity
-- authenticates a box. `server` named it when a box and the software on it were
-- one record; the role is a machine's, so the stored value says so.
--
-- Enrolment inputs keep accepting `server` as an alias, so an agent deployed
-- before the rename goes on enrolling. The alias is on the input only: what
-- Canopy stores and presents is `machine`, which is why this rewrites the rows
-- rather than teaching every reader two names.
-- See [DTR](.workhorse/specs/private-server/device-trust.md).
UPDATE devices SET role = 'machine' WHERE role = 'server';
