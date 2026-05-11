# Issues/Events/Incidents — follow-up work

Originally five features deferred from the initial `feat/issues` stack.
Item 1 (ack/resolve/snooze) has since shipped — see "shipped" note below.
This doc lays out options, tradeoffs, and a recommended starting shape for
the remaining four.

---

## 1. Ack, human-resolution, snooze — **SHIPPED**

Landed in the `feat/issues` stack. Summary of the final shape:

- `issues` carries `acknowledged_at/_by`, `resolved_at/_by`,
  `resolved_reason` (enum stored as text), and `snoozed_until`.
- `incidents` carries the same minus `snoozed_until`.
- `ResolvedReason` enum in `commons-types`: `fixed | wont_fix | expected |
  duplicate | flapping`. (Validated at the API layer.)
- **Ack** is informational only: no effect on incident membership. Ack
  persists through reopen.
- **Resolve** (human) closes the issue's contribution to the current
  incident; if it was the last contributor, the incident auto-closes too.
  Unresolve reverses: if the issue is still active+severity, it rejoins
  (or starts a new incident).
- **Device reopen after human-resolve** is Sentry-style: an `active=true`
  device event clears `resolved_at/_by/_reason` and the issue rejoins the
  incident flow. The reopen is logged in `events`.
- **Snooze** (`snoozed_until` timestamp) suppresses incident contribution.
  Setting it on a currently-in-incident issue leaves the incident
  immediately. Unsnooze re-evaluates membership.
- API surface: `/api/issues/ack`, `unack`, `resolve`, `unresolve`,
  `snooze`, `unsnooze`; `/api/incidents/ack`, `unack`, `resolve`,
  `unresolve`. UI: buttons on each row in `IssuesSection` and
  `IncidentsSection`, with inline reason/duration pickers.
