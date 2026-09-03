-- Restore the association table and the trigger that maintained it. The
-- history it held is not recoverable: it was derived from status rows as they
-- arrived, and nothing replays them.

CREATE TABLE device_server_associations (
	device_id UUID NOT NULL REFERENCES devices (id) ON DELETE CASCADE,
	server_id UUID NOT NULL REFERENCES applications (id) ON DELETE CASCADE,
	first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	last_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	PRIMARY KEY (device_id, server_id)
);

CREATE INDEX device_server_associations_server_id ON device_server_associations (server_id);

CREATE FUNCTION upsert_device_server_association() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.device_id IS NOT NULL THEN
        INSERT INTO device_server_associations (device_id, server_id, first_seen, last_seen)
        VALUES (NEW.device_id, NEW.server_id, NEW.created_at, NEW.created_at)
        ON CONFLICT (device_id, server_id) DO UPDATE
        SET last_seen = GREATEST(device_server_associations.last_seen, EXCLUDED.last_seen);
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER statuses_upsert_device_server_association
	AFTER INSERT ON statuses
	FOR EACH ROW
	EXECUTE FUNCTION upsert_device_server_association();
