-- Remove the transition scaffolding that gave an application a machine of its
-- own whenever the caller did not name one.
--
-- It existed so `applications.machine_id` could be NOT NULL from the outset
-- rather than nullable-and-tightened-later, back when nothing wrote a machine.
-- Now the operator create flow creates the machine first and hangs the
-- application off it, and enrolment binds the identity to the machine, so
-- every production insert names its machine explicitly.
--
-- The hazard it carried, and the reason it goes now rather than later: a
-- caller that should attach an application to an EXISTING machine but omits
-- `machine_id` silently got a second machine instead of an error. That is
-- exactly wrong for a two-workload host, which is the case this whole card
-- exists to serve. With the default gone, omitting the machine is a NOT NULL
-- violation, which is what it should always have been.

ALTER TABLE applications ALTER COLUMN machine_id DROP DEFAULT;
DROP FUNCTION application_default_machine();
