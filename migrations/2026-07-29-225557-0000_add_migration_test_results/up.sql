-- A migration test's result, hanging off the restore-health report that carries
-- its common fields. Its own table rather than nullable columns on the report,
-- because the timings and sizes are the whole point of the test and a plain
-- restore report has nothing to put in them.
--
-- Neither table has `test_` in its name: `except_tables` in diesel.toml matches
-- unanchored, so a name containing it is silently left out of the generated
-- schema.
CREATE TABLE migration_tests (
	check_id BIGINT PRIMARY KEY REFERENCES backup_restore_checks (id) ON DELETE CASCADE,
	target_version_id UUID NOT NULL REFERENCES versions (id) ON DELETE CASCADE,
	total_elapsed INTERVAL NOT NULL,
	failed_migration TEXT,
	data_bytes_before BIGINT NOT NULL,
	data_bytes_after BIGINT NOT NULL
);

CREATE INDEX migration_tests_target_version ON migration_tests (target_version_id);

CREATE TABLE migration_timings (
	check_id BIGINT NOT NULL REFERENCES migration_tests (check_id) ON DELETE CASCADE,
	ordinal INTEGER NOT NULL,
	name TEXT NOT NULL,
	elapsed INTERVAL NOT NULL,
	PRIMARY KEY (check_id, ordinal)
);

-- Reading one migration's duration across every deployment that ran it is the
-- cross-row question these timings exist to answer.
CREATE INDEX migration_timings_name ON migration_timings (name);
