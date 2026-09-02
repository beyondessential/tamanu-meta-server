-- A check is identified by what it asserts something about, as well as by its
-- source and its name. A machine-subject `disk_free` and an application-subject
-- one are two checks, with their own ceilings, rules and statistics.
--
-- Everything defaults to the application, which is where every check was filed
-- before the split and so where an unlisted name still belongs. The statements
-- below then move the rows whose filing site says otherwise, so an operator's
-- ceiling stays attached to the check it was set on.
alter table check_policies
	add column subject text not null default 'application';

alter table check_policies
	add constraint check_policies_subject_known
	check (subject in ('application', 'machine', 'group', 'canopy'));

-- Canopy's own checks: each grain is the scope its filing site names.
update check_policies set subject = 'machine'
where source = 'canopy'
  and check_name in (
	'backup-staleness',
	'backup-never',
	'backup-reconcile-report-gap',
	'backup-reconcile-size-mismatch',
	'backup-reconcile-missing',
	'backup-reconcile-recency',
	'restore-verification',
	'redaction'
  );

update check_policies set subject = 'group'
where source = 'canopy'
  and check_name in (
	'backup-maintenance-stale',
	'backup-maintenance-error',
	'backup-corruption',
	'backup-rotation-broken',
	'preflight-identity',
	'preflight-assume',
	'preflight-object-lock'
  );

update check_policies set subject = 'canopy'
where source = 'canopy'
  and check_name in (
	'slack-delivery-failure',
	'stale-healthchecks',
	'history-partition-runway',
	'dns-zone-coverage',
	'certificate-authority-unreachable',
	'certificate-authority-account',
	'certificate-authority-throttled',
	'name-management-pause-forgotten',
	'mcp-token-expiry'
  );

-- Reported checks: the names a unified push files against the box. Any source,
-- since the list describes what the check asserts rather than who reports it,
-- and canopy files none of these names itself.
update check_policies set subject = 'machine'
where check_name in (
	'billing_tags',
	'btrfs',
	'caddy_resolvers',
	'caddy_version',
	'caddyfile_version',
	'canopy_registration',
	'disk_free',
	'external_users',
	'held_captures',
	'inodes',
	'ips',
	'load',
	'memory',
	'munin',
	'tailscale',
	'tailscale_config',
	'time_sync',
	'uptime'
  );

alter table check_policies
	drop constraint check_policies_pkey,
	add constraint check_policies_pkey primary key (source, subject, check_name);
