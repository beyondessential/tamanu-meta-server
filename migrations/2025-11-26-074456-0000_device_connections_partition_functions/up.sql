-- Create partition management functions for device_connections table
--
-- These functions are called by cron jobs to automatically manage partitions

-- Function to create the next N weeks of partitions
-- Usage: SELECT create_device_connections_partitions(8);
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

-- Function to drop partitions older than a specified number of weeks
-- Usage: SELECT drop_old_device_connections_partitions(520);  -- Drop partitions older than 10 years
CREATE OR REPLACE FUNCTION drop_old_device_connections_partitions(weeks_to_keep INTEGER DEFAULT 520)
RETURNS TABLE(partition_name TEXT, action TEXT)
LANGUAGE plpgsql
AS $$
DECLARE
    v_partition_record RECORD;
    v_cutoff_date DATE;
    v_partition_year INT;
    v_partition_week INT;
    v_partition_start DATE;
BEGIN
    -- Calculate cutoff date
    v_cutoff_date := CURRENT_DATE - (weeks_to_keep * INTERVAL '1 week');

    -- Find and drop old partitions
    FOR v_partition_record IN
        SELECT tablename
        FROM pg_tables
        WHERE tablename ~ '^device_connections_\d{4}w\d{2}$'
        AND schemaname = 'public'
    LOOP
        -- Extract year and week from partition name
        v_partition_year := SUBSTRING(v_partition_record.tablename FROM 'device_connections_(\d{4})w')::INT;
        v_partition_week := SUBSTRING(v_partition_record.tablename FROM 'w(\d{2})$')::INT;

        -- Calculate approximate start date for this partition
        -- ISO week 1 starts on the Monday of the week containing Jan 4
        v_partition_start := DATE_TRUNC('week',
            MAKE_DATE(v_partition_year, 1, 4) + ((v_partition_week - 1) * INTERVAL '1 week')
        )::DATE;

        -- Drop if older than cutoff
        IF v_partition_start < v_cutoff_date THEN
            EXECUTE FORMAT('DROP TABLE IF EXISTS %I', v_partition_record.tablename);

            partition_name := v_partition_record.tablename;
            action := FORMAT('dropped (started %s, older than %s)',
                v_partition_start, v_cutoff_date);
            RETURN NEXT;
        END IF;
    END LOOP;
END;
$$;

-- Function to get partition health information
-- Usage: SELECT * FROM get_device_connections_partition_info();
CREATE OR REPLACE FUNCTION get_device_connections_partition_info()
RETURNS TABLE(
    partition_name TEXT,
    row_count BIGINT,
    total_size TEXT,
    table_size TEXT,
    indexes_size TEXT,
    week_start DATE,
    week_end DATE,
    is_current BOOLEAN
)
LANGUAGE plpgsql
AS $$
BEGIN
    RETURN QUERY
    WITH partition_bounds AS (
        SELECT
            c.relname::text as pname,
            pg_get_expr(c.relpartbound, c.oid) as bounds
        FROM pg_class c
        JOIN pg_inherits i ON i.inhrelid = c.oid
        JOIN pg_class p ON p.oid = i.inhparent
        WHERE p.relname = 'device_connections'
        AND c.relkind = 'r'
    )
    SELECT
        t.tablename::text,
        COALESCE(s.n_live_tup, 0)::bigint,
        pg_size_pretty(pg_total_relation_size('public.'||t.tablename)),
        pg_size_pretty(pg_relation_size('public.'||t.tablename)),
        pg_size_pretty(
            pg_total_relation_size('public.'||t.tablename) -
            pg_relation_size('public.'||t.tablename)
        ),
        -- Extract start date from partition bounds
        SUBSTRING(pb.bounds FROM '''(\d{4}-\d{2}-\d{2})')::DATE,
        -- Extract end date from partition bounds
        SUBSTRING(pb.bounds FROM 'TO \(''(\d{4}-\d{2}-\d{2})')::DATE,
        -- Check if this partition covers current date
        CURRENT_DATE >= SUBSTRING(pb.bounds FROM '''(\d{4}-\d{2}-\d{2})')::DATE
        AND CURRENT_DATE < SUBSTRING(pb.bounds FROM 'TO \(''(\d{4}-\d{2}-\d{2})')::DATE
    FROM pg_tables t
    LEFT JOIN pg_stat_user_tables s ON s.schemaname = t.schemaname AND s.relname = t.tablename
    LEFT JOIN partition_bounds pb ON pb.pname = t.tablename
    WHERE t.tablename ~ '^device_connections_\d{4}w\d{2}$'
    AND t.schemaname = 'public'
    ORDER BY t.tablename DESC;
END;
$$;

-- Add helpful comments
COMMENT ON FUNCTION create_device_connections_partitions(INTEGER) IS
    'Creates the next N weeks of partitions for the device_connections table. Run weekly via cron: SELECT create_device_connections_partitions(8);';

COMMENT ON FUNCTION drop_old_device_connections_partitions(INTEGER) IS
    'Drops partitions older than N weeks. Default is 520 weeks (10 years). Run monthly via cron: SELECT drop_old_device_connections_partitions();';

COMMENT ON FUNCTION get_device_connections_partition_info() IS
    'Returns detailed information about all device_connections partitions including sizes, row counts, and date ranges.';
