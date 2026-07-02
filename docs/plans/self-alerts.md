# Self-alerts: direct Slack path + distinct UI

Canopy's alerts about its own operation (backup IRSA identity broken, Slack
delivery failing, MCP token nearing expiry) file coalescing issues against
the nil "Canopy" server. That server will never be grouped or displayed, so
today those alerts page nobody and render in the UI as rows for a
pseudo-server. This plan gives them their own delivery path and their own
surface.

Builds on the two nil-server commits already on this branch (token-expiry
and preflight-identity moved off the per-group fan-out).

## Concept

A **self-alert** is an issue on the nil server: one condition, one
coalescing `(canopy, <ref>)` issue, active while the condition holds.
Current refs: `preflight-identity` (Critical), `mcp-token-expiry` (Error),
`slack-delivery-failure` (Error, recovers only by operator resolve).

## Delivery — a non-incident outbox kind

- Migration: `slack_outbox.incident_id` becomes nullable (self-alert rows
  have no incident). `SlackOutbox::enqueue` takes `Option<Uuid>`; existing
  callsites wrap in `Some`.
- New kind `self_alert`, one webhook for both directions; the payload
  carries the state: `{state: "alert"|"recovered", severity, source_ref,
  title, message}`. The drainer injects `link = {PRIVATE_URL}/alerts`
  (kind-aware: incident kinds keep their incident deep-link).
- Config: `SLACK_WEBHOOK_SELF_ALERT_URL`, deliberately NOT part of the
  all-hooks-or-none set — adding it there would hard-fail existing deploys
  whose env predates the var. Unset while incident hooks are set → loud
  startup warning + per-kind noop (rows marked delivered without POSTing,
  same as global noop). Revisit folding it into the strict set once ops
  config has it everywhere.
- Flap grace, mirroring incidents but keyed on `issue_id` (the outbox
  column already exists): opens below Critical get `deliver_after = now +
  3 min`; Critical ships immediately. Recovery first cancels a still-pending
  open (by issue id) and skips the resolve entirely when it cancelled one —
  a sub-grace flap makes no Slack noise at all.

## Raising — `database::self_alerts`

`raise(conn, ref, severity, title, message)` and `recover(conn, ref,
message)`:

1. `NewEvent::save` against the nil server (unchanged coalescing).
2. On the inactive→active transition only, enqueue the `self_alert` open;
   on active→inactive, cancel-or-resolve as above. No transition, no row —
   repeated raises while active stay Slack-silent.

Rewire the three producers: `sweep_token_expiry`, preflight's
`file_identity_alert`/`recover_identity_alert`, and the drainer's
`file_self_event` (which gains banner visibility; its enqueue self-loop
terminates because a re-raise while active enqueues nothing).

## Distinct UI

- `fns/self_alerts.rs`: `active` (for the banner) and `list` (recent,
  with events, for the page), both any-tailnet-user reads.
- App-level banner: when any self-alert is active, a severity-colored MUI
  Alert renders above the routed content on every page, naming each active
  alert (title + since), linking to `/alerts`.
- `/alerts` page: active alerts with their message bodies, then recent
  recovered/resolved history. Not under Settings — it's state, not config;
  reachable via the banner (no permanent nav slot: when nothing is wrong
  the page is empty and unvisited).
- Nil-server issues stop rendering as server rows: `Issue::list` (the
  global Incidents-page feed) excludes the nil server; the alerts page
  reads them through its own endpoint.
- Playwright: seeded active self-alert → banner shows on any page, links
  to `/alerts`, page lists it; recovered alert leaves the banner.

## Spec

New `.workhorse/specs/private-server/self-alerts.md` (id `SELF`): what a
self-alert is, singular-per-condition coalescing, notification without the
per-group incident flow (with flap grace), distinct presentation apart from
fleet issues, and the current condition catalogue. Amend the MCP spec's
token-expiry sentence to point at the mechanism by name.

## Done when

- Identity breakage / token expiry produce exactly one Slack message each
  on raise (after grace; Critical immediate) and one on recovery, with no
  per-group incidents and no fan-out.
- Sub-grace flaps produce zero Slack messages.
- With `SLACK_WEBHOOK_SELF_ALERT_URL` unset, the drainer warns and no-ops
  the kind; nothing accumulates.
- Active self-alerts banner every admin-UI page and list on `/alerts`;
  the Incidents page no longer shows nil-server rows.
- Spec, openapi artefacts, tests (database enqueue/transition, drainer
  routing/config, Playwright) updated.
