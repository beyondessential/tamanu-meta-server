# Flatten the server hierarchy into server groups, add notes and tags

## Context

Today, servers form a tree via `servers.parent_server_id`: one "root" server per
group, with children below it. The root server is what incidents attach to, what
the Incidents page filter lists, and what the UI uses as the unit of grouping.
This works but conflates two concerns — "this server is special / top of tree"
and "these servers belong together" — and forces every code path that wants
group-level state to walk the tree via recursive CTEs.

We want:

1. Replace the parent/child tree with a flat **server group**: a first-class
   entity that owns a set of equal-level servers.
2. Add a freeform **notes** text field to both server groups and servers.
3. Add a structured key-value **tags** map (`HashMap<String, String>`) to both
   server groups and servers.
4. A new public-server, device-authenticated endpoint that returns the *merged*
   tags for the calling device's server — the group's tags overlaid by the
   server's own tags (server wins on key collision).

Decisions taken with the user (so the plan can be terse):

- Incidents are rekeyed: `incidents.server_id` → `incidents.server_group_id`.
  This makes the schema honest (incidents have always been group-scoped).
- Tag values are strings only.
- `ServerKind` (Central/Facility/…) stays as classification but no longer
  dictates hierarchy. Any kind can belong to any group.
- The `central_public_key` ticket field and its parent-lookup behaviour in
  `upsert_from_ticket` is removed outright. Operators will assign groups
  through the admin UI for now; ticket-driven group placement is a separate
  follow-up.
- `servers.group_id` is **nullable** — a server may be "ungrouped".
- Events from an ungrouped server are still stored as issues, but
  `find_or_open_incident` skips while `group_id IS NULL` (no incident is
  opened). When the server is later moved into a group, an incident
  re-evaluation runs over that server's currently-open issues, creating an
  incident if warranted. Once grouped, normal incident flow resumes.

UI direction (also from the user):

- The current `/servers` page (Central/Facility tabs) is replaced by a
  groups-first view: a "Groups" tab listing groups, plus an "Ungrouped" tab
  listing servers with no `group_id`.
- The status page (`/`) becomes group-first: each card represents a group
  rather than a single central server. Cards are bucketed under section
  headers by the **highest rank present in the group** (Production > Clone
  > Demo > Test > Dev, per `SERVER_RANK_ORDER`).
- Wherever a server name is rendered in lists/headers, prefix it with the
  group's name and an interpunct separator: `Group Name · Server Name`.
  Ungrouped servers show just the server name (no leading separator).

When done, this plan file is to be moved to `docs/plans/server-groups-and-tags.md`
in the repo and committed with the `plan:` prefix per the personal workflow.

## Database changes

One migration directory under `migrations/`, dated for today:

`2026-05-22-NNNNNN-0000_server_groups/up.sql` (and matching `down.sql`):

1. Create `server_groups`:
   ```
   id          uuid primary key default gen_random_uuid()
   name        text not null
   notes       text not null default ''
   tags        jsonb not null default '{}'::jsonb
       check (jsonb_typeof(tags) = 'object')
   created_at  timestamptz not null default now()
   updated_at  timestamptz not null default now()
   ```
   Plus an `updated_at` trigger consistent with other tables in the schema.

2. Add columns to `servers`:
   - `group_id uuid` (**stays nullable**; foreign key references
     `server_groups(id)`)
   - `notes text not null default ''`
   - `tags jsonb not null default '{}'::jsonb check (jsonb_typeof(tags) = 'object')`
   - Index `servers(group_id)` (partial — `WHERE group_id IS NOT NULL`).

3. Data migration in the same migration:
   - For each `servers` row with `parent_server_id IS NULL` (root) that has
     at least one child, insert a `server_groups` row using the root's name
     (fallback: id-derived name when the root has none) and set the root's
     `group_id` to the new group id.
   - Walk descendants iteratively (a single `WITH RECURSIVE` UPDATE) and
     propagate `group_id` to every child.
   - Standalone roots (parent_server_id IS NULL and no children) are left
     ungrouped (`group_id IS NULL`) — they don't need a group purely to
     preserve a one-server "tree".
   - The `Uuid::nil()` "own" server stays ungrouped.

4. Drop `servers.parent_server_id` and its index.

