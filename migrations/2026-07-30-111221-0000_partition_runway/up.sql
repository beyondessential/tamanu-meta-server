-- Weekly-partition provisioning that doesn't lock out the history it extends,
-- plus the runway reading the self-alert is filed from.
--
-- spec: HST
--
-- Three changes of substance over the per-table functions this replaces:
--
--  * Partitions are built detached and then ATTACHed. `CREATE TABLE ...
--    PARTITION OF` takes ACCESS EXCLUSIVE on the parent, so it waits behind
--    every in-flight query on the history and queues all new readers and
--    writers behind itself while it waits — a convoy risk on a table with
--    known multi-minute queries. `ALTER TABLE ... ATTACH PARTITION` takes
--    SHARE UPDATE EXCLUSIVE, which conflicts with neither SELECT nor INSERT.
--  * An advisory lock single-flights callers, so the in-process maintenance
--    loop and any external caller can't both pass the existence check and
--    then collide on CREATE.
--  * The trailing fleet-wide `ANALYZE` is gone. It re-analysed the entire
--    history (tens of gigabytes across a hundred-odd partitions) on every
--    run, to no purpose: the partition it had just created is empty.
--    Autovacuum analyses partitions as they fill.

-- Provision `weeks_ahead` weeks of future partitions for a range-partitioned
-- table, plus the current week. Idempotent, and safe to call concurrently.
CREATE OR REPLACE FUNCTION ensure_weekly_partitions(
    parent TEXT,
    weeks_ahead INTEGER DEFAULT 4
)
RETURNS TABLE(partition_name TEXT, week_start DATE, week_end DATE, action TEXT)
LANGUAGE plpgsql
AS $$
DECLARE
    -- Arbitrary, but canopy-specific: this database is shared with other
    -- services, and advisory locks are per-database.
    c_lock_key CONSTANT BIGINT := 6819230114770001;
    v_week_start DATE;
    v_week_end DATE;
    v_name TEXT;
    v_i INT;
    v_attached BOOLEAN;
    v_exists BOOLEAN;
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_class c
        JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE c.relname = parent AND n.nspname = 'public' AND c.relkind = 'p'
    ) THEN
        RAISE EXCEPTION 'ensure_weekly_partitions: % is not a partitioned table', parent;
    END IF;

    -- Whoever holds this is doing the same idempotent work, so a caller that
    -- can't take it has nothing to do. Held to end of transaction.
    IF NOT pg_try_advisory_xact_lock(c_lock_key) THEN
        RETURN;
    END IF;

    -- Never queue: every statement below is idempotent and runs again shortly,
    -- so failing fast beats waiting behind a long reader.
    SET LOCAL lock_timeout = '5s';

    FOR v_i IN 0..weeks_ahead LOOP
        v_week_start := DATE_TRUNC('week', CURRENT_DATE + (v_i * INTERVAL '1 week'))::DATE;
        v_week_end := v_week_start + INTERVAL '7 days';
        v_name := FORMAT(
            '%s_%sw%s',
            parent,
            EXTRACT(ISOYEAR FROM v_week_start),
            LPAD(EXTRACT(WEEK FROM v_week_start)::TEXT, 2, '0')
        );

        -- Attachment, not mere existence: a run that created the table and
        -- then failed to attach it leaves a table of the right name that
        -- nothing routes to. Checking the name alone would call that done and
        -- leave the week permanently unwritable.
        SELECT EXISTS (
            SELECT 1
            FROM pg_inherits i
            JOIN pg_class c ON c.oid = i.inhrelid
            JOIN pg_class p ON p.oid = i.inhparent
            WHERE p.relname = parent AND c.relname = v_name
        ) INTO v_attached;

        partition_name := v_name;
        week_start := v_week_start;
        week_end := v_week_end;

        IF v_attached THEN
            action := 'already_exists';
            RETURN NEXT;
            CONTINUE;
        END IF;

        SELECT EXISTS (
            SELECT 1 FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relname = v_name AND n.nspname = 'public'
        ) INTO v_exists;

        -- Each week in its own subtransaction: one week failing (a lock
        -- timeout, most likely) must not undo the weeks already provisioned
        -- in this call, nor stop the ones after it.
        BEGIN
            IF NOT v_exists THEN
                EXECUTE FORMAT(
                    'CREATE TABLE %I (LIKE %I INCLUDING DEFAULTS INCLUDING CONSTRAINTS INCLUDING STORAGE)',
                    v_name, parent
                );
            END IF;

            EXECUTE FORMAT(
                'ALTER TABLE %I ATTACH PARTITION %I FOR VALUES FROM (%L) TO (%L)',
                parent, v_name, v_week_start, v_week_end
            );

            action := CASE WHEN v_exists THEN 'attached' ELSE 'created' END;
        EXCEPTION WHEN OTHERS THEN
            action := FORMAT('failed: %s', SQLERRM);
        END;

        RETURN NEXT;
    END LOOP;
