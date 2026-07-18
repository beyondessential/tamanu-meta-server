# Close–reopen noise: linger and trend-based damping

Exploration of how to reduce close-then-immediately-reopen alert noise, on
top of the incident-pipeline rework (CHK/INC/STA). Stage 1 (the static
linger) is implemented; the staging section at the end tracks the rest.

## The problem: grace is one-sided

The open side is handled. An incident's Slack open sits in the outbox for
the group's `slack_open_delay` (default 3 minutes); an incident that closes
inside that window cancels the open and skips the resolve, so a briefly-red
check never reaches Slack.

The close side has nothing. The moment the last effective failure leaves —
one green report is enough — the incident closes and the resolve ships. When
the check goes red again two minutes later, that is a *new* incident row:
fresh grace, fresh open message, fresh escalation budget.

So a red check that blips green costs, per blip:

- a **resolve** message ("all clear") that was wrong within minutes;
- a new **open** message one grace-period later;
- for an escalating check, potentially a fresh **Critical** open, since
  `escalated_at`'s once-per-incident cap resets with each new row;
- a fragmented record: one span of trouble becomes N incident rows, which
  distorts incident counting, duration stats, and MCP reporting.

There is also an inverse failure mode hiding in the current design: a check
that flaps red/green *faster* than the grace period never accumulates a
continuous red span longer than grace, so each of its incident rows closes
within its own window and is cancelled. The target can be in trouble for
hours and Slack never hears anything at all. Both failure modes have the
same root cause — incident identity is tied to continuous redness rather
than to the span of trouble.

## The simple fix: linger (close-side grace)

Make the close symmetric with the open. When the last effective failure
leaves an open incident, the incident does not close; it starts **lingering**
(stamp `closing_at`). Two ways out:

- An effective failure joins (or rejoins) the target while lingering →
  clear `closing_at`; the *same* incident continues. No resolve was sent,
  no new open is enqueued, `escalated_at` and the pending-open row carry
  over. Issue join/leave timeline entries record the blip.
- The linger window elapses with no failure → finalize: set `closed_at`
  **backdated to `closing_at`** (the truthful end of trouble), then run
  exactly today's close path — cancel a still-pending open, or ship the
  resolve.

The finalizer is a minute-cadence DB-only sweep, which is exactly what the
monitor pod's loop is for (`sweep_staleness` et al.); a
`sweep_lingering_incidents` rides the same tick.

### Interaction with the open-side grace

The subtle part. Today the close cancels a pending open immediately; with
linger, cancellation moves to finalize. That changes what the open delay
measures, deliberately:

- The pending open row keeps its original `deliver_after`, counting from
  when the incident first opened.
- The drainer must **not ship an open while its incident is lingering** —
  otherwise a single 30-second blip would notify at the 3-minute mark
  purely because the linger held the incident open past its grace. A
  drainer-side gate (skip `incident_open` rows whose incident has
  `closing_at` set) is stateless and enough.
- If the incident finalizes closed before the open ever shipped → cancel,
  as today. A genuine one-off blip stays silent.
- If a failure rejoins and the row is past `deliver_after` → it ships on
  the next drain tick.

Net effect: an incident publishes once it is **older than grace and
currently red**. That fixes the fast-flapper hole as a side effect — a
check flapping every couple of minutes keeps one incident row alive
(rejoining within linger), so three minutes after the trouble started the
open ships, once. Today that scenario ships nothing forever.

Escalating checks are unaffected on the open side (they bypass grace and
linger alike: `deliver_after = now`); on the close side they benefit the
most, since one surviving row means at most one Critical re-notify per span
of trouble instead of one per blip.

### Cost and knob

The only behavioural cost is that resolves arrive up to one linger window
late. That's cheap against a resolve+reopen pair, but it argues for a
modest default, not a huge one.

Knob: `slack_close_delay INTERVAL` on `server_groups` next to
`slack_open_delay` (plus a matching const for the global target). Default
somewhat larger than the open delay — 5 minutes — because green→red→green
oscillation is typically slower than the report cadence. Name it in UI
terms as "linger" or "hold-open", not "close delay" (nothing about the
close is being hidden; the incident is genuinely still suspect).

Spec impact (INC): "The incident closes when its last effective failure
leaves" becomes "…when its last effective failure has been gone for the
target's linger window; a failure returning within the window continues the
same incident; the close is recorded as of when the last failure left."

