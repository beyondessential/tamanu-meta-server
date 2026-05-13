# Status snapshots & health-driven incidents

Two related changes drawn from `TODO.txt`:

1. Surface the contemporary status payload on issues and events ("Status
   Snapshot") — a button-opens-modal (or expandable section) that loads
   the `statuses` row that was current when the event/issue happened.
2. Extend the `POST /status/{server_id}` contract with a `healthy: bool`
   and a `health: [...]` array, and drive an event/issue/incident
   through the existing pipeline when the server reports itself
   unhealthy.

These ship together because the snapshot UI is the thing that makes the
new `health[]` array legible to operators, and the `healthy` field is
the thing that gives the snapshot a useful headline.

---

## Background

Today, `POST /status/{server_id}` takes an arbitrary JSON body and
stores it verbatim in `statuses.extra` (jsonb). The `canopy` reachability
sweep (`Status::sweep_reachability`) files a single issue per server with
`source = "canopy"`, `ref = "reachability"` when a server stops pinging
in; once it returns, a recovery event closes the issue.

A status row carries: timestamp, server id, device id, optional version
string, free-form `extra` jsonb. `ServerLastStatusData` in
`private-server/src/fns/servers.rs` pulls platform / postgres / nodejs /
timezone out of `extra` for the "current" view on `ServerDetail`. There
is no way today to see *the same shape* of data as it stood at any other
point in time, nor any contract for a server saying "I'm reachable but
sick".

---

## 1. Wire format changes (public-server)

The body of `POST /status/{server_id}` gains two reserved keys:

```jsonc
{
  // ... existing free-form fields (uptime, pgVersion, timezone, …)
  "healthy": true,                 // optional; absent ⇒ true
  "health": [                      // optional; absent ⇒ []
    {
      "check": "database",         // required, identifies the check
      "healthy": true,             // required for each entry
      // ... arbitrary check-specific fields (latency_ms, message, …)
    }
  ]
}
```

Backwards-compatibility rule (from the TODO): `healthy` missing MUST be
treated as `true`. This stops every legacy server from immediately
opening a critical incident the day we deploy.

`health` missing or empty is fine and means "the server didn't break
out per-check data" — not "the server is unhealthy".

### Validation

- `healthy` if present must be a JSON bool.
- `health` if present must be a JSON array; each element must have
  `check` (non-empty string) and `healthy` (bool). Anything else on
  each entry is accepted verbatim and stored as-is.
- A `healthy: false` body with `health: []` is legal (server knows it's
  unhealthy but didn't itemise) and should still file an incident-class
  event.

Validation errors map to `400` via `AppError::custom`.

---

## 2. Database schema

Hoist `healthy` and `health` out of the `extra` blob into dedicated
columns on `statuses`:

```sql
ALTER TABLE statuses
  ADD COLUMN healthy boolean NOT NULL DEFAULT true,
  ADD COLUMN health jsonb NOT NULL DEFAULT '[]'::jsonb;
```

Rationale: keeping them in `extra` would mean every snapshot query has
to dig into jsonb to know if the row is healthy, and the canopy
event-filing code (Phase 3) would do the same on every push. Columns
are cheap and the index on `(server_id, healthy)` could matter later
for "how often does this server go unhealthy" queries (not built now,
but doesn't cost us to leave the door open).

The default on `healthy` is `true`. This means historic rows (created
before the migration) read back as healthy, which is the right answer:
they predate the contract.

`extra` keeps everything else and continues to be returned verbatim.
The public endpoint must *also* strip `healthy` / `health` keys from
the JSON before storing into `extra` (otherwise we'd have two copies
of the same data — confusing, and prone to drift if something
mutates one but not the other). Do this with a small helper that
takes the parsed JSON and pulls those two keys out.

Migration file: `migrations/<date>_statuses_health/up.sql` &
`down.sql`. No data backfill needed beyond the column defaults.

---

## 3. Backend: parse + persist + file events

### Parsing in the public endpoint

In `crates/public-server/src/statuses.rs`, change the handler to:

1. Parse the incoming body into a small struct (or work with
   `serde_json::Value`) that pulls out `healthy` (default true) and
   `health` (default `[]`), then re-collects the rest as the `extra`
   blob.
2. Validate the `health` array shape (per Phase 1).
3. Insert a `NewStatus` extended with `healthy: bool` and `health:
   serde_json::Value`. Update `database::statuses::NewStatus` &
   `Status` model accordingly.
4. After the insert (in the same handler), if the *previous* status
   for this server was healthy and the new one is not (or vice versa),
   file a canopy-sourced event. See "Filing logic" below.

The "previous status" lookup is one extra query — `Status::latest_for_server`
already exists, but it's called *after* the new row is inserted in
order to compare. Easier: fetch the previous-latest *before* the
insert (or in the same transaction), compare against the new payload,
then file. Wrap in a transaction so we don't end up with a stored
status but a missing event push if the network blips mid-call.

### Filing logic — when to push a NewEvent

Guiding principle (user's wording): **trust the reporter**. The server
decides what's healthy. A `healthy: false` *check* with `healthy: true`
*overall* is the server saying "something is degraded but I haven't
escalated it yet" — that's a warning, not an incident. A top-level
`healthy: false` is the server saying "I'm in trouble" — that's an
error, even if `health[]` is empty.

Two kinds of issues come out of one status push, with different
`(source, ref)`:

- **Roll-up**: `source = "status"`, `ref = "health"`. One issue per
  server reflecting top-level `healthy`. Always `Error` severity —
  this is the thing that drives incident open/close.
- **Per-check**: `source = "status"`, `ref = format!("health/{check}")`.
  One issue per (server, check). Severity depends on the *current
  push's* top-level: `Warning` if top-level `healthy: true`, `Error`
  if top-level `healthy: false`. Per-check Warning events won't open
  an incident on their own, but they'll join an existing one (the
  incident-membership rules already allow any-active-issue-joins-open
  incident, just not low-sev creation).

We re-evaluate severity on every push — so a check that was previously
Error (when top-level was unhealthy) is naturally downgraded to Warning
on the next push that has top-level healthy again, since `NewEvent::save`
updates the issue's severity to whatever's in the new event.

#### Algorithm per push

The handler needs the *previous* latest status for the same server to
know which checks just transitioned. Fetch it before the insert (one
extra `latest_for_server` call — cheap), then:

1. Compute previous state:
   - `prev_top_healthy: bool` — `true` if no prior status row, else
     prior row's `healthy` column.
   - `prev_failing_checks: Set<String>` — names of checks in prior
     row's `health[]` where `healthy: false`. Empty if no prior row.
2. Insert the new status row (with its `healthy` and `health` values).
3. Compute current state:
   - `curr_top_healthy: bool` — the new row's `healthy`.
   - `curr_failing_checks: Set<String>` — failing checks from this
     push's `health[]`.
4. **Roll-up event filing**:
   - If `curr_top_healthy = false`: file `("status", "health")`,
     `active = true`, `severity = Error`. (NewEvent::save coalesces if
     the issue is already open and unchanged; otherwise opens or
     updates.)
   - Else if `prev_top_healthy = false` (i.e. transitioned to healthy):
     file `("status", "health")`, `active = false`, `severity = Info`.
   - Else: no roll-up event.
5. **Per-check event filing**: pick the severity once —
   `sev = if curr_top_healthy { Warning } else { Error }`. Then:
   - For each `check` in `curr_failing_checks`: file
     `("status", format!("health/{check}"))`, `active = true`,
     `severity = sev`.
   - For each `check` in `prev_failing_checks − (set of checks
     present in current `health[]` regardless of healthy)`:
     the server stopped reporting this check, treat as recovered —
     file `active = false`. (Trusting the reporter: if you drop a
     check from your output, you're saying it no longer applies.)
   - For each `check` in `prev_failing_checks ∩ (current passing)`:
     the server explicitly reports it as fixed — file `active = false`.
   - Checks that newly appear *and* are healthy: no event (we don't
     create resolved-from-birth issues; nothing to track).

Step 5's three sub-cases collapse cleanly to: walk the union of
`prev_failing_checks` and `curr_failing_checks`; if the check is in
current-failing, file active-true at `sev`; else file active-false.

#### Event ordering within a single push

One status push can fan out to *multiple* event filings — a roll-up
recovery and several per-check transitions at once. Each
`NewEvent::save` call independently runs
`re_evaluate_incident_membership`, which can close an incident when
its last active contributor leaves. Naïvely processing closes before
opens leads to a (transient) close+reopen flicker, and worse: if the
second event is `Warning` severity, it won't *reopen* a closed
incident, so we'd lose incident continuity.

Rule, mirroring the user's framing: **if any event in a push would
keep an incident open or contribute a new opener, the incident stays
open.** To get there with the existing per-event re-eval, the handler
files events in this order:

1. **All `active = true` events first**, severity-descending so any
   Error-class event lands before any Warning-class one. This
   establishes / refreshes the incident: as soon as the first Error
   opens (or re-finds) the incident, all subsequent active-true
   events join via the `group_open` branch even if their severity
   alone wouldn't open one.
2. **All `active = false` events last.** By the time these run, the
   incident already has its new contributors, so each leave's
   "remaining_open == 0?" check sees the right denominator.

Wrap steps 2–5 of the per-push algorithm in one outer transaction
(in the public-server handler). `NewEvent::save` already opens its
own transaction; on `diesel_async` that nests as a savepoint, which
is fine. The outer transaction guarantees: either the status row,
all events, and the resulting incident state all commit together, or
nothing does.

There remains a cross-handler race we're *not* trying to close in
this plan: the status push and the reachability-sweep job run in
different connections. If the sweep happens to fire a reachability
*recovery* event in the millisecond window between this handler
inserting the status row and committing its health events, the
sweep's transaction will momentarily see "no health contributors
yet". The actual incident transition is single-row at the DB level
so we won't end up with two open incidents for the same group, but
in theory the incident could close+reopen instead of staying open.
Acceptable for now; if it shows up in practice we'd take a `FOR
UPDATE` lock on the incident row at the top of each `NewEvent::save`
to serialise.

#### Why this split (roll-up + per-check)

The roll-up issue is what an operator wants to glance at: "is this
server's self-reported health currently OK?" One row, one ack, one
resolve.

The per-check issues are what an operator wants to *drill into*: which
specific check is failing, what extra context did the server attach,
when did it start. Each per-check issue can be acked/resolved/snoozed
independently — useful for noisy or flapping checks.

Together they mirror a familiar pattern: incident = "service is down",
contributing issues = "specifically because the DB is down AND the
disk is full". The TODO line "trigger an event/issue/incident (in the
usual manner)" reads naturally with this split.

#### Message / description copy

Roll-up unhealthy event:

- `message`: `"Server reports unhealthy"` (server name comes from the
  issue's joined server data in the UI).
- `description`: if `health[]` had failing entries, a markdown bullet
  list of failing check names; otherwise `None`.

Roll-up recovery event:

- `message`: `"Server reports healthy"`.
- `description`: `None`.

Per-check failing event:

- `message`: `format!("Health check '{check}' failed")`.
- `description`: a small markdown rendering of the entry's
  check-specific fields (everything other than `check` and `healthy`),
  formatted as key: value lines. Empty/None if the entry has no
  extras.

Per-check recovery event:

- `message`: `format!("Health check '{check}' recovered")`.
- `description`: `None`.

### Interaction with reachability

The existing reachability sweep stays under `source = "canopy"`,
`ref = "reachability"` — different source, different ref, no overlap.
Reachability is canopy's external view ("did we hear from you");
status/health is the server's self-report. Both can be open at once;
both can join the same incident (incident is per server group).
Nothing to change in `sweep_reachability`.

A subtle case: a server that's been silent for a while sends one
`healthy: false` ping. The reachability sweep will close the
`canopy/reachability` issue on its next pass (the server's most recent
status is now within the `Up` window); the `status/health` roll-up
opens.

Concretely, the active incident **should remain open across this
transition** — it's one continuous "this server is broken" event from
the operator's perspective, even though the underlying signal shifted
from "we can't hear you" to "you're telling us you're sick". The
existing logic delivers that: the `status/health` roll-up fires at
Error severity from the public handler, joins (or re-finds) the
group's incident; later, when the sweep closes the reachability
issue, its `re_evaluate_incident_membership` pass sees the health
issue still contributing and leaves the incident open. The
"opens-first" ordering rule above is the in-handler analogue of the
same idea.

---

## 4. Snapshot endpoint (private-server)

### Data model lookup

Add `Status::at_time(db, server_id, at: Timestamp) -> Option<Status>`:
"most recent status row for this server with `created_at <= at`". Use
a `LIMIT 1 ORDER BY created_at DESC` query mirroring
`latest_for_server`. No 7-day window cap here — operators reviewing
old issues want the truth, not "nothing because the matching row is
old". The performance shape is identical to `latest_for_server`.

### Wire shape

Re-use a single struct (`StatusSnapshotData`) for both "the latest
status" view and "the snapshot at time T" view; extend
`ServerLastStatusData` to include the new fields *or* introduce a
fresh `StatusSnapshotData` and have `ServerLastStatusData` be a
subset (avoids forcing the ServerDetail view to render checks it
doesn't care about). Lean toward a fresh `StatusSnapshotData` —
ServerDetail can stay how it is for now, and the snapshot endpoint
returns the richer shape.

Fields to include in `StatusSnapshotData`:

- `id`, `created_at`, `server_id`, `device_id` (raw)
- `version`, `version_distance` (computed against latest known
  release, same as today)
- `min_chrome_version` (already computed in `ServerLastStatusData`)
- `platform`, `postgres`, `nodejs`, `timezone` (extracted from `extra`
  or from device connection, same as today)
- `healthy: bool`, `health: Vec<HealthCheck>` (new)
- `extra: serde_json::Value` (the rest of the payload, unchanged)

### Endpoint

`POST /api/statuses/snapshot` with body `{ server_id, at? }`:
- `at` missing → return latest status (same row a `latest_for_server`
  would yield).
- `at` present → return most recent prior to that timestamp.

Returns `Option<StatusSnapshotData>` — `null` if the server has no
status row at or before `at` (e.g. issue predates first ping). Frontend
must tolerate that and render "no snapshot available".

### Auth

`TailscaleUser` — same gate as the rest of the issues/events views.
Admin not required to view a snapshot.

---

## 5. Frontend

Three coordinated UI changes: a snapshot modal launched from issues,
extensions to the existing ServerDetail latest-status view, and a
small but high-signal tweak to `<StatusDot>` so a server's reachability
and health show up in one glance.

### 5.1 `StatusSnapshotModal`

New component in `private-web/src/components/`. Props: `serverId`,
`at: string | null`, `open`, `onClose`. Fetches via
`useApi("statuses", "snapshot", …)` on open.

Modal contents:

- Header: server name (link to `/servers/<id>`), status timestamp
  with `<TimeAgo>`, prominent healthy / unhealthy indicator (use a
  `<Chip>` with `color="success"` or `"error"`).
- Curated fields grid (same look as the InfoSection on ServerDetail):
  Tamanu version + distance (via `<VersionIndicator>`), Platform,
  PostgreSQL, Node.js, Timezone, min Chrome.
- Health checks section: if `health[]` non-empty, render each as a
  small bordered box — check name, pass/fail dot, then a key/value
  block for that entry's extra fields. Sort failing first so they're
  visible without scrolling.
- Raw extras: a `<details>` showing the `extra` jsonb pretty-printed,
  matching ServerDetail's existing "Extra Data" pattern. (This is the
  forward-compat hatch for "we're about to send a lot more".)

Wire the modal in two places:

1. **IssueRow header** (in `IssueRow.tsx`): a small "snapshot" icon
   button between the source chip and the time. Clicking opens the
   modal at `issue.last_seen`. This is the "what's it doing now"
   answer for an active issue, and "what was it doing when it last
   fired" for a closed one.
2. **EventLog rows** (also in `IssueRow.tsx`): same small button at
   the right of each event row, opens the modal at
   `event.occurred_at ?? event.created_at`. This is the "what
   precisely was the system doing at this event" answer.

### 5.2 ServerDetail InfoSection — checks table

On ServerDetail, the existing `InfoSection` shows the curated
last-status fields plus a `<details>` block for `extra`. Extend it
with:

- A **global health indicator** rendered prominently in the section
  header. Mirrors the modal's chip: `<Chip color="success">healthy</Chip>`
  if `last_status.healthy === true`, `<Chip color="error">unhealthy</Chip>`
  if `false`. If `last_status` is absent (server has never pinged in),
  no chip.
- A **checks table** below the curated fields, *before* the existing
  Extra Data details. Columns: check name, status (icon: green check
  / red cross), and per-row a small key/value block of the entry's
  extra fields (everything in the entry other than `check` and
  `healthy`).
- Sort: failing first, then alphabetical by check name.
- Visible height capped at ~5 rows. When `health.length > 5`, show
  the first 5 (after sorting, so all failing rows are guaranteed
  visible if there are ≤5 of them) with an "expand to show all"
  toggle underneath. Keep layout stable for fleets that ship 30+
  checks.
- If `health[]` is empty, no table — just the existing fields and
  Extra Data section. (No false implication that we have per-check
  data when we don't.)

The Extra Data `<details>` block stays. It continues to be the
catch-all dump of the raw status payload, useful as the contract
expands.

### 5.3 `<StatusDot>` border for health state

`<StatusDot>` today renders a single solid dot whose colour comes
from `ShortStatus` (`up`/`away`/`blip`/`down`/`gone`). That collapses
two distinct signals — *can we reach you* and *do you say you're
sick* — into one channel. With the new contract we have two
independent dimensions; render both in one dot:

- Fill colour: unchanged. Driven by `ShortStatus`.
- Border: new. Three states, only meaningful when the server is
  reachable (when not reachable, no border):
  - **Healthy** (`healthy: true`, no failing checks): no border.
  - **Warning** (`healthy: true`, at least one `health[]` entry with
    `healthy: false`): thick orange border (`warning.main`).
  - **Unhealthy** (`healthy: false`): thick red border (`error.main`).

So a server that's reachable but reports itself sick reads as a green
dot in a red ring — both states visible without needing a tooltip. A
reachable-and-degraded server (top-level healthy, one check failing)
is a green dot in an orange ring. Unreachable servers keep their
existing colour scheme and pick up no border (the absent ping carries
no health signal we'd want to amplify).

The border draws inside the existing dot footprint so adjacent dots
don't reflow when state changes. Use `outline` rather than `border`
to avoid the box-model shift, or pre-allocate the width via padding.

#### Wire-data plumbing for the dot

To render the border server-side, the wire payload for every place a
`StatusDot` appears needs a `health_state` field alongside the
existing `up: ShortStatus`. Introduce a small enum in `commons-types`:

```rust
pub enum HealthState {
    Healthy,    // most recent status: healthy=true, all checks pass
    Warning,    // most recent status: healthy=true, ≥1 check failing
    Unhealthy,  // most recent status: healthy=false
}
```

(No `Unknown` variant — pre-migration rows default to `healthy=true`
with `health=[]`, which is correctly classified as `Healthy`. Servers
with no status row at all also default to `Healthy`; the dot's
reachability fill already conveys "gone" in that case, so we don't
need a separate "unknown health" state.)

Surface `health_state` everywhere `up: ShortStatus` is currently
surfaced. That's at least:

- `ServerDetailData` (root server + per `child_servers` tuple)
- `CentralServerCard` + `FacilityServerStatus` (in
  `commons-types/src/server/cards.rs`) — used by the `/status` page.

Compute it from the same `Status` row that's already being fetched
for the `up` calculation, so no extra DB hits.

Frontend: `<StatusDot>` grows a `health?: HealthState` prop. Every
call site that has the data passes it; sites that don't, omit it
(falls back to no border, matching today's behaviour).

---

## 6. Wire-types regeneration

After the Rust changes:

```
just gen-openapi
```

Commit `private-web/openapi.json` and `private-web/src/api-types.ts`
alongside the Rust diff. The hand-written `private-web/src/types.ts`
re-export shim gets a new line for `StatusSnapshotData`.

---

## 7. Tests

### Public-server (`crates/public-server/tests/statuses.rs`)

Extend the existing fixtures with:

- `submit_status_with_healthy_true_no_checks` — body has
  `healthy: true`, empty/missing `health`. Assert: no `status/*`
  issues opened.
- `submit_status_legacy_no_healthy_field` — body with no `healthy`
  key at all (legacy server). Assert: no `status/*` issues opened
  (regression guard for the "absent ⇒ true" rule).
- `submit_status_warning_check_only` — `healthy: true` but `health[]`
  contains one failing entry. Assert: a `status/health/<check>` issue
  exists, severity `Warning`, active; the roll-up `status/health`
  issue does NOT exist; no incident is opened (Warning alone doesn't
  cross the threshold).
- `submit_status_unhealthy_with_checks` — `healthy: false` with two
  failing checks and one passing. Assert: the roll-up
  `status/health` issue exists at `Error` severity; both failing
  per-check issues exist at `Error` severity; the passing-check has
  no issue (no resolved-from-birth row); an incident is open for the
  server group.
- `submit_status_unhealthy_no_checks` — `healthy: false` with empty
  `health[]`. Assert: only the roll-up `status/health` issue exists,
  no per-check issues; incident opens.
- `submit_status_severity_demotion` — first push `healthy: false` with
  a failing check (per-check issue lands at Error). Second push
  `healthy: true` with the same check still failing. Assert: per-check
  issue severity is now `Warning`; the roll-up issue closed; the
  per-check is still active.
- `submit_status_check_recovery_dropped` — first push has check `foo`
  failing; second push omits `foo` from `health[]` entirely. Assert:
  the per-check issue's most recent event is `active=false` (server
  implicitly cleared it).
- `submit_status_check_recovery_explicit` — first push has check
  `foo` failing; second push has `foo` with `healthy: true`. Assert:
  same recovery outcome as above.
- `submit_status_full_recovery_closes_incident` — push `healthy:
  false` with failing checks, then push `healthy: true` with all
  checks passing. Assert: roll-up resolves, all per-check issues
  resolve, incident auto-closes.
- `submit_status_invalid_health_entry` — `health` element missing
  `check` → 400.
- `submit_status_keeps_incident_open_across_roll_up_recovery` — set
  up: a prior push with top `healthy: false` and check `db` failing
  (incident open, two contributing issues). Then push top `healthy:
  false` with check `db` recovered but a *new* check `disk` failing.
  Assert: incident is still open (handler ordered the disk-open
  event before the db-close event); both the roll-up and the disk
  issue contribute; the db issue has resolved.
- `submit_status_reachability_to_health_handoff` — set up: server
  unreachable, `canopy/reachability` issue at Error, incident open.
  Then submit a status push with `healthy: false`. Assert: the
  same incident now has both the reachability issue and the
  `status/health` roll-up as contributors; after the reachability
  sweep next runs (simulated by directly invoking
  `Status::sweep_reachability`), the reachability issue closes but
  the incident stays open because the health roll-up is still
  contributing.

### Database

- `Status::at_time` unit test: insert three statuses 1h apart, query
  with `at` set to midway timestamps, verify the right row comes back
  for each query. (Database-only test via `TestDb::run`.)

### Private-server (`crates/private-server/tests/statuses.rs` —
create if absent)

- Snapshot endpoint with `at = null` returns the latest row.
- Snapshot endpoint with `at` mid-history returns the prior row.
- Snapshot endpoint with `at` before any row returns `null`.

### Frontend

Skip Playwright for this round — the modal is straightforward UI on
top of existing patterns, and the value of an E2E test here is low
compared to the cost of the fixture changes. The Rust-side tests
cover the contract.

---

## 8. Out of scope (deferrals)

- **Caller-supplied severity per check.** The contract takes a
  `healthy: bool` per entry; the *severity* of the resulting event is
  picked by canopy from the top-level flag, not from the payload. The
  current shape will be more than enough.
- **Index on `(server_id, healthy)`.** Don't add prophylactically;
  the queries we file in this plan all use `created_at DESC`. Wait for
  a query that actually wants it.

Status-timeline / health% chart is also out of scope here; the rough
sketch lives in `TODO.txt` for a future refinement pass.

---

## Implementation order

The phases above are roughly the implementation order. Suggested
commit boundaries (using the user's commit-as-you-write workflow):

1. `feat: add healthy/health columns to statuses` — migration + model
   changes, no behaviour change yet (defaults preserve legacy).
2. `feat: parse healthy/health from /status payload` — public endpoint
   reads & persists the new fields, strips them from `extra`. No
   event filing yet. Backwards-compat test passes.
3. `feat: file status/health events on healthy transitions` — wire
   the roll-up (`status/health`) + per-check (`status/health/<check>`)
   event filing in the public-server handler, including the
   previous-status lookup. Covering tests.
4. `feat: status snapshot endpoint` — `Status::at_time` + private-server
   handler + openapi regen.
5. `feat: HealthState on server wire data` — extend `ServerDetailData`,
   `CentralServerCard`, `FacilityServerStatus`, plus the new
   `HealthState` enum in `commons-types`. No UI change yet.
6. `feat: StatusDot health border` — `<StatusDot>` consumes the new
   prop, every call site passes it.
7. `feat: ServerDetail health indicator + checks table` — global
   chip in the InfoSection header, sortable/capped checks table.
8. `feat: status snapshot modal` — React component + wire into
   IssueRow header and EventLog.

Each commit should leave the working tree green (tests/lints).
