-- Drop partition management functions for device_connections

DROP FUNCTION IF EXISTS get_device_connections_partition_info();
DROP FUNCTION IF EXISTS drop_old_device_connections_partitions(INTEGER);
DROP FUNCTION IF EXISTS create_device_connections_partitions(INTEGER);
