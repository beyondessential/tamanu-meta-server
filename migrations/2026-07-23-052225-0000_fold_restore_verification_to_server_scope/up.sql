-- Restore-verification checks were filed group-scoped with the server id
-- baked into the check name: restore-verification:<server_id>:<type>:<intent>.
-- They are now ordinary server-scoped checks with a stable name
-- restore-verification:<type>:<intent> (the per-server dimension is
-- issues.server_id). Fold the existing group-scoped check-states onto the
-- new shape: move the embedded server id into server_id, clear
-- server_group_id, and rewrite ref/check_name to the stable name.
--
-- Old rows are group-scoped (server_group_id IS NOT NULL); the guard on the
-- embedded id being a uuid skips any malformed legacy row. A server belongs
-- to one group, so (server, type, intent) maps 1:1 to the stable name — no
-- issues_server_id_source_ref_key collision.
UPDATE issues
SET
	server_id = split_part(check_name, ':', 2)::uuid,
	server_group_id = NULL,
	ref = 'restore-verification:' || split_part(check_name, ':', 3) || ':' || split_part(check_name, ':', 4),
	check_name = 'restore-verification:' || split_part(check_name, ':', 3) || ':' || split_part(check_name, ':', 4)
WHERE source = 'canopy'
	AND server_group_id IS NOT NULL
	AND check_name LIKE 'restore-verification:%:%:%'
	AND split_part(check_name, ':', 2) ~ '^[0-9a-fA-F-]{36}$';

-- Drop the old per-server catalog rows (4-part names); the stable
-- (source, check_name) policies re-register on the next report/sweep.
DELETE FROM check_policies
WHERE source = 'canopy'
	AND check_name LIKE 'restore-verification:%:%:%';