## Trend-based damping: beyond one hard threshold

A single fleet-wide linger constant treats a check that has been green for
a year the same as one that flaps every lunchtime. The history to do better
already exists. The question is what to compute, at what scope, and how
much to let it act autonomously.

### Signal

Use **observed** results, not effective ones, so the statistics are
untouched by policy edits and by the damping machinery itself (no feedback
loop).

Raw history exists in `statuses` (complete, per-source, timestamped), but
it only covers device-reported checks — canopy's own filings (staleness,
backup, key expiry…) leave no history beyond the current issue stamps — and
mining per-check series out of status JSONB is a scan-heavy job. Since
every producer already funnels through one filing path that sees every
transition (`stamp_check_state`), the cheap and uniform option is to
maintain **rolling aggregates at filing time**, updated in place. We just
deleted the `events` table for being an unbounded append-only log; the
replacement must be fixed-size per key, not a new log. Per
(server/group/global target, source, check):

- **flip stats**: a small ring of the last K degraded↔healthy transition
  timestamps (K fixed, ~16–32). Everything below derives from it: flip
  rate over 24h/7d, typical degraded-run length, typical healthy-gap
  length between runs.
- **hour-of-week duty cycle**: 168 buckets of EWMA "observed degraded",
  updated on each filing. Fixed size, captures the load-dependent shape
  (red business hours, green nights) directly. Timezone: bucket in UTC —
  the pattern is just as periodic in UTC and saves per-server timezone
  bookkeeping; DST smears one hour twice a year, which the EWMA absorbs.

Group- and fleet-scope views aggregate the server rows for the same
(source, check) at read time — fleet answers "is this check noisy
everywhere or only here", mirroring the fleet → group → server layering
policy already has.

### Uses, in increasing order of ambition

