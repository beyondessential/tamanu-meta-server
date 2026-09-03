-- Restore the scaffolding default. See the up migration for why it existed and
-- what it hides.

CREATE FUNCTION application_default_machine() RETURNS UUID
LANGUAGE sql VOLATILE AS $$
	INSERT INTO machines DEFAULT VALUES RETURNING id;
$$;

ALTER TABLE applications ALTER COLUMN machine_id SET DEFAULT application_default_machine();
