# restore-verification: server-scoped, stable names

Restore-verification checks are filed group-scoped with the server id baked
into the check name — `restore-verification:<server_id>:<type>:<intent>`.
The fleet catalog fills with single-use names (one per server × type ×
intent), and a per-check policy — the whole point of the catalog — is
meaningless when every server has its own name.

## Why it's like that

The checks are filed `FilingScope::Group(group_id)` so they page regardless
of any server's `is_monitored` state (the old RST behaviour). Group-scoped
issues have no `server_id` slot, so the per-server dimension got pushed into
the check name.

## The decision

Drop the "page regardless of `is_monitored`" special case. Restore-verification
becomes an ordinary **server-scoped** check, subject to the same monitoring
and incident gates as any other of a server's checks. RST spec updated
accordingly (the Alerting section).

## Changes

`crates/database/src/restore.rs`:

- `restore_verification_ref(type, intent)` — drop the `server_id` parameter;
  return the stable `restore-verification:<type>:<intent>`
  (e.g. `restore-verification:tamanu-postgres:verify`).
- The three filing sites — `record_report`, `recover_old_scope_alerts`,
  `sweep_overdue` — file `FilingScope::Server { server_id, device_id: None }`
  instead of `FilingScope::Group(group_id)`. The per-server dimension moves
  from the check name into `issues.server_id`.
- `recover_old_scope_alerts` already iterates the group's servers and files
  per-server, so it maps over cleanly.

Net: the catalog holds a couple of stable policies reusable across the
fleet; per-server state and incidents still work via `server_id`; the checks
now respect `is_monitored`.

## Migration

Fold existing group-scoped `restore-verification:<server_id>:<type>:<intent>`
check-states onto the server-scoped stable name:

- set `server_id` from the embedded uuid, clear `server_group_id`, rewrite
  `ref`/`check_name` to `restore-verification:<type>:<intent>`;
- delete the old per-server catalog rows (the stable ones re-register on the
  next report/sweep via `file_check`).

A server belongs to one group, so `(server_id, type, intent)` maps 1:1 to
`(server_id, stable-name)` — no `issues_server_id_source_ref_key` collisions.
Guard the uuid parse (`split_part(check_name,':',2)::uuid`) against any
malformed legacy rows.

## Testing

- Update the restore tests asserting group-scope + the old ref format to the
  server-scoped stable ref.
- Two servers under one declaration → one catalog policy + two server-scoped
  check-states (not two catalog rows).
- An unmonitored server's failed restore-verification does not contribute to
  an incident (the behaviour change).
- Overdue sweep and scope-change recovery file server-scoped.

## Deferred (separate design)

The `(scope, scope_record_id)` check-state refactor — making scope
first-class instead of the two-nullable-FK encoding — is its own later
design. It must solve the polymorphic-FK / `ON DELETE CASCADE` question so it
doesn't trade the group-scope hack for a fresh orphan source.
