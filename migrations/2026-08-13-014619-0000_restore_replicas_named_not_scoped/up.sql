-- A restore replica is identified by its name, not by the scope it covers.
--
-- An operator may declare as many replicas of one (group, type, intent, server)
-- as they have uses for — a raw one and a redacted one, a nightly one and a
-- weekly one — and tells them apart by name. The scope indexes allowed only one
-- of each, so the second was refused with "a matching restore replica is
-- already declared" whatever it was called. Names are already unique per
-- consumer, which is the uniqueness that was actually wanted.
DROP INDEX restore_replicas_scope_server;
DROP INDEX restore_replicas_scope_group;

-- With the scope no longer unique, (server, type, intent) no longer identifies
-- a replica: two of them can now sit on one server under one intent. The
-- declaration's name is what separates them, so a report has to carry it the
-- same way it already carries the group, server, type, and intent —
-- denormalised at report time so the report keeps naming its replica after the
-- declaration it came from is retired and `replica_id` goes NULL.
ALTER TABLE backup_restore_checks
	ADD COLUMN replica_name TEXT;

UPDATE backup_restore_checks c
   SET replica_name = r.name
  FROM restore_replicas r
 WHERE c.replica_id = r.id;
