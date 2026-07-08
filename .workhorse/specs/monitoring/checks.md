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

Two source names are reserved for Canopy itself: `canopy` for conditions Canopy determines on its own (staleness, reachability, backup health, key expiry, self-monitoring), and `manual` for conditions raised by operators.
Reports arriving over the device API cannot use the reserved names.

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

A check is registered in the catalog with a ceiling of `warning` the first time it is reported; operators adjust from there.
Canopy's own checks register with the policy their condition warrants instead of the default.

### Scoped policy

Beyond the fleet catalog, a transform can be scoped to a target: per server, per group, or Canopy-wide.
Transforms apply in order — fleet catalog, then group, then server — each acting on the previous effective result, so the most specific scope has the last word.

The operator interface presents one scoped policy: the **silence**, a scoped ceiling of `skipped`, recording who silenced and when.
A silenced check keeps recording its observed results; its effective result is skipped, so it raises nothing and counts nowhere.
The model admits arbitrary scoped transforms; surfaces beyond the silence are deliberately not offered yet.

## Documentation

Each catalogued (source, check) can carry operator-authored documentation, written in markdown in three sections: a general description of what the check observes, what each result means — what makes it fail as opposed to warn — and hints for solving a failure.
Operators author and edit the documentation in the operator UI; it is presented alongside the check wherever its state is presented, and is available over the MCP interface (see [MCP](../private-server/mcp.md)) so agents work from curated knowledge about a check rather than deriving it.
Canopy's own checks ship with their documentation.

## State

For each (target, source, check) Canopy keeps exactly one state: the observed and effective results, the detail the source attached to the check's most recent report, when the check was first and most recently reported, and — while it is degraded — when the current degradation began.
All reported checks are kept, including passing ones, so that "every server reporting this check" is answerable without scanning history.

A state whose effective result is warning or failed is an **issue**, eligible to contribute to incidents.
"Degraded since" is the start of the current unbroken run of degradation; a recovery ends the run, and a later degradation starts a fresh one.

An effective broken result neither confirms nor clears the check's previous definite result: while broken, the state retains the contribution and degraded-since of its last definite effective result, and the brokenness itself additionally counts as a warning.
A policy rule can grade brokenness differently — up to a failure where not being able to check is itself the failure, or down to a pass where a flaky check runner should not raise noise.
A definite effective result (passed, warning, or failed) ends the broken condition and replaces the retained contribution.

## Reporting semantics

A source's report for a server carries that source's complete current set of checks.
Canopy trusts the reporter: a check the source previously reported but omits from its current report has recovered, and its state records that.
Omission by one source never affects another source's checks.

## Source staleness

A source that has reported on a server is expected to keep reporting.
When a source's most recent report for a server is older than the server's down threshold, Canopy raises a staleness check for that (server, source) under the `canopy` source, and clears it when the source reports again.
A server all of whose sources are stale is presented as unreachable.

## Health rollup

A server's health is derived from its current effective results across all sources: any failure makes it unhealthy; otherwise any warning or brokenness makes it degraded; otherwise it is healthy.
Passed and skipped checks do not count against a server.

## Operator controls

**Silences** are the scoped policy described above.

**Snoozes** suppress one state until a chosen time, after which it contributes again if still degraded.

**Resolution** is an operator marking one state as dealt with, recording who and why.
A resolved state that degrades again reopens: the resolution is cleared and the state contributes anew.

**Notes** attach free-form operator commentary to a state.

## Manual conditions

Operators can raise a condition directly against a server, under the `manual` source, with a chosen check name, result, and message, and optionally marked as escalating.
A manual condition behaves as a reported check whose reporter is the operator: it stays active until an operator resolves it or raises it again as recovered.

## Monitoring gate

Server-targeted checks on a server that is not monitored are recorded and presented for visibility but do not contribute to incidents.
Group and Canopy-wide checks are not subject to any server's monitoring gate.
