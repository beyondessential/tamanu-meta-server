-- Enrolment binds a machine, never an application.
--
-- An enrolment admits a box. The operator creates the machine, mints a ticket
-- for it, and the agent presents that ticket to bind its identity to the box.
-- Applications are created by what that agent then reports, so an enrolment
-- that required one could never happen: nothing can report until it has
-- enrolled, and nothing creates an application but a report.
--
-- Every application is 1:1 with a machine that carries its id, so the rows
-- move across unchanged and every ticket already in the field keeps working.

ALTER TABLE server_enrollment_tokens RENAME TO machine_enrollment_tokens;
ALTER TABLE server_enrollment_challenges RENAME TO machine_enrollment_challenges;

ALTER TABLE machine_enrollment_tokens RENAME COLUMN server_id TO machine_id;
ALTER TABLE machine_enrollment_challenges RENAME COLUMN server_id TO machine_id;

ALTER TABLE machine_enrollment_tokens
	DROP CONSTRAINT server_enrollment_tokens_server_id_fkey,
	ADD CONSTRAINT machine_enrollment_tokens_machine_id_fkey
		FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE CASCADE;

ALTER TABLE machine_enrollment_challenges
	DROP CONSTRAINT server_enrollment_challenges_server_id_fkey,
	ADD CONSTRAINT machine_enrollment_challenges_machine_id_fkey
		FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE CASCADE;

ALTER INDEX server_enrollment_tokens_server RENAME TO machine_enrollment_tokens_machine;
ALTER INDEX server_enrollment_challenges_lookup RENAME TO machine_enrollment_challenges_lookup;
