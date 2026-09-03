-- A backup is a machine's, not an application's.
--
-- What a run captures is a box's databases, configuration and filesystems, and
-- a box shared by two workloads backs up once. BAK has said so since it was
-- written: a device request resolves identity → machine → group and never
-- reaches the applications on the box. The storage lagged that, keying every
-- backup table on whichever application reported the run — which on a
-- two-workload box would attribute a snapshot to one of them arbitrarily.
--
-- Nothing on the wire moves with this. A device never names a target; it is
-- resolved from the authenticated identity. These columns are internal.
-- See [BAK](.workhorse/specs/public-server/backup.md).
--
-- Each column is RENAMEd rather than added-and-dropped, so it keeps its
-- position in the row. Every model here loads positionally, and a column that
-- moved to the end would silently misalign the ones after it.

-- 1. The append-only histories: no uniqueness on the moved column, so rename,
--    remap through the application's machine, and re-point the key.
ALTER TABLE backup_runs RENAME COLUMN server_id TO machine_id;
ALTER TABLE backup_runs DROP CONSTRAINT backup_runs_server_id_fkey;
UPDATE backup_runs r SET machine_id = a.machine_id FROM applications a WHERE a.id = r.machine_id;
ALTER TABLE backup_runs ADD CONSTRAINT backup_runs_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE SET NULL;
DROP INDEX backup_runs_server_id_type_reported_at_idx;
CREATE INDEX backup_runs_machine_id_type_reported_at_idx
	ON backup_runs (machine_id, type, reported_at DESC);

ALTER TABLE backup_run_progress RENAME COLUMN server_id TO machine_id;
ALTER TABLE backup_run_progress DROP CONSTRAINT backup_run_progress_server_id_fkey;
UPDATE backup_run_progress p SET machine_id = a.machine_id FROM applications a WHERE a.id = p.machine_id;
ALTER TABLE backup_run_progress ADD CONSTRAINT backup_run_progress_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE SET NULL;

ALTER TABLE backup_repo_snapshots RENAME COLUMN server_id TO machine_id;
ALTER TABLE backup_repo_snapshots DROP CONSTRAINT backup_repo_snapshots_server_id_fkey;
UPDATE backup_repo_snapshots s SET machine_id = a.machine_id FROM applications a WHERE a.id = s.machine_id;
ALTER TABLE backup_repo_snapshots ADD CONSTRAINT backup_repo_snapshots_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE SET NULL;

-- 2. The two keyed tables. The moved column is in their primary key, so two
--    applications on one box collapse to one row — which is the point: a box
--    advertises its capabilities once and is queued a request once. The key
--    comes off for the remap, since the intermediate state can collide.
--
--    No existing data collides, every machine being 1:1 with an application
--    until now, but the collapse is written rather than assumed away.
ALTER TABLE backup_requests RENAME COLUMN server_id TO machine_id;
ALTER TABLE backup_requests DROP CONSTRAINT backup_requests_pkey;
ALTER TABLE backup_requests DROP CONSTRAINT backup_requests_server_id_fkey;
UPDATE backup_requests r SET machine_id = a.machine_id FROM applications a WHERE a.id = r.machine_id;

-- An operator's latest intent for a (machine, type, purpose) is the one that
-- stands; an earlier duplicate has already been superseded in fact.
DELETE FROM backup_requests r USING backup_requests other
WHERE r.machine_id = other.machine_id
  AND r.type = other.type
  AND r.purpose = other.purpose
  AND (r.requested_at, r.ctid) < (other.requested_at, other.ctid);

ALTER TABLE backup_requests ADD CONSTRAINT backup_requests_pkey
	PRIMARY KEY (machine_id, type, purpose);
ALTER TABLE backup_requests ADD CONSTRAINT backup_requests_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE CASCADE;

-- The table is a machine's, so its name says so.
ALTER TABLE server_backup_capabilities RENAME TO machine_backup_capabilities;
ALTER TABLE machine_backup_capabilities RENAME COLUMN server_id TO machine_id;
ALTER TABLE machine_backup_capabilities DROP CONSTRAINT server_backup_capabilities_pkey;
ALTER TABLE machine_backup_capabilities DROP CONSTRAINT server_backup_capabilities_server_id_fkey;
UPDATE machine_backup_capabilities c SET machine_id = a.machine_id
	FROM applications a WHERE a.id = c.machine_id;

-- A box can run a type if any of its workloads advertised it, and has been able
-- to since the first of them said so.
UPDATE machine_backup_capabilities c SET
	enabled = agg.enabled,
	registered_at = agg.registered_at
FROM (
	SELECT machine_id, type, bool_or(enabled) AS enabled, min(registered_at) AS registered_at
	FROM machine_backup_capabilities
	GROUP BY machine_id, type
) agg
WHERE c.machine_id = agg.machine_id AND c.type = agg.type;

DELETE FROM machine_backup_capabilities c USING machine_backup_capabilities other
WHERE c.machine_id = other.machine_id AND c.type = other.type AND c.ctid > other.ctid;

-- Named explicitly: Postgres does not rename a constraint with its table, so a
-- generated name here would be the old one and the reverse would not find it.
ALTER TABLE machine_backup_capabilities ADD CONSTRAINT machine_backup_capabilities_pkey
	PRIMARY KEY (machine_id, type);
ALTER TABLE machine_backup_capabilities ADD CONSTRAINT machine_backup_capabilities_machine_id_fkey
	FOREIGN KEY (machine_id) REFERENCES machines (id) ON DELETE CASCADE;
