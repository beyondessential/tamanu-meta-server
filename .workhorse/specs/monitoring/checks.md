---
id: CHK
---

# Check state

Canopy's monitoring is organised around checks: named conditions, each with a current result, reported by sources or determined by Canopy itself.
This spec covers the check-state model — targets, sources, results, policy, and the operator controls over them.
How device reports arrive is the status contract (see [STA](../public-server/statuses.md)); how degraded checks aggregate into incidents is the incident spec (see [INC](incidents.md)).

## Targets

Every check is scoped to exactly one target: a server, a server group, or Canopy as a whole.

Server checks come from sources reporting on that server, and from Canopy's own per-server determinations such as source staleness.
Group checks are conditions Canopy determines about a group's control plane, such as backup maintenance health (see [BKJ](../jobs/backup.md)).
Canopy-wide checks are Canopy monitoring its own operation (see [SELF](../private-server/self-alerts.md)).

## Sources

A source is a named reporter of checks, identified by a short string.
Multiple sources may report on the same server, each concerned with part of the system, and each source's reports are independent: a report from one source says nothing about another source's checks.

One source name is reserved for Canopy itself: `canopy`, for conditions Canopy determines on its own (reachability, backup health, key expiry, self-monitoring).
Reports arriving over the device API cannot use the reserved name.

### Source policy

Each source other than the reserved name carries two operator-set modes, global to the source and edited alongside the check catalog.

Its **reachability mode** governs how the source's silence bears on its servers' reachability (see "Reachability"):

- `on` — a stale source warns, and all of a server's sources stale is unreachable;
- `quiet` — a stale source raises no warning, but still counts toward unreachable;
- `off` — the source is excluded from reachability entirely.

Its **ingest mode** governs whether the device API accepts the source's reports (see [STA](../public-server/statuses.md)):

- `allow` — reports are ingested normally;
- `ignore` — reports are accepted, but the source's data is discarded before ingestion;
- `deny` — the device API rejects the push.

New sources default to `on` and `allow`. A source that isn't ingested (`ignore` or `deny`) is excluded from reachability regardless of its reachability mode — there is no fresh data to judge it by.

## Results

A check's result is one of, in decreasing order of urgency:

- **failed** — the condition is failing.
- **warning** — the condition is degraded but not failing.
- **broken** — the check itself could not run; the condition is unconfirmed either way.
- **passed** — the condition holds.
- **skipped** — the check deliberately did not run.

Every check has two results: the **observed** result, what the source reported, and the **effective** result, what policy makes of it.
The observed result is always recorded as reported; everything Canopy acts on — issues, incidents, health rollups — follows the effective result.

## Policy

Policy is a transformation of results: for each check it maps the observed result to the effective one.
There is one vocabulary on both sides — policy speaks in results, and what a source is told about its checks is the policy itself, not a projection of it.

Fleet-wide policy lives in a catalog keyed by (source, check).
An entry carries:

- a **ceiling** — the maximum effective result, on the urgency ordering: a ceiling of `failed` changes nothing, `warning` grades failures as warnings, `passed` means recorded but never alerting, and `skipped` additionally tells the source not to bother running the check.
- optional **rules** — conditional transforms evaluated against the check's own detail, the report's server-wide detail, and the server's effective tags; a rule can move a result in any direction, including upward: a warning graded as a failure, or a pass with a particular detail graded as a warning.
- an **escalates** flag — an effective failure of this check notifies immediately, bypassing incident grace (see [INC](incidents.md)).

A check is registered in the catalog with a ceiling of `warning` the first time it is reported, and is pending operator review; operators confirm or adjust its policy from there, and checks still awaiting review are surfaced for it.
Canopy's own checks register already reviewed, with the policy their condition warrants instead of the default.

### Scoped policy

Beyond the fleet catalog, a transform can be scoped to a target: per server, per group, or Canopy-wide.
Transforms apply in order — fleet catalog, then group, then server — each acting on the previous effective result, so the most specific scope has the last word.

The operator interface presents one scoped policy: the **silence**, a scoped ceiling of `skipped`, recording who silenced and when.
A silenced check keeps recording its observed results; its effective result is skipped, so it raises nothing and counts nowhere.
The model admits arbitrary scoped transforms; surfaces beyond the silence are deliberately not offered yet.

## Documentation

Each catalogued (source, check) can carry operator-authored documentation: a single markdown document.
By convention it covers three things — a general description of what the check observes, what each result means (what makes it fail as opposed to warn), and hints for solving a failure — and the editor seeds new documentation with a template of those sections; Canopy attaches no meaning to the document's structure.
Operators author and edit the documentation in the operator UI; it is presented alongside the check wherever its state is presented, and is available over the MCP interface (see [MCP](../private-server/mcp.md)) so agents work from curated knowledge about a check rather than deriving it.
Canopy's own checks ship with their documentation.

