ALTER TABLE restore_replicas RENAME COLUMN overdue_after TO freshness;
ALTER TABLE restore_replicas DROP COLUMN params;

ALTER TABLE restore_consumer_capabilities
	DROP COLUMN params,
	DROP COLUMN semantics,
	DROP COLUMN description;
