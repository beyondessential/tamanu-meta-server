# Issues, Events, Incidents

Modelled on PagerDuty Events API v2 / Sentry "issue + events", with an
incident concept layered on top for server-group rollup.

## Concepts

- **Event** — a single push from a device (or operator). Append-only.
- **Issue** — long-lived entity dedup'd by `(server_id, source, ref)`. Has an
  `active` bool tracking the last reported state. Has a current severity
  reflecting the latest event's severity.
- **Incident** — opens for a server-group when an issue's state crosses the
  *incident floor* (severity ≥ error AND active). Closes when no contributing
  issue is still active. Multiple issues can contribute to one incident.

## Severity

RFC 5424 set, validated at the API layer (enum in Rust):
`emergency, alert, critical, error, warning, notice, info, debug`.

- Default severity: `error` (was `warning` in the prior alerts design).
- Incident floor: severity ≥ `error`. Below the floor, issues exist but don't
  open incidents.

## Identity & scoping

- `servers.device_id` is unique — strict 1:1 between a server and the device
  that backs it. Separate migration ships ahead of the issues work.
- Public event submission is rejected (412) if the submitting device has no
  `servers.device_id = device.id` row. Issues always have a server.
- Issue identity is `(server_id, source, ref)`. `ref` is required on event
  submission (clients that don't want dedup can mint a UUID).
- `source = "manual"` is reserved for operator-submitted events on the
  private side. Public API rejects it.

## Event API

Public (`POST /events`, ServerDevice-gated):

```json
{
  "source": "watchdog",         // required, must not be "manual"
  "ref": "disk-/var",           // required
  "severity": "error",          // optional, default "error"
  "description": "...",         // optional
  "message": "...",             // required
  "active": true,               // optional, default true
  "occurredAt": "..."           // optional RFC 3339 timestamp
}
```

Private (`POST /api/issues/submit_manual_event`, TailscaleAdmin-gated):

```json
{
  "serverId": "...",
  "ref": "...",
  "severity": "error",
  "description": "...",
  "message": "...",
  "active": true,
  "occurredAt": "..."
}
```

Source is server-set to `"manual"`.

## Event coalescing (hybrid)

Each event row carries a SHA-256 hash over `(severity, active, message,
description_or_empty)` — the user-visible fields, excluding `source`/`ref`
(constant within an issue) and timestamps (we want them to differ).

On push, look up the *latest* event for the issue:
- Same hash → bump `occurrences`, update `last_seen` (= max(occurred_at,
  created_at) of the new push). No new row inserted.
- Different hash → insert a new event row with `occurrences = 1`.

This collapses identical "still firing" pings into a single row while any
meaningful change (severity, active flip, message edit) starts a fresh run.

## Issue lifecycle

- Created on first event matching `(server_id, source, ref)`.
- `active`, `severity`, `description`, `message` track the latest event.
- `first_seen` set at creation; `last_seen` advances on every event.
- Inactivating: an event with `active: false` flips the issue to inactive.
- Reactivating: an event with `active: true` on an inactive issue flips it
  back; same `(server_id, source, ref)` identity persists. UI can show
  "reopened N times" by counting transitions in the event log.

## Incident lifecycle

Group = root server reached by walking `servers.parent_server_id` to the top.

- Issue transitions into "incident-eligible" when its current state is
  `active = true AND severity ≥ error`. A row is inserted into
  `incident_issues(incident_id, issue_id, joined_at, left_at = NULL)`. If no
  open incident exists for the group, one is opened.
- Once an issue is in an incident, severity downgrade does *not* remove it.
  The `incident_issues` row only closes when the issue goes `active = false`
  (`left_at` set to the event's effective time).
- Incident is open while any `incident_issues` row for it has
  `left_at IS NULL`. When the last contributor leaves, incident's `closed_at`
  is set.
- Reactivating an issue (active=false → active=true with severity ≥ error)
  inserts a fresh `incident_issues` row. If the group has an open incident,
  it joins that one; otherwise a new incident is opened.

## Tables

```
incidents (
  id            UUID PK,
  created_at,
  updated_at,
  server_id     UUID NOT NULL REFERENCES servers(id),   -- root of group
  opened_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  closed_at     TIMESTAMPTZ NULL
)
INDEX (server_id, opened_at DESC)
PARTIAL INDEX (server_id) WHERE closed_at IS NULL  -- "any open incident?"

issues (
  id            UUID PK,
  created_at,
  updated_at,
  server_id     UUID NOT NULL REFERENCES servers(id),
  device_id     UUID NULL REFERENCES devices(id),       -- NULL for manual
  source        TEXT NOT NULL,
  ref           TEXT NOT NULL,
  severity      TEXT NOT NULL DEFAULT 'error',          -- latest
  description   TEXT NULL,                              -- latest
  message       TEXT NOT NULL,                          -- latest
  active        BOOLEAN NOT NULL,                       -- latest
  first_seen    TIMESTAMPTZ NOT NULL,
  last_seen     TIMESTAMPTZ NOT NULL,
  UNIQUE (server_id, source, ref)
)
INDEX (server_id, last_seen DESC)
INDEX (device_id) WHERE device_id IS NOT NULL

events (
  id            UUID PK,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),      -- server receive
  occurred_at   TIMESTAMPTZ NULL,                        -- client-supplied
  issue_id      UUID NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  severity      TEXT NOT NULL,
  description   TEXT NULL,
  message       TEXT NOT NULL,
  active        BOOLEAN NOT NULL,
  hash          BYTEA NOT NULL,                          -- 32-byte sha256
  occurrences   INTEGER NOT NULL DEFAULT 1,
  last_seen     TIMESTAMPTZ NOT NULL                     -- = max(occurred_at, created_at) of last coalesced push
)
INDEX (issue_id, created_at DESC)

incident_issues (
  incident_id   UUID NOT NULL REFERENCES incidents(id) ON DELETE CASCADE,
  issue_id      UUID NOT NULL REFERENCES issues(id) ON DELETE CASCADE,
  joined_at     TIMESTAMPTZ NOT NULL,
  left_at       TIMESTAMPTZ NULL,
  PRIMARY KEY (incident_id, issue_id, joined_at)
)
INDEX (issue_id) WHERE left_at IS NULL
INDEX (incident_id) WHERE left_at IS NULL
```

## Save logic (single transaction)

For each event push:

1. Look up issue by `(server_id, source, ref)`. If absent, insert with
   `first_seen = effective_time`, `last_seen = effective_time`, fields from
   the event. Otherwise update fields, advance `last_seen` if effective_time
   is later.
2. Compute hash over `(severity, active, message, description_or_empty)`.
   Look at the latest event row for this issue:
   - Hash match → `UPDATE events SET occurrences = occurrences + 1,
     last_seen = GREATEST(last_seen, effective_time) WHERE id = latest`.
   - Hash mismatch (or no prior event) → `INSERT INTO events ...`.
3. Evaluate incident contribution against the *new* issue state:
   - Was the issue in an open `incident_issues` row before? (issue had a row
     with `left_at IS NULL`.)
   - Should it be now? (current `active = true AND severity ≥ error`.)
   - If new=true and prior=false: find or open an incident for the group,
     insert `incident_issues` row.
   - If new=false and prior=true: set `left_at` on the open row. If that
     was the last open row for that incident, set incident's `closed_at`.

`effective_time = coalesce(occurred_at, created_at)`.

## Endpoints

Public:
- `POST /events` (ServerDevice).

Private (all TailscaleAdmin):
- `POST /api/issues/list_for_device {device_id, limit?, active_only?}`
- `POST /api/issues/list_for_server {server_id, limit?, active_only?}`
- `POST /api/issues/list_events {issue_id, limit?}`
- `POST /api/issues/submit_manual_event { ... }`
- `POST /api/incidents/list_for_server {server_id, limit?, include_closed?}`
- `POST /api/incidents/list_active`
- `POST /api/incidents/get {incident_id}` — returns incident + contributing issues

## UI

- `DeviceDetail`: issues section (latest first, active by default, toggle to
  include resolved). Click → modal/page showing the issue's event log.
- `ServerDetail`: issues section for the server, incidents section for the
  group (only if this server is the root). Manual-event submit form.
- Severity chip: same colour map as before; `debug` is grey, `info`/`notice`
  blue, `warning` orange, `error`/`critical`/`alert`/`emergency` red.

## Deferred work (out of scope for this stack)

- Ack & human-resolution overrides (orthogonal columns on issues — closed
  issue can still be ack'd after the fact).
- Operator override: "this issue isn't part of that incident" (manual edits
  of `incident_issues`).
- Retention / partitioning on `events` (and possibly `incident_issues`).
- Notifications / paging integration.
- Severity floor as configurable per group.

## Migrations

1. `servers.device_id` becomes UNIQUE — ships first, on its own. Pre-merge
   verification: confirm no existing rows violate.
2. Create `incidents`, `issues`, `events`, `incident_issues` tables, indexes,
   constraints. The prior alerts migration is dropped from this stack (was
   never merged).

## Errors

New entries in `ERRORS.md`:

- `device-has-no-server` — public event submission when the device isn't
  registered against any server (412).
- `source-manual-forbidden` — public event submission with `source =
  "manual"` (400).
- `severity-invalid` — already covered by serde rejection on the enum, but
  worth a slug for the renamed error.
- `ref-required` — public event submission with missing/empty `ref` (400).