## State

For each (target, source, check) Canopy keeps exactly one state: the observed and effective results, the detail the source attached to the check's most recent report, when the check was first and most recently reported, and — while it is degraded — when the current degradation began.
All reported checks are kept, including passing ones, so that "every server reporting this check" is answerable without scanning history.

A state whose effective result is warning or failed is an **issue**, eligible to contribute to incidents.
"Degraded since" is the start of the current unbroken run of degradation; a recovery ends the run, and a later degradation starts a fresh one.

### Stability

Alongside each state, Canopy keeps a bounded stability record, updated as reports arrive.
It is derived from **observed** results — untouched by policy, so operator grading and any noise damping built on the record never feed back into it.
An observation is degraded when the observed result is warning, failed, or broken, and healthy when it is passed; skipped observations carry no signal and are not recorded.

The record holds:

- how many observations the state has received, and how many were degraded;
- the most recent transitions between healthy and degraded, each with when it happened — a bounded ring, so a long-stable check remembers its distant history while a flapping one remembers only its recent churn;
- an hour-of-week profile of how often the check is observed degraded, weighted towards recent weeks, so a load-dependent check (degraded during working hours, healthy overnight) is distinguishable from one degraded around the clock.

From the ring, Canopy derives and presents how often the state has flapped recently and how long its degraded runs and healthy gaps typically last.
The record is presented alongside the check on its check detail page and is available over the MCP interface (see [MCP](../private-server/mcp.md)).
Nothing beyond the record is kept: it is a fixed-size summary per state, not a history.

An effective broken result neither confirms nor clears the check's previous definite result: while broken, the state retains the contribution and degraded-since of its last definite effective result, and the brokenness itself additionally counts as a warning.
A policy rule can grade brokenness differently — up to a failure where not being able to check is itself the failure, or down to a pass where a flaky check runner should not raise noise.
A definite effective result (passed, warning, or failed) ends the broken condition and replaces the retained contribution.

## Reporting semantics

A source's report for a server carries that source's complete current set of checks.
Canopy trusts the reporter: a check the source previously reported but omits from its current report has recovered, and its state records that.
Omission by one source never affects another source's checks.

## Reachability

Canopy tracks, for each server, the sources expected to report — those that have reported, are not in reachability mode `off`, and whose checks are not all decommissioned — and when each last reported.
It keeps one `reachability` check per server, under the `canopy` source, reflecting how many expected sources are currently reporting within the server's down threshold:

- **passed** when every expected source is fresh;
- **warning** when a source in mode `on` is stale but not every expected source is; the stale sources are named in the check's detail, so an operator sees which reporter went quiet. A `quiet` source going stale never raises this warning;
- **failed** when every expected source is stale — nothing is reaching Canopy — and the server is presented as unreachable. `quiet` and `on` sources count alike here.

A stale source degrades the server rather than silently dropping its checks, so a reporter going quiet is never mistaken for health.
There is no per-source staleness check; the one reachability check carries the full picture.

## Liveness and decommissioning

Reachability is a per-server signal about reporters that have gone quiet; check liveness is a fleet-wide signal about a (source, check) that has gone away everywhere.
For each catalogued (source, check) Canopy tracks when it was most recently reported on any server.

A (source, check) not reported anywhere for seven days is surfaced to operators as a candidate for decommissioning.
A (source, check) not reported anywhere for thirty days raises a Canopy-wide warning (see [SELF](../private-server/self-alerts.md)).

Decommissioning is an operator action, never automatic: the candidate list and the Canopy-wide warning surface what has gone away, and an operator decides.
A decommissioned (source, check) is retired fleet-wide: its state on every server is resolved, recording decommissioning as the reason, and it then contributes to nothing — not health, not incidents, not reachability.
A source all of whose checks are decommissioned is no longer an expected source, so it drops out of the reachability signal.

If a decommissioned check is reported again it is treated as newly registered — pending operator review, at the warning ceiling — so a resurrected check never silently resumes a retired policy.

## Health rollup

A server's health is derived from the checks currently contributing across all its sources: any effective failure makes it unhealthy; otherwise any effective warning or brokenness makes it degraded; otherwise it is healthy.
Passed and skipped checks, and states that are resolved, snoozed, or decommissioned, do not count against a server.

## Operator controls

**Silences** are the scoped policy described above.

**Snoozes** suppress one state until a chosen time, after which it contributes again if still degraded.

**Resolution** is an operator marking one state as dealt with, recording who and why.
A resolved state that degrades again reopens: the resolution is cleared and the state contributes anew.

**Notes** attach free-form operator commentary to a state.

## Monitoring gate

Server-targeted checks on a server that is not monitored are recorded and presented for visibility but do not contribute to incidents.
Group and Canopy-wide checks are not subject to any server's monitoring gate.
