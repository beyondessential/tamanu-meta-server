-- Drop the machine grain.
--
-- Lossless for anything that existed before the migration: every machine-ish
-- value was copied from `applications` and left there, so dropping `machines`
-- discards only the machine rows themselves. Machines created after the
-- migration, and any notes or tags written against a machine, are lost.

DROP INDEX IF EXISTS applications_machine_id;
ALTER TABLE applications ALTER COLUMN machine_id DROP DEFAULT;
DROP FUNCTION IF EXISTS application_default_machine();
ALTER TABLE applications DROP COLUMN machine_id;
DROP TABLE machines;
