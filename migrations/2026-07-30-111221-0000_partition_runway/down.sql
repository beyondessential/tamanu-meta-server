-- Back to the per-table functions, each creating partitions with CREATE TABLE
-- ... PARTITION OF (ACCESS EXCLUSIVE on the parent) and analysing the whole
-- table afterwards.

DROP FUNCTION IF EXISTS partition_runway();
DROP FUNCTION IF EXISTS create_statuses_partitions(INTEGER);
DROP FUNCTION IF EXISTS create_device_connections_partitions(INTEGER);
DROP FUNCTION IF EXISTS ensure_weekly_partitions(TEXT, INTEGER);

CREATE OR REPLACE FUNCTION create_statuses_partitions(weeks_ahead INTEGER DEFAULT 8)
RETURNS TABLE(partition_name TEXT, week_start DATE, week_end DATE, action TEXT)
LANGUAGE plpgsql
AS $$
DECLARE
    v_week_start DATE;
    v_week_end DATE;
    v_partition_name TEXT;
    v_year INT;
    v_week_num INT;
    v_i INT;
BEGIN
    -- Generate partitions for the next N weeks
    FOR v_i IN 0..weeks_ahead-1 LOOP
        -- Calculate the start of the week (Monday)
        v_week_start := DATE_TRUNC('week', CURRENT_DATE + (v_i * INTERVAL '1 week'))::DATE;
        v_week_end := v_week_start + INTERVAL '7 days';

        -- Extract year and ISO week number
        v_year := EXTRACT(ISOYEAR FROM v_week_start);
        v_week_num := EXTRACT(WEEK FROM v_week_start);

        -- Format partition name (e.g., statuses_2025w47)
        v_partition_name := FORMAT('statuses_%sw%s', v_year, LPAD(v_week_num::TEXT, 2, '0'));

        -- Check if partition already exists
        IF NOT EXISTS (
            SELECT 1
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relname = v_partition_name
            AND n.nspname = 'public'
        ) THEN
            -- Create the partition
            EXECUTE FORMAT(
                'CREATE TABLE %I PARTITION OF statuses FOR VALUES FROM (%L) TO (%L)',
                v_partition_name,
                v_week_start,
                v_week_end
            );

            partition_name := v_partition_name;
            week_start := v_week_start;
            week_end := v_week_end;
            action := 'created';
            RETURN NEXT;
        ELSE
            partition_name := v_partition_name;
            week_start := v_week_start;
            week_end := v_week_end;
            action := 'already_exists';
            RETURN NEXT;
        END IF;
    END LOOP;

    -- Analyze the table to update statistics
    EXECUTE 'ANALYZE statuses';
END;
$$;

CREATE OR REPLACE FUNCTION create_device_connections_partitions(weeks_ahead INTEGER DEFAULT 8)
RETURNS TABLE(partition_name TEXT, week_start DATE, week_end DATE, action TEXT)
LANGUAGE plpgsql
AS $$
DECLARE
    v_week_start DATE;
    v_week_end DATE;
    v_partition_name TEXT;
    v_year INT;
    v_week_num INT;
    v_i INT;
BEGIN
    -- Generate partitions for the next N weeks
    FOR v_i IN 0..weeks_ahead-1 LOOP
        -- Calculate the start of the week (Monday)
        v_week_start := DATE_TRUNC('week', CURRENT_DATE + (v_i * INTERVAL '1 week'))::DATE;
        v_week_end := v_week_start + INTERVAL '7 days';

        -- Extract year and ISO week number
        v_year := EXTRACT(ISOYEAR FROM v_week_start);
        v_week_num := EXTRACT(WEEK FROM v_week_start);

        -- Format partition name (e.g., device_connections_2025w47)
        v_partition_name := FORMAT('device_connections_%sw%s', v_year, LPAD(v_week_num::TEXT, 2, '0'));

        -- Check if partition already exists
        IF NOT EXISTS (
            SELECT 1
            FROM pg_class c
            JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE c.relname = v_partition_name
            AND n.nspname = 'public'
        ) THEN
            -- Create the partition
            EXECUTE FORMAT(
                'CREATE TABLE %I PARTITION OF device_connections FOR VALUES FROM (%L) TO (%L)',
                v_partition_name,
                v_week_start,
                v_week_end
            );

            partition_name := v_partition_name;
            week_start := v_week_start;
            week_end := v_week_end;
            action := 'created';
            RETURN NEXT;
        ELSE
            partition_name := v_partition_name;
            week_start := v_week_start;
            week_end := v_week_end;
            action := 'already_exists';
            RETURN NEXT;
        END IF;
    END LOOP;

    -- Analyze the table to update statistics
    EXECUTE 'ANALYZE device_connections';
END;
$$;
