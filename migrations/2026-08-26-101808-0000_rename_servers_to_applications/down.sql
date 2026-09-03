-- Reverse the applications rename. Pure renaming both ways, so no data moves
-- and nothing is dropped.

-- Restate the trigger body against the pre-rename column name (a PL/pgSQL body
-- is text, so it does not follow the rename back either).
CREATE OR REPLACE FUNCTION update_server_group_effective_version()
RETURNS trigger
LANGUAGE plpgsql
AS $function$
BEGIN
    IF NEW.version IS NOT NULL THEN
        UPDATE server_groups
        SET effective_version = NEW.version, updated_at = now()
        WHERE version_server_id = NEW.server_id;
    END IF;
    RETURN NEW;
END;
$function$;

DO $$
DECLARE
	r RECORD;
BEGIN
	FOR r IN
		SELECT c.conname, c.conrelid::regclass::text AS tbl
		FROM pg_constraint c
		WHERE c.contype = 'f'
		  AND c.conrelid::regclass::text IN (
			'application_certificates', 'application_names', 'issues',
			'scoped_check_policies', 'incident_reeval_queue',
			'version_known_issues', 'server_groups'
		  )
		  AND c.conname LIKE '%application_id_fkey'
	LOOP
		EXECUTE format(
			'ALTER TABLE %s RENAME CONSTRAINT %I TO %I',
			r.tbl,
			r.conname,
			replace(r.conname, 'application_id_fkey', 'server_id_fkey')
		);
	END LOOP;

	FOR r IN
		SELECT c.conname, c.conrelid::regclass::text AS tbl
		FROM pg_constraint c
		WHERE c.conname IN (
			'applications_alert_when_down_for_check',
			'application_certificates_state_check'
		)
	LOOP
		EXECUTE format(
			'ALTER TABLE %s RENAME CONSTRAINT %I TO %I',
			r.tbl,
			r.conname,
			replace(
				replace(r.conname, 'application_certificates_', 'server_certificates_'),
				'applications_', 'servers_'
			)
		);
	END LOOP;
END
$$;

ALTER INDEX IF EXISTS server_groups_version_application_id RENAME TO server_groups_version_server_id;
ALTER INDEX IF EXISTS scoped_check_policies_application RENAME TO scoped_check_policies_server;
ALTER INDEX IF EXISTS issues_application_source_ref RENAME TO issues_server_source_ref;
ALTER INDEX IF EXISTS issues_application_last_seen RENAME TO issues_server_last_seen;

ALTER INDEX IF EXISTS application_names_unpublished RENAME TO server_names_unpublished;
ALTER INDEX IF EXISTS application_names_application RENAME TO server_names_server;
ALTER INDEX IF EXISTS application_names_name RENAME TO server_names_name;
ALTER INDEX IF EXISTS application_names_pkey RENAME TO server_names_pkey;

ALTER INDEX IF EXISTS application_certificates_application RENAME TO server_certificates_server;
ALTER INDEX IF EXISTS application_certificates_renewal RENAME TO server_certificates_renewal;
ALTER INDEX IF EXISTS application_certificates_name_key RENAME TO server_certificates_name_key;
ALTER INDEX IF EXISTS application_certificates_expiry RENAME TO server_certificates_expiry;
ALTER INDEX IF EXISTS application_certificates_due RENAME TO server_certificates_due;
ALTER INDEX IF EXISTS application_certificates_pkey RENAME TO server_certificates_pkey;

ALTER INDEX IF EXISTS applications_host_key RENAME TO servers_host_key;
ALTER INDEX IF EXISTS applications_product RENAME TO servers_product;
ALTER INDEX IF EXISTS applications_parent_application_id RENAME TO servers_parent_server_id;
ALTER INDEX IF EXISTS applications_name_management_paused RENAME TO servers_name_management_paused;
ALTER INDEX IF EXISTS applications_live RENAME TO servers_live;
ALTER INDEX IF EXISTS applications_kind RENAME TO servers_kind;
ALTER INDEX IF EXISTS applications_id_hash RENAME TO servers_id_hash;
ALTER INDEX IF EXISTS applications_host_live RENAME TO servers_host_live;
ALTER INDEX IF EXISTS applications_host RENAME TO servers_host;
ALTER INDEX IF EXISTS applications_group_id RENAME TO servers_group_id;
ALTER INDEX IF EXISTS applications_device_id_unique RENAME TO servers_device_id_unique;
ALTER INDEX IF EXISTS applications_device RENAME TO servers_device;
ALTER INDEX IF EXISTS applications_pkey RENAME TO servers_pkey;

ALTER TABLE server_groups RENAME COLUMN version_application_id TO version_server_id;
ALTER TABLE version_known_issues RENAME COLUMN application_id TO server_id;
ALTER TABLE incident_reeval_queue RENAME COLUMN application_id TO server_id;
ALTER TABLE scoped_check_policies RENAME COLUMN application_id TO server_id;
ALTER TABLE issues RENAME COLUMN application_id TO server_id;
ALTER TABLE application_names RENAME COLUMN application_id TO server_id;
ALTER TABLE application_certificates RENAME COLUMN application_id TO server_id;

ALTER TABLE application_names RENAME TO server_names;
ALTER TABLE application_certificates RENAME TO server_certificates;
ALTER TABLE applications RENAME TO servers;
