-- Rename `servers` to `applications`, the first step of splitting the core
-- model into machines, application servers, and identities.
--
-- Canopy's `server` conflates a machine with an application server. The split
-- gives each its own grain, but the rename lands first so the machine work is
-- written against names that already read correctly, and so each affected
-- table is touched once rather than twice.
--
-- Scope of this migration: the tables that stay at the APPLICATION grain.
--
--   servers             -> applications
--   server_certificates -> application_certificates   (a name is served by an application)
--   server_names        -> application_names
--
-- plus the `server_id` columns on tables that keep pointing at an application:
-- issues, scoped_check_policies, incident_reeval_queue, version_known_issues,
-- and server_groups.version_server_id.
--
-- DELIBERATELY UNTOUCHED, because they move to the machine grain in the split
-- and renaming them here would touch them twice:
--
--   statuses, server_reported_detail, server_backup_capabilities,
--   server_enrollment_tokens, server_enrollment_challenges, backup_*,
--   restore_replicas, device_server_associations
--
-- Their `server_id` keeps referencing `applications.id` in the meantime; the
-- FK follows the table rename automatically.
--
-- `server_groups` and its satellites keep their names: renaming them to
-- `deployments` would pre-empt card W1, which exists to settle that word.
--
-- NOTE ON THE WIRE. The device API's `server_id` is the MACHINE id and keeps
-- that meaning through the transition, while `servers.id` becomes
-- `applications.id`. This migration renames storage only; no wire field
-- changes name here.
--
-- CHECK constraints and partial-index predicates that mention a renamed column
-- follow it automatically -- Postgres stores parsed expressions, not text --
-- so only the object NAMES need restating below.

ALTER TABLE servers RENAME TO applications;
ALTER TABLE server_certificates RENAME TO application_certificates;
ALTER TABLE server_names RENAME TO application_names;

ALTER TABLE application_certificates RENAME COLUMN server_id TO application_id;
ALTER TABLE application_names RENAME COLUMN server_id TO application_id;
ALTER TABLE issues RENAME COLUMN server_id TO application_id;
ALTER TABLE scoped_check_policies RENAME COLUMN server_id TO application_id;
ALTER TABLE incident_reeval_queue RENAME COLUMN server_id TO application_id;
ALTER TABLE version_known_issues RENAME COLUMN server_id TO application_id;
ALTER TABLE server_groups RENAME COLUMN version_server_id TO version_application_id;

-- Indexes. Renaming a table or column leaves its indexes under their old
-- names, so restate them to keep the catalog readable.
ALTER INDEX IF EXISTS servers_pkey RENAME TO applications_pkey;
ALTER INDEX IF EXISTS servers_device RENAME TO applications_device;
ALTER INDEX IF EXISTS servers_device_id_unique RENAME TO applications_device_id_unique;
ALTER INDEX IF EXISTS servers_group_id RENAME TO applications_group_id;
ALTER INDEX IF EXISTS servers_host RENAME TO applications_host;
ALTER INDEX IF EXISTS servers_host_live RENAME TO applications_host_live;
ALTER INDEX IF EXISTS servers_id_hash RENAME TO applications_id_hash;
ALTER INDEX IF EXISTS servers_kind RENAME TO applications_kind;
ALTER INDEX IF EXISTS servers_live RENAME TO applications_live;
ALTER INDEX IF EXISTS servers_name_management_paused RENAME TO applications_name_management_paused;
ALTER INDEX IF EXISTS servers_parent_server_id RENAME TO applications_parent_application_id;
ALTER INDEX IF EXISTS servers_product RENAME TO applications_product;
ALTER INDEX IF EXISTS servers_host_key RENAME TO applications_host_key;

ALTER INDEX IF EXISTS server_certificates_pkey RENAME TO application_certificates_pkey;
ALTER INDEX IF EXISTS server_certificates_due RENAME TO application_certificates_due;
ALTER INDEX IF EXISTS server_certificates_expiry RENAME TO application_certificates_expiry;
ALTER INDEX IF EXISTS server_certificates_name_key RENAME TO application_certificates_name_key;
ALTER INDEX IF EXISTS server_certificates_renewal RENAME TO application_certificates_renewal;
ALTER INDEX IF EXISTS server_certificates_server RENAME TO application_certificates_application;

ALTER INDEX IF EXISTS server_names_pkey RENAME TO application_names_pkey;
ALTER INDEX IF EXISTS server_names_name RENAME TO application_names_name;
ALTER INDEX IF EXISTS server_names_server RENAME TO application_names_application;
ALTER INDEX IF EXISTS server_names_unpublished RENAME TO application_names_unpublished;

ALTER INDEX IF EXISTS issues_server_last_seen RENAME TO issues_application_last_seen;
ALTER INDEX IF EXISTS issues_server_source_ref RENAME TO issues_application_source_ref;
ALTER INDEX IF EXISTS scoped_check_policies_server RENAME TO scoped_check_policies_application;
ALTER INDEX IF EXISTS server_groups_version_server_id RENAME TO server_groups_version_application_id;

-- Constraints. `ALTER TABLE ... RENAME CONSTRAINT` has no IF EXISTS, so guard
-- each on the catalog rather than assuming every name is present.
DO $$
DECLARE
	r RECORD;
BEGIN
	FOR r IN
		SELECT c.conname, c.conrelid::regclass::text AS tbl
		FROM pg_constraint c
		WHERE c.conname IN (
			'servers_alert_when_down_for_check',
			'server_certificates_state_check'
		)
	LOOP
		EXECUTE format(
			'ALTER TABLE %s RENAME CONSTRAINT %I TO %I',
			r.tbl,
			r.conname,
			replace(
				replace(r.conname, 'server_certificates_', 'application_certificates_'),
				'servers_', 'applications_'
			)
		);
	END LOOP;

	-- Foreign keys named after the column they were built on.
	FOR r IN
		SELECT c.conname, c.conrelid::regclass::text AS tbl
		FROM pg_constraint c
		WHERE c.contype = 'f'
		  AND c.conrelid::regclass::text IN (
			'application_certificates', 'application_names', 'issues',
			'scoped_check_policies', 'incident_reeval_queue',
			'version_known_issues', 'server_groups'
		  )
		  AND c.conname LIKE '%server_id_fkey'
	LOOP
		EXECUTE format(
			'ALTER TABLE %s RENAME CONSTRAINT %I TO %I',
			r.tbl,
			r.conname,
			replace(r.conname, 'server_id_fkey', 'application_id_fkey')
		);
	END LOOP;
END
$$;

-- A PL/pgSQL body is stored as text, so it does NOT follow a column rename the
-- way a view or a CHECK expression does. This trigger keeps a group's cached
-- effective version in step with its canonical member's reported version, and
-- it reads the column just renamed to `version_application_id`. Left alone it
-- would keep naming a column that no longer exists, and every status push
-- would fail at the trigger.
--
-- `NEW.server_id` is the `statuses` column, which is deliberately NOT renamed
-- here (statuses moves to the machine grain in the split), so it stays.
CREATE OR REPLACE FUNCTION update_server_group_effective_version()
RETURNS trigger
LANGUAGE plpgsql
AS $function$
BEGIN
    IF NEW.version IS NOT NULL THEN
        UPDATE server_groups
        SET effective_version = NEW.version, updated_at = now()
        WHERE version_application_id = NEW.server_id;
    END IF;
    RETURN NEW;
END;
$function$;