- Incident resolve is metadata-only (doesn't force `closed_at`).
- Notes (issue_notes / incident_notes) are deferred to a separate stack.

The remaining four items below were originally numbered 2–5 and stay
numbered as in the original plan for continuity.

---

## 2. Operator override on incident membership

**What it is.** An issue is auto-linked to an incident by the rules in
`feat/issues`. Operator says "actually, that's a separate problem; don't
group it under this incident."

**Why it's harder than it looks.** The auto rules are stateful and run on
every event. If we just unlink, the next event will re-link. We need
some way to say "this link is operator-overridden; auto-rules don't touch
it."

**Schema options:**

- **A. Override flag on `incident_issues`.** Add
  `operator_overridden_at TIMESTAMPTZ NULL`. Auto-rules skip rows where
  it's set. To "unlink" an issue, we close the link row
  (`left_at = now()`) AND set `operator_overridden_at`. To "force-link",
  insert a new row with the override set.
- **B. Separate exclusions table.**
  `incident_issue_exclusions(incident_id, issue_id, since, reason)`.
  Auto-rules consult exclusions before linking. To re-include, delete the
  exclusion row.

(A) is more compact; (B) separates concerns cleanly. (A) wins for code
simplicity.

**API.**
- `POST /api/incidents/unlink_issue { incident_id, issue_id, reason? }`
- `POST /api/incidents/link_issue { incident_id, issue_id, reason? }`
- Possibly: `POST /api/incidents/merge { from_incident_id, into_incident_id }`
  (close the `from` incident, move its open `incident_issues` rows to
  `into`).

**Effort.** Medium. The override logic needs to thread through the save
path. Tests need to cover "operator unlinks, then device pushes again."

**Open question.** What about "split"? If an operator decides one
incident should be two, do we support that? Probably yes via "create a
new incident, manually link some issues there, unlink them from the
original". A `/incidents/split` endpoint that does both in one
transaction would be nicer UI but not strictly needed.

---

## 3. `events` retention / partitioning

**Sizing math.** Worst case: 100 devices × 1 push/minute × no coalescing
≈ 50M rows/year. Coalescing for steady-state probably cuts this by 10x;
real worst-case (a flapping issue) doesn't coalesce. Plan for 5M–50M
rows/year.

`statuses` is already partitioned weekly with a cron-maintained partition
manager (`migrations/2025-11-26-021905...` and friends); the pattern is
proven in-tree.

**Recommended approach.**

1. **Partition `events` by `created_at`, weekly.** Same pattern as
   `statuses`. Use the existing partition-management cron.
2. **Retention by partition drop.** Keep N weeks (e.g. 26 = ~6 months);
   drop older partitions. Configurable.
3. Indexes: `(issue_id, created_at DESC)` is fine within a partition. The
   "list events for issue" query usually wants recent → partition pruning
   helps.

**Don't.** Don't try to keep daily aggregates while dropping individual
rows; that's a separate analytical concern. If you want long-term
event-rate trends, ship that as a materialized view that survives the
partition drops.

**Effort.** Medium-small. The infrastructure exists in-repo, it's
copy-and-adapt. The bigger question is the retention number — needs an
ops conversation, not a code conversation.

**Open question.** Do we also want to partition `incident_issues`? Probably
not; it grows much slower (one row per issue join/leave, not per push).
`incidents` likewise.

---

## 4. Notifications / paging

**The biggest topic here.** Two axes:

- **Direction**: do we push out (webhook from canopy → external system) or
  pull in (PagerDuty/Opsgenie polls our API)? Push-out is much more common.
- **Integration**: native SDKs (PagerDuty Events API, Slack Webhooks) vs.
  generic outbound webhooks the operator configures.

**Recommended starting shape.**

A single concept: **notification channels**, attached to a server group
(root server). Each channel has a type (webhook, email, slack, etc.) and
config. When an incident opens or closes, we POST/send to all channels
configured for that group.

```
notification_channels (
  id           UUID PK,
  server_id    UUID NOT NULL REFERENCES servers(id),   -- root of group
  kind         TEXT NOT NULL,                           -- 'webhook' | 'slack' | ...
  config       JSONB NOT NULL,                          -- {url, headers, ...}
  triggers     TEXT[] NOT NULL,                         -- ['incident_opened', 'incident_closed']
  enabled      BOOL NOT NULL DEFAULT TRUE,
  created_at, updated_at
)

notification_deliveries (
  id                UUID PK,
  channel_id        UUID NOT NULL REFERENCES notification_channels(id),
  incident_id       UUID NULL REFERENCES incidents(id),
  trigger           TEXT NOT NULL,
  attempted_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  succeeded         BOOL NOT NULL,
  response_status   INT NULL,
  response_body     TEXT NULL,
  error             TEXT NULL
)
```

Delivery is best-effort with bounded retry (cron job picks up failed
deliveries to retry). Don't try to be a queueing system.

**First channel kind to ship.** Generic outbound webhook (POST JSON to a
URL). Everything else (PagerDuty, Slack, Opsgenie, email) layers on top —
the operator points their webhook at a PagerDuty integration URL.

**Open questions:**

- **Routing on severity.** Some teams want "page on critical only, ignore
  warning". Currently incidents only open at error+, so any incident is
  page-worthy. If we add per-group severity floor (item 5 below), the
  channel triggers can stay simple.
- **Routing on time-of-day / on-call rotation.** Skip — that's the job
  of the receiving system (PagerDuty/Opsgenie do this well).
- **De-dup of deliveries.** We already de-dup incidents themselves; only
  trigger once per state transition. The channel doesn't get re-pinged
  if the incident just gains/loses a contributing issue.
- **Email-without-Slack-or-PD.** Tempting but multiplies infra surface
  (SMTP config, deliverability). Defer until someone actually asks.

**Effort.** Large. Probably its own stack of 8–12 commits. The schema +
channel CRUD + delivery executor + UI for configuring channels + tests is
substantial.

---

## 5. Per-group configurable severity floor

**What it is.** The current `error` floor is hardcoded. For some groups
(prod), `error` is right. For dev/test environments, you might want
`critical` (less noisy) or `warning` (more sensitive). Per-group config.

**Schema.** Add column to `servers` (only meaningful on root, but lives on
every row for simplicity):

```
incident_severity_floor TEXT NULL  -- nullable = inherit/default
```

When evaluating "should this issue open an incident?", read the root
server's `incident_severity_floor` (fall back to global default `error`).

Or, more general: `server_config(server_id, key, value)` table for any
future per-group setting. Probably overkill for one knob.

**Open question.** Should the floor be on the *root* server only, or on
any server in the tree? My take: root only. Sub-servers shouldn't have
their own floors — the incident is rolled up to the root anyway.

**Effort.** Small. One column, one query change in `save()`, one UI knob
on the root ServerDetail page.

**Caveat.** Once we ship notification channels (item 4), the same effect
can be partially achieved by filtering at the channel level
("page-on-critical-only"). The floor is more about *whether* an incident
opens at all (and shows in the UI) vs. *whether* we page on it. They're
not the same — an opened-but-not-paged incident still surfaces in the UI
for operator review.

---

## Suggested order if/when we tackle these

- ~~Item 1 (ack/resolve/snooze)~~ — shipped in the `feat/issues` stack.
- **Notes** (separate small stack) — pre-decided next. `issue_notes` and
  `incident_notes` tables; operator free-text annotations against an
  issue/incident, plus a list endpoint and a textarea-with-history UI.
- *Item 5 deferred*: user has decided the hard `error` floor is fine for
  now. Revisit if noise becomes a problem.
- *Item 3 deferred*: revisit once we have real ingress data to size from.
- *Item 2 deferred*: revisit once we have actual usage patterns to inform
  how operators want to slice incidents.
- *Item 4 (notifications)*: design after the PR for `feat/issues` lands.
  User has many opinions here driven by existing workflow; needs its own
  conversation rather than a paper design.

Ack/resolve and severity floor are obvious "next moves". The rest can wait
for actual operator pain points to drive priority.
