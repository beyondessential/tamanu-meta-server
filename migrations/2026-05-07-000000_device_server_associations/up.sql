-- Denormalised lookup table for the (device_id, server_id) pairs ever observed
-- in `statuses`. Maintained by an AFTER INSERT trigger so it stays in sync, and
-- backfilled once from the existing statuses history.
--
-- Replaces `SELECT DISTINCT server_id FROM statuses WHERE device_id = ?`, which
-- scans every weekly partition and runs >2 minutes for old devices in prod.

CREATE TABLE device_server_associations (
    device_id UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    server_id UUID NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    first_seen TIMESTAMPTZ NOT NULL,
    last_seen TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (device_id, server_id)
);

CREATE INDEX device_server_associations_server_id ON device_server_associations (server_id);

CREATE OR REPLACE FUNCTION upsert_device_server_association() RETURNS TRIGGER
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

-- Row-level triggers on a partitioned parent propagate to all current and
-- future partitions in PG13+, so cron-created weekly partitions inherit it.
CREATE TRIGGER statuses_upsert_device_server_association
    AFTER INSERT ON statuses
    FOR EACH ROW
    EXECUTE FUNCTION upsert_device_server_association();

-- One-time backfill from existing statuses. Single full scan + grouped insert;
-- expected to run for several minutes on prod but is a one-shot cost.
INSERT INTO device_server_associations (device_id, server_id, first_seen, last_seen)
SELECT device_id, server_id, MIN(created_at), MAX(created_at)
FROM statuses
WHERE device_id IS NOT NULL
GROUP BY device_id, server_id
ON CONFLICT (device_id, server_id) DO NOTHING;
