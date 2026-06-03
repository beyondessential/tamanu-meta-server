# Archival visibility + group archival

Completes the archival feature shipped with operator-first enrollment. Two gaps:

1. **Archived servers are unreachable.** Soft-delete works, but every server
   list query filters `deleted_at IS NULL`, so an archived server only exists at
   its detail URL (`ArchivedBanner` + Restore). No list/tab/filter to find one.
2. **Groups can't be archived at all** — `ServerGroup::delete` is a hard delete
   (it refuses non-empty groups: "move them out first"), and `server_groups`
   has no `deleted_at`.

Fix: make groups archivable like servers, and add an **Archived** view for both.

## Semantics

- Group archival mirrors servers: a nullable `deleted_at`; `soft_delete` sets
  it, `restore` clears it; live queries filter `deleted_at IS NULL`.
- **Archiving a group requires no *live* members** — same guard as today's
  hard delete, just soft. Already-archived members don't block it (they're
  hidden too). Soft-delete doesn't touch `servers.group_id` (the FK only fires
  on a real delete), so an archived group keeps its archived members.
- Group hard-delete goes away; "Delete group" becomes "Archive group".

## Database (`crates/database`)

- Migration `add_server_group_archival`: `ALTER TABLE server_groups ADD COLUMN
  deleted_at TIMESTAMPTZ` (nullable).
- `ServerGroup.deleted_at: Option<Timestamp>` (jiff_diesel `NullableTimestamp`,
  `treat_none_as_default_value = false`, matching `Server`).
- `ServerGroup::soft_delete` (guard: error if any live member; else set
  `deleted_at`), `ServerGroup::restore`, `ServerGroup::list_archived`.
- Add `.filter(deleted_at.is_null())` to live group queries: `list_all`,
  `list_by_ids`, `highest_member_ranks`, `search` (if present), and the
  status-page grouping source.
- `Server::list_archived` (`deleted_at IS NOT NULL`, ordered like `get_all`).
- `recompute_version`: no change needed (archived groups aren't shown; the
  trigger updating a hidden group's cached version is harmless).

## Endpoints (`crates/private-server/src/fns`)

- `servers`: `list_archived`.
- `server_groups`: `delete` handler now calls `soft_delete`; add `restore` and
  `list_archived`.
- Regenerate OpenAPI + TS types.

## Frontend (`private-web`)

- Servers page: new **Archived** tab (beside Groups/Ungrouped) listing archived
  groups and archived servers, each with a Restore action.
- Group detail: archived banner + Restore (mirror `ServerDetail`'s
  `ArchivedBanner`).
- Group delete control → "Archive" wording.

## Tests

- DB: group `soft_delete` refuses live members, succeeds when empty/only-archived;
  `restore`; `list_archived` (servers + groups); live listings exclude archived.
- Endpoint: archived listings; group archive→restore round-trip.

## Out of scope

- Group hard-delete (removed).
- Cascade archive/restore of members (we require empty-of-live instead).
