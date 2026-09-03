-- An application takes its machine's group, and cannot hold one of its own.
--
-- An operator sets the group on the machine; which deployment a box belongs to
-- is the one organisational fact the box has no way of knowing. The
-- applications on it follow. Moving a machine to another group moves its
-- workloads, and there is no separate move for an application.
-- See [FLT](.workhorse/specs/servers/overview.md), "Groups".
--
-- `applications.group_id` stays as a denormalisation rather than becoming a
-- join through the machine, so every group query reads one column. These
-- triggers are what keep it honest: whichever side is written, the two cannot
-- disagree.
--
-- WHY A TRIGGER AND ALSO A MODEL METHOD. `Machine::update` already propagates
-- the group and does the three things a column write cannot: re-evaluating
-- open issues for anything that gains a group, and recomputing the cached
-- effective version of both the old and new group. The trigger does not
-- replace that; it covers every *other* writer — raw SQL, a future handler, a
-- backfill — so the denormalisation cannot drift even when the consequences
-- are somebody else's job.
--
-- The two triggers are mutually recursive by construction and terminate:
-- propagating from a machine issues an UPDATE on its applications, which fires
-- the application-side trigger, which reads back the same group it was just
-- given.

CREATE FUNCTION applications_take_machine_group() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
	SELECT m.group_id INTO NEW.group_id FROM machines m WHERE m.id = NEW.machine_id;
	RETURN NEW;
END;
$$;

-- UPDATE only, deliberately not INSERT.
--
-- A data-modifying CTE's rows are not visible to the rest of the same
-- statement, so a caller that creates the machine and the application in one
-- statement — `WITH m AS (INSERT INTO machines …) INSERT INTO applications …`
-- — would have this trigger look the machine up, find nothing, and blank the
-- group it was just handed. The foreign key still passes, because constraint
-- checks use a later snapshot than the trigger's SELECT, so the failure is
-- silent: a correct-looking insert with a null group.
--
-- Insert-time agreement is therefore the caller's to get right, which the
-- operator flow does by creating the machine in its own statement. What the
-- trigger covers is drift *after* the fact: an application's group being
-- changed on its own, or a reassignment to another machine leaving the old
-- group behind. Adding INSERT back here reintroduces the footgun.
CREATE TRIGGER applications_take_machine_group
	BEFORE UPDATE OF group_id, machine_id ON applications
	FOR EACH ROW
	EXECUTE FUNCTION applications_take_machine_group();

CREATE FUNCTION machine_group_propagates() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
	UPDATE applications SET group_id = NEW.group_id WHERE machine_id = NEW.id;
	RETURN NULL;
END;
$$;

CREATE TRIGGER machine_group_propagates
	AFTER UPDATE OF group_id ON machines
	FOR EACH ROW
	WHEN (NEW.group_id IS DISTINCT FROM OLD.group_id)
	EXECUTE FUNCTION machine_group_propagates();

-- Bring existing rows into agreement. The machine is authoritative, but the
-- 1:1 backfill copied each application's group onto its machine, so this is a
-- no-op on anything that came through it and a correction on anything since.
UPDATE applications a
SET group_id = m.group_id
FROM machines m
WHERE a.machine_id = m.id AND a.group_id IS DISTINCT FROM m.group_id;
