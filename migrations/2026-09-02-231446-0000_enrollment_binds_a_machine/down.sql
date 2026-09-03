ALTER INDEX machine_enrollment_challenges_lookup RENAME TO server_enrollment_challenges_lookup;
ALTER INDEX machine_enrollment_tokens_machine RENAME TO server_enrollment_tokens_server;

ALTER TABLE machine_enrollment_challenges
	DROP CONSTRAINT machine_enrollment_challenges_machine_id_fkey,
	ADD CONSTRAINT server_enrollment_challenges_server_id_fkey
		FOREIGN KEY (machine_id) REFERENCES applications (id) ON DELETE CASCADE;

ALTER TABLE machine_enrollment_tokens
	DROP CONSTRAINT machine_enrollment_tokens_machine_id_fkey,
	ADD CONSTRAINT server_enrollment_tokens_server_id_fkey
		FOREIGN KEY (machine_id) REFERENCES applications (id) ON DELETE CASCADE;

ALTER TABLE machine_enrollment_challenges RENAME COLUMN machine_id TO server_id;
ALTER TABLE machine_enrollment_tokens RENAME COLUMN machine_id TO server_id;

ALTER TABLE machine_enrollment_challenges RENAME TO server_enrollment_challenges;
ALTER TABLE machine_enrollment_tokens RENAME TO server_enrollment_tokens;
