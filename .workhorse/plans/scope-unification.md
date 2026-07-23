# Unify check-state scope into one `Scope` type

Check-state scope (server / group / canopy-wide) is modelled three ways
today, all encoding the same idea:

- `FilingScope { Server { server_id, device_id }, Group(Uuid), Global }` —
  the write side (also bundles the reporting device, which is *provenance*,
  not scope);
- `PolicyScope { Server(Uuid), Group(Uuid), Global }` — scoped silences;
- an ad-hoc `match (issue.server_id, issue.server_group_id)` that resolves an
  issue's scope to an incident target, **repeated verbatim** in
  `issue_target_and_monitored` and `reconcile_open_incidents`.

Storage is fine and stays: `issues` and `scoped_check_policies` each carry
nullable `server_id` / `server_group_id` (CHECK "at most one", three partial
unique indexes, and `ON DELETE CASCADE` FKs). That native integrity is
load-bearing — it's what stops check-states outliving their server/group —
so no migration and no schema change. This is a Rust-only unification
(chosen over a polymorphic `(scope, scope_record_id)` column precisely to
keep the FK cascade + uniqueness Postgres enforces today).

## The type

New in the `database` crate (e.g. `crate::scope`):

```rust
pub enum Scope { Server(Uuid), Group(Uuid), Global }
```

The single place that maps to/from storage and to an incident target:

- `Scope::to_columns(self) -> (Option<Uuid> /*server_id*/, Option<Uuid> /*server_group_id*/)`
- `Scope::from_columns(server_id, server_group_id) -> Scope`
- `Scope::resolve_incident_target(&mut conn) -> Result<Option<(IncidentTarget, bool /*monitored*/)>>`
  — the one implementation of today's duplicated match (server → its group +
  `is_monitored`; group → itself, monitored; global → Global; ungrouped
  server → None).

## Refactor

1. `FilingScope` → `Scope`. Decouple provenance: `CheckFiling` gets
   `scope: Scope` + a separate `device_id: Option<Uuid>`. `file_check`
   routes on `scope` via `to_columns` (server → `save_with_state`, group →
   `raise_group_event_with_state`, global → `raise_global_event_with_state`).
   Update the write-side producers (tailnet_sweeps, statuses, self_alerts,
   restore, backup/staleness, backup/reconcile) to pass `scope` + `device_id`.
2. `PolicyScope` → `Scope`. `ScopedCheckPolicy` CRUD
   (`silence`/`unsilence`/`get`/`list_silences`/`chain_for`) and the
   `silenced_refs` shim take `Scope`; `scope_filter` becomes `to_columns`.
3. Replace the duplicated `match (server_id, server_group_id)` in
   `issue_target_and_monitored` and `reconcile_open_incidents` with
   `Scope::from_columns(...).resolve_incident_target(conn)`.

Leave direct scope-specific read filters as they are (e.g.
`.filter(server_id.eq(x))` in `health_from_check_state`,
`consolidated_checks_latest`, `source_freshness`, `list_*`): those query a
known scope, they aren't the scope-interpretation hack.

## No behaviour change

Nothing observable changes and the DB is untouched, so the existing suite
passing unchanged is the correctness bar. Add unit tests for
`Scope::{to_columns, from_columns}` round-tripping and
`resolve_incident_target` covering server-in-group / ungrouped-server /
group / global.

## Convention

Add to `AGENTS.md` (Code Style & Patterns): check-state / issue / policy
scope is the single `Scope` type — use it for filing, silences, and
incident-target resolution; never add another scope enum or write ad-hoc
`match (server_id, server_group_id)`. Map through
`Scope::from_columns`/`to_columns`/`resolve_incident_target`.
