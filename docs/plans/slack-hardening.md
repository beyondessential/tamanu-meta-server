# Slack integration hardening

Follow-up work after auditing why some incident closes weren't reaching Slack.
The symptom that started this: a row marked `delivered_at` in `slack_outbox`
for a `KIND_INCIDENT_RESOLVE` event, but Slack had no record of the trigger
arriving. The audit also surfaced several adjacent issues worth fixing in the
same pass.

## Drainer changes (`crates/jobs/src/bin/slacker_outbox.rs`)

### 1. Strict config — refuse to start on partial setup

Today the drainer accepts any subset of (`SLACK_WEBHOOK_OPEN_URL`,
`SLACK_WEBHOOK_RESOLVE_URL`) and silently drops rows for missing kinds with a
`debug!` line. That's how the symptom got hidden.

New policy: read env once at startup, then validate that **either all known
kinds have a URL set, or none do**. If any are set, all must be set, and
`PRIVATE_URL` must be set. Otherwise refuse to start. Preserve "no hooks set"
no-op mode for dev environments.

Remove the `url_for() → None → Ok(())` branch in `deliver()`. With the
startup check above, this branch can never legitimately fire — the only way
to reach it would be a future new `kind` added without a URL config, which
should be a hard error not a silent drop.

### 2. Capture and persist Slack response body

Right now we only check `status.is_success()`. A 2xx response from a
Workflow Builder webhook doesn't mean the workflow ran successfully — Slack
returns 2xx for the *trigger acceptance*, not for downstream step success.

Change:

- Add a `last_response` column on `slack_outbox` (or fold into `last_error`
  with a tagged value — leaning toward a separate column for clarity).
- After every POST, capture `resp.text()` and store it on the row alongside
  `delivered_at` / `last_error`.
- Log the response body at `info!` so operators can see what Slack actually
  said without digging through the DB.

### 3. Max-attempts terminal state + proper logging

`mark_failed` only bumps `attempts` and sets `last_error`. After
`attempts >= MAX_ATTEMPTS` the current code logs "giving up" and calls
`mark_failed` again — which doesn't take the row out of the pending set, so
it keeps being claimed every 5s indefinitely.

Change:

- Add a `gave_up_at` column (or reuse `delivered_at` with a sentinel — a new
  column reads cleaner) so the row leaves the pending set.
- Filter `claim_pending` on `attempts < MAX_ATTEMPTS AND gave_up_at IS NULL`
  to avoid the churn.
- `warn!` on transient delivery failure, `error!` once when a row is moved
  to the terminal state. Include the row id, kind, incident_id, and last
  error message in both.

### 4. Reqwest timeout + heartbeat watchdog

Two ways the drainer can wedge:

1. The reqwest client has no timeout configured, so a black-holed network
   call can hang the batch transaction forever.
2. A logic bug or downstream deadlock can stall the loop with no external
   signal (the JoinHandle just never completes).

Change:

- Configure a request timeout on the reqwest client (`.timeout(...)` at
  build time, probably 10–15s).
- Track a `last_tick: AtomicI64` (millis-since-epoch) updated at the top of
  each loop iteration in the main drainer task. Spawn a watchdog task that
  wakes every 30s and checks the delta. If stale by more than e.g. `3 ×
  TICK + request_timeout + buffer`, log `error!` and `std::process::exit(1)`
  so Kubernetes restarts us. Surface in the operator UI via the standard pod
  restart count.

## Self-monitoring (slack-on-slack)

When a row hits the terminal failure state (max attempts exceeded), file a
`canopy/slack-delivery-failure` event against the nil-UUID server via
`NewEvent::save(.., Uuid::nil(), None)`. The issue surfaces in the existing
issues/incidents UI like any other monitored failure.

To prevent the obvious feedback loop (failure to send Slack → file event →
opens incident → tries to send Slack → fails → files event…) add a guard at
the slack outbox enqueue boundary:

> If `incident.server_id == Uuid::nil()`, skip the Slack enqueue.

The nil server is the canopy meta-server: reachability sweep already excludes
it from monitoring, and treating it as "no Slack notifications" matches that
intent.

## Database changes (`crates/database/src/issues.rs`)

### 5. Concurrent-leave race in `re_evaluate_incident_membership`

Two transactions each removing the last live issue on a single incident can
both observe `remaining_open >= 1` (each sees its own in-flight `left_at`
but not the other's), commit, and leave the incident in a "no live issues
but `closed_at IS NULL`" state with no Slack message fired.

Fix: take a row-level `FOR UPDATE` lock on the `incidents` row before
counting `remaining_open` — or replace the read-then-write with an atomic
conditional `UPDATE incidents SET closed_at = $1 WHERE id = $2 AND
closed_at IS NULL AND NOT EXISTS (SELECT 1 FROM incident_issues WHERE
incident_id = $2 AND left_at IS NULL) RETURNING …`. Either serializes
concurrent closes correctly. Leaning toward the explicit lock for
readability.

This is defensive — we haven't seen it happen in production. But the same
code path is where the cascade close lives, and now we know how badly
silent-drop bugs hide.

### 6. Operator attribution through cascade close

When an operator's `Issue::resolve` or `Incident::resolve` causes the
last live issue to leave, the cascade close fires via
`enqueue_slack_cascade_close(_, None)` → Slack message says "by:
automation". The operator's name is lost.

Fix: thread `Option<&str>` operator login through
`re_evaluate_incident_membership` so the cascade close can credit the
operator when known. Device-driven cascades (e.g. `active:false` from a
public-server push) continue to pass `None` → "automation".

## Out of scope

- The `Incident::unresolve` path that clears `resolved_*` but never touches
  `closed_at`. Behavior is intentional; UI semantics already match.
- Snooze-expiry sweep (lazy unsnooze on next write is fine for now).
- Server-demotion-from-`alert_when_down`-while-incident-open. Operator
  resolves manually; not a Slack delivery issue.

## Implementation order

1. **#1 strict config + #4 terminal state** — directly addresses the
   reported symptom. Drainer changes only.
2. **#2 response body capture** — makes future "2xx but didn't actually
   deliver" cases self-evident.
3. **#5 reqwest timeout + watchdog** — defense against drainer wedge.
4. **#6 self-monitoring via nil server** — needs the nil-server enqueue
   guard wired first to avoid feedback loop.
5. **#7 concurrent-leave race**.
6. **#8 attribution threading**.

Test coverage to add as we go:

- Drainer: rejects partial config at startup; rejects unknown kind at
  delivery; row reaches terminal state after N attempts; response body
  ends up on the row.
- Database: concurrent-leave race (two tasks each resolving one of two
  live issues at once) reliably closes the incident. Attribution carries
  through cascade close.
