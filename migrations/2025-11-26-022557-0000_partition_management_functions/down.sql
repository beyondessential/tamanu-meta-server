-- Drop partition management functions

DROP FUNCTION IF EXISTS get_statuses_partition_info();
DROP FUNCTION IF EXISTS drop_old_statuses_partitions(INTEGER);
DROP FUNCTION IF EXISTS create_statuses_partitions(INTEGER);
