alter table check_policies
	add column last_seen timestamptz,
	add column decommissioned_at timestamptz,
	add column decommissioned_by text;