1. **Adaptive linger.** When the last failure leaves, size the linger from
   the leaving check's own history: `clamp(default, k × p90(healthy-gap
   within recent degraded episodes), cap)`. The day-red/night-green check
   earns a linger that bridges its usual green gaps; the green-for-a-year
   check keeps the minimum — its recovery is believable immediately. The
   cap matters (an unbounded linger delays legitimate resolves by hours);
   a scoped-policy-style override can raise it per target where an
   operator wants the nightly green bridged entirely.

2. **Adaptive open grace.** The mirror image. A check red for the first
   time in 90 days fleet-wide is high-information — shrink the wait; a
   check that flaps weekly on this server — stretch it. Bounded both ways;
   `escalates` remains absolute and bypasses everything.

3. **Flapping state.** When the flip rate crosses a threshold
   (Nagios-style flap detection), mark the (target, source, check) as
   flapping: notify **once** ("`pg/connections` on X is flapping: 14
   state changes in 6h"), suppress the per-flip churn while it lasts, and
   notify once when it stabilises in either direction. This is the honest
   answer for the pathological case, and it is also the safety valve that
   keeps adaptive damping from teaching itself to never alert: heavy
   flapping gets *more* visible, not silently absorbed.

4. **Pattern surfacing — suggest, don't act.** Classify profiles from the
   duty cycle (e.g. degradation concentrated in business hours =
   load-dependent) and turn the big interventions into *suggestions* the
   operator confirms: a scoped warning-ceiling for that (server, source,
   check), a longer scoped linger, or a silence. Policy is the operator's
   word in this model; statistics should inform it, not overwrite it. The
   UI hook is natural — the check attention page and the incident
   timeline show the profile and the one-click suggestion.

### Design tensions to respect

- **Explainability.** Every damping decision must leave a visible trace:
  "resolve held 45 min: green gaps during this check's episodes are
  typically ~30 min", on the incident timeline and over MCP. An operator
  who can't see why nothing alerted stops trusting the pipeline.
- **Normalizing deviance.** A check that is red every business day *is* a
  problem (capacity), and damping must not bury it. Mitigations: the
  flapping-state notification, hard caps on adaptive windows,
  suggestions-over-automation for anything larger, and published-incident
  reporting continuing to count the (now well-formed) spans.
- **Cold start.** No history → static defaults. Aggregates warm up
  within days.
- **Cheapness.** Everything stays minute-cadence and DB-only: in-place
  aggregate updates on the filing path, no history scans in the hot loop.

## Off-the-shelf options considered

Surveyed (July 2026) for something to host alongside canopy — or embed —
instead of building the analysis/damping ourselves. Two distinct layers to
buy: the *notification pipeline* and the *analysis*.

### Pipeline services (forward alerts to them, they decide what notifies)

- **Prometheus Alertmanager** (Go, tiny, battle-tested): grouping,
  inhibition, silences, throttling. Its `group_interval` batching absorbs
  some flapping, but it has explicitly declined flap detection for years
  (prometheus/alertmanager#204), has no per-alert adaptive behaviour, and
  no history-based anything.
- **Alerta** (Python): the closest conceptual match — ships an
  `is_flapping(window, count)` plugin utility and a transient-alert plugin
  that grades flapping alerts down so notifiers skip them. But it is a
  whole parallel alert store + UI + API.
- **Keep** (Python/TypeScript, multi-service deployment): dedup,
  correlation, enrichment platform; the AI correlation sits behind paid
  tiers, and flap damping specifically isn't a feature.
- **Grafana OnCall OSS**: archived March 2026, read-only — off the table.

All of these sit between "alert fired" and "notification sent". Plugging
one in means forwarding incident opens/closes as alerts and letting it
decide what ships — which outsources notification decisioning but does
nothing about the *record*: incidents would still fragment in our database
on every blip, `published`/escalation state would still reset per row, and
the MCP/UI/timeline surfaces would still show N incidents for one span of
trouble. The noise problem lives in the incident model, not just in Slack
delivery, so the fix has to live there too (which the linger does). Running
a second alert brain alongside canopy's own — with its own store, silences,
and UI — costs more operational surface than the ~200 lines of damping
logic it would replace, and the bespoke Slack-workflow contract would need
adapting either way.

### Analysis libraries (embed, keep the pipeline ours)

- **augurs** (Rust, MIT/Apache, under the Grafana org): seasonality
  detection, MSTL decomposition, changepoint detection (Bayesian online
  changepoint via the `changepoint` crate), outlier detection (MAD/DBSCAN),
  forecasting (ETS/Prophet). In-process, no service to run.
  Maintenance check (July 2026): alive but quiet — v0.10.2 released
  February 2026 (266 releases to date), commits through July 2026 though
  mostly bot dependency bumps, one primary human maintainer (a Grafana
  engineer; the README says explicitly it is *not* an official Grafana
  project and still pre-1.0), ~575 stars, a dozen open issues with slow
  responses. Fine as a leaf dependency, not as architecture.
- **changepoint** (Rust, MIT, promised-ai): BOCPD directly, if only that
  is wanted; small and stable, last release October 2025.

Vendor risk is low either way: the pieces damping actually needs —
periodogram seasonality, BOCPD, MAD — are small, textbook algorithms on
tiny inputs (per-check aggregates, not raw series), so if upstream stalls
the crates can be pinned or the ~few hundred relevant lines vendored. The
heavyweight parts of augurs (Prophet, DTW, clustering) would go unused.

This is the sweet spot for stages 3–4: canopy keeps every decision and all
explainability, and buys the statistics — "is this check's degradation
periodic?" (seasonality over the duty-cycle aggregate) and "did its
behaviour just change?" (changepoint over the flip rate, a natural
alert-now / suppress-now signal). The flap-detection algorithm itself
(Nagios/Icinga's weighted state-change ratio) is ~50 lines and not worth a
dependency.

**Conclusion**: nothing plugs in at the incident layer without ceding the
incident model; adopt augurs as an embedded dependency when stage 3 lands,
and keep the pipeline ours.

## Staging

1. **Static linger.** ✅ Implemented. Small, symmetric with the existing
   grace, immediate relief, and fixes the fast-flapper silence hole. Spec
   change to INC, `closing_at` on incidents, drainer gate, monitor-pod
   sweep, group knob (`slack_close_delay`, default 5 minutes) + UI field.
   Only report-driven recoveries linger: operator suppression (resolve,
   snooze, silence, monitoring-off) closes immediately — an explicit
   action isn't a flap.
2. **Transition aggregates.** Recorded at filing time, invisible to
   behaviour; surfaced read-only as a stability indicator on the check
   attention page and over MCP. Independently useful for triage even if
   nothing below ever ships.
3. **Adaptive linger, then adaptive open grace.** Derived from the
   aggregates, bounded, each decision traced in the UI.
4. **Flapping state and pattern suggestions.** The operator-in-the-loop
   layer.

1 and 2 are independent of each other and both prerequisites for 3; each
stage is useful on its own if the later ones never happen.