5. Rekey incidents:
   - `alter table incidents add column server_group_id uuid`
   - `update incidents set server_group_id = (select group_id from servers where servers.id = incidents.server_id)`
   - There should be no pre-existing incidents for ungrouped servers, but
     defensively: any incident whose server's `group_id` is NULL after the
     above join should be deleted, with a comment in the migration logging
     the cleanup. (Incidents on standalone-root servers were previously
     possible — that data needs a decision: I'll preserve them by giving
     each such root its own group too. Update step 3: also create a group
     for any standalone root that has any open incident.)
   - `alter table incidents alter column server_group_id set not null`
   - `alter table incidents add foreign key (server_group_id) references server_groups(id)`
   - Drop `incidents.server_id` and its foreign key.
   - Adjust the partial unique index on open incidents
     (`2026-05-21-060134-0000_incidents_open_unique`) — it currently keys on
     `server_id`; rebuild on `server_group_id`.

Files touched:
- `migrations/<new>/up.sql`, `down.sql`
- `crates/database/src/schema.rs` — regenerated; `parent_server_id` removed,
  `group_id` / `notes` / `tags` added to `servers`, new `server_groups` table,
  `incidents.server_id` becomes `server_group_id`.

## Database crate

`crates/database/src/server_groups.rs` (new module, re-exported from `lib.rs`):

- `pub struct ServerGroup { id, name, notes, tags: TagMap, created_at, updated_at }`
- `pub struct NewServerGroup { name, notes, tags }`
- `pub struct PartialServerGroup { id, name?, notes?, tags? }` (`AsChangeset`)
- Methods: `create`, `get_by_id`, `list_all`, `update`, `delete`,
  `list_servers(&self, db)` — returns `Vec<Server>` ordered by name.
- `search` — fuzzy on `name` + `id` for the group picker (mirrors the shape of
  the current `search_for_parent`, minus the rank/kind tie-breakers).

`commons-types` gets `pub type TagMap = std::collections::BTreeMap<String, String>;`
(BTreeMap so JSON serialisation is deterministic) with a thin wrapper newtype
that owns `Serialize`/`Deserialize` and a `merged_with(&self, other)` helper:
"self overrides other on key collision". This is what the public tags endpoint
uses for the group→server overlay.

`crates/database/src/servers.rs`:
- Remove: `parent_server_id` field, `list_roots`, `get_children`, `root_id`,
  `descendant_ids`, `search_for_parent`, `central_public_key` branch in
  `upsert_from_ticket`.
- Add to `Server`: `group_id: Option<Uuid>`, `notes: String`, `tags: TagMap`.
- Add to `PartialServer`: `group_id: Option<Option<Uuid>>` (nested option so
  the wire format can distinguish "leave alone" from "unset to null"),
  `notes?`, `tags?`. Remove `parent_server_id`.
- Add to `NewServer`: `group_id: Option<Uuid>` (optional — new servers can
  start ungrouped).
- `upsert_from_ticket` no longer derives a parent or group from the ticket;
  imported servers start with `group_id = None`.
- New helper: `Server::tags_merged_with_group(&self, db) -> Result<TagMap>` —
  fetches the group's tags (empty map if ungrouped) and overlays the server's
  tags on top. Used by the public endpoint.