END;
$$;

-- How much future range each range-partitioned table has left. `covered_to`
-- is the exclusive upper bound of its last partition, so a table covered to
-- tomorrow has one day of runway. DEFAULT partitions carry no bound and are
-- ignored here.
CREATE OR REPLACE FUNCTION partition_runway()
RETURNS TABLE(parent TEXT, partitions BIGINT, covered_to DATE, days_remaining INTEGER)
LANGUAGE sql
STABLE
AS $$
    SELECT
        p.relname::TEXT,
        COUNT(*)::BIGINT,
        MAX(SUBSTRING(pg_get_expr(c.relpartbound, c.oid) FROM 'TO \(''(\d{4}-\d{2}-\d{2})'))::DATE,
        (MAX(SUBSTRING(pg_get_expr(c.relpartbound, c.oid) FROM 'TO \(''(\d{4}-\d{2}-\d{2})'))::DATE
            - CURRENT_DATE)::INTEGER
    FROM pg_class p
    JOIN pg_namespace n ON n.oid = p.relnamespace
    JOIN pg_inherits i ON i.inhparent = p.oid
    JOIN pg_class c ON c.oid = i.inhrelid
    WHERE p.relkind = 'p' AND n.nspname = 'public'
    GROUP BY p.relname;
$$;

-- The named entry points stay: an external schedule still calls them, and
-- their shape is a contract with it. They forward to the generic one.
CREATE OR REPLACE FUNCTION create_statuses_partitions(weeks_ahead INTEGER DEFAULT 8)
RETURNS TABLE(partition_name TEXT, week_start DATE, week_end DATE, action TEXT)
LANGUAGE plpgsql
AS $$
BEGIN
    RETURN QUERY SELECT * FROM ensure_weekly_partitions('statuses', weeks_ahead);
END;
$$;

CREATE OR REPLACE FUNCTION create_device_connections_partitions(weeks_ahead INTEGER DEFAULT 8)
RETURNS TABLE(partition_name TEXT, week_start DATE, week_end DATE, action TEXT)
LANGUAGE plpgsql
AS $$
BEGIN
    RETURN QUERY SELECT * FROM ensure_weekly_partitions('device_connections', weeks_ahead);
END;
$$;

COMMENT ON FUNCTION ensure_weekly_partitions(TEXT, INTEGER) IS
    'Provisions the current week plus N future weekly partitions of a range-partitioned table. Idempotent, concurrency-safe, and does not block reads or writes of the table.';

COMMENT ON FUNCTION partition_runway() IS
    'Future range remaining per range-partitioned table: partition count, exclusive upper bound of the last partition, and days from today to that bound.';

COMMENT ON FUNCTION create_statuses_partitions(INTEGER) IS
    'Provisions weekly partitions for the statuses table. Canopy maintains these itself while running; this entry point exists for external callers.';

COMMENT ON FUNCTION create_device_connections_partitions(INTEGER) IS
    'Provisions weekly partitions for the device_connections table. Canopy maintains these itself while running; this entry point exists for external callers.';
