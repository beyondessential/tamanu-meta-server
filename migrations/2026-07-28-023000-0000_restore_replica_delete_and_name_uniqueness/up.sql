-- Two fixes to restore replica declarations, both about retiring one cleanly.

-- 1. Deleting a declaration must not be blocked by its restore-health history.
-- `backup_restore_checks.replica_id` was always meant to go NULL when the
-- declaration it points at is retired — the column is nullable precisely "so
-- history survives the declaration being retired", and RST says recorded
-- restore-health history is retained when a declaration is deleted. But the FK
-- was created with the default NO ACTION, so the first check reported against a
-- declaration pinned it in place: deleting it failed with
--   update or delete on table "restore_replicas" violates foreign key
--   constraint "backup_restore_checks_replica_id_fkey"
-- Recreate the constraint with ON DELETE SET NULL so the reports outlive the
-- declaration, unattached, exactly as intended.
ALTER TABLE backup_restore_checks
	DROP CONSTRAINT backup_restore_checks_replica_id_fkey;
ALTER TABLE backup_restore_checks
	ADD CONSTRAINT backup_restore_checks_replica_id_fkey
	FOREIGN KEY (replica_id) REFERENCES restore_replicas(id) ON DELETE SET NULL;

-- 2. A consumer's declaration names must be distinct. The scope uniqueness
-- indexes key on (consumer, group, type, intent, server), so two declarations
-- differing only by intent were allowed to share a name — and the operator UI
-- suggested the same group/server-derived name for both, making the collision
-- the default outcome rather than an unlikely slip. The name is what the
-- worklist hands the consumer to identify the replica, so a duplicate is
-- ambiguous to the consumer and to the operator reading the list.
--
-- Existing duplicates are kept, not dropped: the oldest declaration keeps the
-- name and each later one gets a `-2`, `-3`, … suffix, so the index can be
-- created without losing an operator's declaration. Operators can rename them
-- to something meaningful afterwards. Suffixing repeats because a suffixed name
-- can itself collide with a name already in use; each round strictly lengthens
-- the names it touches, so it converges.
DO $$
DECLARE
	round INT := 0;
BEGIN
	LOOP
		UPDATE restore_replicas r
		   SET name = dup.name || '-' || dup.n
		  FROM (
			SELECT id, name, n
			  FROM (
				SELECT id,
				       name,
				       row_number() OVER (
				           PARTITION BY consumer_device_id, name
				           ORDER BY created_at, id
				       ) AS n
				  FROM restore_replicas
			  ) numbered
			 WHERE n > 1
		  ) dup
		 WHERE r.id = dup.id;
		EXIT WHEN NOT FOUND;

		round := round + 1;
		IF round > 10 THEN
			RAISE EXCEPTION
				'restore_replicas names still collide per consumer after % rounds of suffixing',
				round;
		END IF;
	END LOOP;
END
$$;

CREATE UNIQUE INDEX restore_replicas_consumer_name
	ON restore_replicas (consumer_device_id, name);
