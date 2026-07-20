alter table check_policies
	drop column last_seen,
	drop column decommissioned_at,
	drop column decommissioned_by;
