DROP INDEX restore_replicas_consumer_name;

-- Names that were suffixed to resolve a duplicate stay as they are: the
-- original is not recoverable, and reinstating it would recreate the ambiguity.

ALTER TABLE backup_restore_checks
	DROP CONSTRAINT backup_restore_checks_replica_id_fkey;
ALTER TABLE backup_restore_checks
	ADD CONSTRAINT backup_restore_checks_replica_id_fkey
	FOREIGN KEY (replica_id) REFERENCES restore_replicas(id);