- New helper:
  `Server::assign_to_group(db, server_id, Option<Uuid>) -> Result<Server>` —
  used by the private update endpoint. When the transition is `None →
  Some(group)`, after the update it triggers
  `re_evaluate_incident_membership` for the new group so any open issues on
  this server can promote to an incident. When `Some → None` the server's
  open issues stay around but no new incident logic runs (existing incidents
  for the group are unaffected since they're already group-keyed).
- `Server::list_ungrouped(db) -> Result<Vec<Self>>` — for the "Ungrouped" UI
  tab.

`crates/database/src/issues.rs`:
- `Incident::server_id` → `Incident::server_group_id`. Same for any joinables in
  schema.rs.
- `Incident::list_for_server(server_id)` keeps its name but is reimplemented as
  "fetch server's group; if ungrouped return empty Vec; else list incidents
  for that group" — direct join, no CTE.
- `re_evaluate_incident_membership(root_server_id)` →
  `re_evaluate_incident_membership(server_group_id)`. All callers updated
  (callers in `issues.rs` already had `Server::root_id` calls — those go away
  and the callers pass `server.group_id` directly).
- `find_or_open_incident` keys on `server_group_id`. Its callers must pass
  `Some(group_id)` — if the originating server is ungrouped, the caller
  (`NewEvent::save` and friends) **skips the call** entirely. The new issue
  row is persisted normally; an incident simply isn't opened until the
  server is later grouped (at which point `Server::assign_to_group` triggers
  re-evaluation).
- `IssueListFilters.server_group_id` is now a real group id, not "any server in
  the group". The query becomes `issues.server_id IN (SELECT id FROM servers
  WHERE group_id = $1)` — no recursive CTE.
- `NewEvent::save` is updated: load the server, branch on
  `server.group_id.is_some()` for the incident-opening side; always save the
  issue.

`crates/database/src/notes.rs`: untouched. `IssueNote`/`IncidentNote` aren't
affected; the new `notes` field is a column on `server_groups`/`servers`, not a
note-thread.

## Private-server

`crates/private-server/src/fns/server_groups.rs` (new module):
- `POST /api/server_groups/list` → `Vec<ServerGroup>`
- `POST /api/server_groups/get` → `{ group, servers: Vec<ServerInfo> }`
- `POST /api/server_groups/create` → `ServerGroup`
- `POST /api/server_groups/update` → `ServerGroup` (handles name, notes, tags)
- `POST /api/server_groups/delete` → empty body; reject if it still has servers
- `POST /api/server_groups/search` → `Vec<ServerGroup>` (for the group picker in
  ServerEdit)
- Mount: add `.nest("/server_groups", server_groups::routes())` in
  `crate::fns::routes()`.
- All endpoints take `TailscaleAdmin`.

`crates/private-server/src/fns/servers.rs`:
- Remove `list_roots` and `search_parent`.
- `get_detail`:
  - Drop the `child_servers` branch keyed on `kind == Central`.
  - Add `group: ServerGroup` (the server's group) and
    `siblings: Vec<ServerInfo>` (other servers in the same group).
- `update` accepts `group_id`, `notes`, `tags` (already plumbed via
  `PartialServer`).
- `get_info` no longer populates `parent_server_*`; populate `group_id` and a
  display `group_name` instead.

`crates/private-server/src/fns/statuses.rs`:
- `server_grouped_ids` (used by the status page) returns
  `{rank: [server_group_id]}` instead of `{rank: [server_id]}`. The rank for
  a group is the **highest-ranked member's rank** per `SERVER_RANK_ORDER`
  (Production > Clone > Demo > Test > Dev). Groups with no ranked members
  use `None`/unranked.
- `server_details` is replaced by `group_details(group_id) -> ServerGroupCard`.
  The new shape contains:
  - `id, name, notes`
  - `version, version_distance` — picked from a representative server (the
    one with the most-recent status, or by rank/name tiebreak — pick the
    rule and document it inline).
  - `members: Vec<{ id, name, host, kind, rank, up, health, version }>` —
    one entry per server in the group, so the UI can render the status dot
    grid without further fetches.
- Rename `CentralServerCard` → `ServerGroupCard` and update consumers.
- The "ungrouped" tab in the UI is fed by a separate
  `servers::list_ungrouped` private endpoint; the Status page intentionally
  does *not* display ungrouped servers (they're not monitored for
  incidents).

`crates/private-server/src/fns/devices.rs`:
- `ServerInfo` builder loses `parent_server_*`, gains `group_id`/`group_name`.

`crates/private-server/openapi`: re-generate via `just gen-openapi` and commit
both `openapi.json` and `api-types.ts` alongside the Rust change.

## Public-server

`crates/public-server/src/tags.rs` (new module):
- `GET /tags` with `security(("server-device" = []))`.
- Resolve the device → `Server::get_by_device_id` → pick the (single) server
  for that device (error if 0 or >1).
- Call `Server::tags_merged_with_group` and return `Json<TagMap>`.
- Mount in `lib.rs`: `.nest("/tags", tags::routes())`.

`crates/public-server/src/servers.rs`:
- The `PublicServer` shape unchanged (it didn't expose `parent_server_id`); the
  underlying `Server` PATCH/POST endpoints inherit the new shape automatically
  via `PartialServer`/`NewServer`.
- `list` still filters to `ServerKind::Central` and `listed = true` — that's a
  display choice unrelated to grouping; leave it alone.

Tests:
- `crates/public-server/tests/tags.rs` (new): two happy-path tests
  (group-only tags; server overrides group) and one "no server attached to
  device" failure.

## React UI (`private-web/`)

After `just gen-openapi`, `api-types.ts` will already have the new shapes.

New code:
- `src/routes/GroupDetail.tsx`: shows group name, notes, tags, member servers
  (linking out to each), plus an Edit button.
- `src/routes/GroupEdit.tsx`: form for name, notes, tags, plus a member-server
  picker (multi-add from existing servers).
- `src/components/TagsEditor.tsx`: reusable key-value list editor. Each row is
  `{ key: TextField, value: TextField, deleteButton: IconButton }`, plus an
  "Add tag" button that appends an empty row. Emits `Record<string, string>` on
  change. Validation: trim keys, reject empty keys, reject duplicate keys.
- `src/components/ServerNameWithGroup.tsx`: small presentational helper that
  renders `<groupName> · <serverName>` (with the group name styled as
  secondary text), or just `<serverName>` when ungrouped. Used everywhere a
  server name appears in lists or headers (`ServerShorty`, the status page
  members list, incidents/issues rows, search results).

Updated code:
- `src/App.tsx`: add `/groups/:id` and `/groups/:id/edit` routes. The existing
  `/servers` route stays but its page changes (below).
- `src/routes/Servers.tsx` + `src/routes/ServersList.tsx`: replace the
  Central/Facility tabs with two tabs: **Groups** (lists `ServerGroup`s — id,
  name, member count, tag count, notes preview, linking to `/groups/:id`) and
  **Ungrouped** (lists servers with `group_id IS NULL`, linking to
  `/servers/:id`). `ServerShorty` still renders rows, possibly extended with
  a member-count column when used in the groups tab; alternatively introduce
  a sibling `GroupShorty` component.
- `src/routes/Status.tsx`:
  - Swap `statuses.server_grouped_ids` and `statuses.server_details` for the
    new group-keyed equivalents (`server_grouped_ids` returns group ids;
    `group_details` returns a `ServerGroupCard`).
  - `openIncidentServers` set becomes `openIncidentGroups`, populated from
    `incidents.list_active`'s new `server_group_id` field.
  - The card renders the group name, then a row of `StatusDot`s — one per
    member server — and the version indicator picks the representative
    server's version (per the server-side decision).
  - Section headers come from the group's highest-rank bucket.
- `src/routes/Incidents.tsx`: replace `useApi("servers", "list_roots")` with
  `useApi("server_groups", "list")` and update the filter dropdown labels.
  Server-name rendering in issue rows uses `<ServerNameWithGroup>`.
- `src/routes/ServerDetail.tsx`: drop `ChildServers`; render a "Group" section
  linking to `/groups/:id` and listing siblings. Header uses
  `<ServerNameWithGroup>`. Add notes/tags display.
- `src/routes/ServerEdit.tsx`:
  - Replace `ParentServerControl` (autocomplete on parent server) with a
    `GroupControl` autocomplete on `server_groups/search` (with a "no group"
    clear option).
  - Add `notes` multiline `TextField`.
  - Add `<TagsEditor>` for `tags`.

E2E tests (`private-web/e2e/`):
- `seed.ts`:
  - Add `seedServerGroup(sql, opts)`.
  - Update `seedServer(sql, opts)` to accept optional `groupId`; drop
    `parentServerId`.
- New `groups.spec.ts`: list, create, edit (name/notes/tags), delete.
- Update `servers.spec.ts` for the new tabs (Groups + Ungrouped) and the
  group-name-prefixed display.
- Update `incidents.spec.ts` for group-keyed filtering.
- Update `status.spec.ts` for group-keyed cards bucketed by max rank.

## Tests in Rust crates

- `crates/database/tests/server_groups.rs` (new): CRUD + `list_servers` +
  `tags_merged_with_group` (covers group-only, server-only, override,
  ungrouped-server case).
- `crates/database/tests/upsert_from_ticket.rs`: assert that
  `central_public_key` is ignored / no longer derives a group, imported
  server starts ungrouped.
- `crates/private-server/tests/issues.rs`:
  - `incident_groups_at_root_server` → rename `incident_groups_at_server_group`
    and rewrite to seed a group with two servers, file an event on one, and
    assert the incident is on the group and visible when querying by either
    server's id.
  - New `ungrouped_server_event_skips_incident`: file an event on an
    ungrouped server, assert the issue row exists, assert no incident is
    opened.
  - New `assigning_group_opens_pending_incident`: file an event on an
    ungrouped server (issue exists, no incident), then assign the server to
    a group, assert an incident is now open and points to the group.
- `crates/private-server/tests/update_server.rs`: drop parent_server_id tests;
  add `update_server_group_id`, `update_server_clear_group_id`,
  `update_server_notes`, `update_server_tags`.
- `crates/private-server/tests/import_ticket.rs`: update for ungrouped
  ticket-import semantics.
- `crates/public-server/tests/tags.rs`: covered above; include the
  ungrouped-server happy path (returns just the server's tags).

## Files to be modified — quick index

Critical reads/edits (Rust):
- `migrations/<new>/up.sql` (new)
- `crates/database/src/schema.rs`
- `crates/database/src/server_groups.rs` (new)
- `crates/database/src/servers.rs`
- `crates/database/src/issues.rs`
- `crates/database/src/lib.rs` (re-export)
- `crates/commons-types/src/server/ticket.rs` (drop `central_public_key`)
- `crates/private-server/src/fns/mod.rs` (mount new module)
- `crates/private-server/src/fns/server_groups.rs` (new)
- `crates/private-server/src/fns/servers.rs`
- `crates/private-server/src/fns/statuses.rs`
- `crates/private-server/src/fns/devices.rs`
- `crates/public-server/src/lib.rs` (mount new module)
- `crates/public-server/src/tags.rs` (new)
- `crates/public-server/src/servers.rs` (only if `Server` shape changes break
  it — likely zero edits beyond a recompile)

Critical reads/edits (frontend):
- `private-web/openapi.json` (regenerated)
- `private-web/src/api-types.ts` (regenerated)
- `private-web/src/types.ts` (re-exports for `ServerGroup`, `TagMap`,
  `ServerGroupCard`)
- `private-web/src/App.tsx`
- `private-web/src/routes/Servers.tsx` (Groups + Ungrouped tabs)
- `private-web/src/routes/ServersList.tsx` (extend to render groups or split
  into `GroupsList` + `UngroupedServersList`)
- `private-web/src/routes/GroupDetail.tsx` (new)
- `private-web/src/routes/GroupEdit.tsx` (new)
- `private-web/src/routes/Status.tsx` (group-keyed, ranked buckets)
- `private-web/src/routes/Incidents.tsx`
- `private-web/src/routes/ServerDetail.tsx`
- `private-web/src/routes/ServerEdit.tsx`
- `private-web/src/components/TagsEditor.tsx` (new)
- `private-web/src/components/ServerNameWithGroup.tsx` (new)
- `private-web/src/components/ServerShorty.tsx` (uses `ServerNameWithGroup`)
- `private-web/e2e/seed.ts`
- `private-web/e2e/groups.spec.ts` (new)
- `private-web/e2e/servers.spec.ts`
- `private-web/e2e/incidents.spec.ts`
- `private-web/e2e/status.spec.ts`

## Implementation order

Commit-as-you-go, in this order:

1. Migration file + schema regen + minimal `database::server_groups` module.
   Build only — tests will still fail at this point.
2. Update `Server` model and remove the dead hierarchy methods. Update
   `upsert_from_ticket` to drop the `central_public_key` lookup (imported
   servers start ungrouped). Add `list_ungrouped`, `assign_to_group`.
3. Update `Incident`/`Issue` to use `server_group_id`. Plumb the
   skip-if-ungrouped branch through `NewEvent::save` and friends. Re-run
   `just test-package database` until green.
4. Private-server: add `server_groups` module, update `servers`/`statuses`/
   `devices` handlers (including the rank-bucket logic in `statuses`), run
   `just gen-openapi`, commit `openapi.json` and `api-types.ts`.
5. Public-server: add `tags` module + tests.
6. Frontend, in order so each step keeps the app compiling and usable:
   a. Regen types (already from step 4) and add `TagsEditor`,
      `ServerNameWithGroup`.
   b. Rework `ServerEdit` (group picker + notes + tags) and `ServerDetail`
      (group section, siblings).
   c. Add `GroupDetail` and `GroupEdit` routes.
   d. Replace `Servers`/`ServersList` tabs with Groups + Ungrouped.
   e. Rework `Status.tsx` to be group-keyed with ranked buckets.
   f. Update `Incidents` filter and issue-row server-name display.
   g. Update `seed.ts` and Playwright specs.
7. `unplan:` commit once the plan file is fully satisfied.

## Verification

Backend:
- `just check`
- `just test-package database`
- `just test-package private-server`
- `just test-package public-server`
- Full `just test` at the end.

Frontend:
- `just typecheck`
- `cargo build --bin private-server --bin migrate` then
  `cd private-web && npm run test:e2e` (Chromium pre-installed per AGENTS.md).

Manual smoke (the golden path):
- `just watch-private-api` + `just watch-private-web`, open
  `http://localhost:8090/`.
- Create a group, add notes + a few tags.
- Move a server into the group, set per-server tags and notes.
- Verify Incidents page filters by group.
- Verify ServerDetail shows the group's other servers.
- Hit the public tags endpoint with a device cert:
  `curl --cert dev-device.pem https://canopy.test/tags` and inspect the
  merged response (server keys override group keys).
