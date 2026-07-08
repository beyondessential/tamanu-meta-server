---
id: CHK
---

# Check state

Canopy's monitoring is organised around checks: named conditions, each with a current result, reported by sources or determined by Canopy itself.
This spec covers the check-state model — targets, sources, results, severities, and the operator controls over them.
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

A check's result is one of:

- **passed** — the condition holds.
- **warning** — the condition is degraded but not failing.
- **failed** — the condition is failing.
- **broken** — the check itself could not run; the condition is unconfirmed either way.
- **skipped** — the check deliberately did not run.

## State

For each (target, source, check) Canopy keeps exactly one state: the current result, the detail the source attached to the check's most recent report, when the check was first and most recently reported, and — while it is degraded — when the current degradation began.
All reported checks are kept, including passing ones, so that "every server reporting this check" is answerable without scanning history.

A state whose result is warning or failed is an **issue**: it acquires a severity from the catalog and is eligible to contribute to incidents.
"Degraded since" is the start of the current unbroken run of degradation; a recovery ends the run, and a later degradation starts a fresh one.

A broken result neither confirms nor clears the check's previous definite result: while broken, the state retains the severity contribution and degraded-since of its last definite result, and additionally warns that the check itself is broken.
The severity of brokenness defaults to warning and is configurable per check in the catalog.
A definite result (passed, warning, or failed) ends the broken condition and replaces the retained contribution.

## Severity catalog

Check severities are configured in a catalog keyed by (source, check).
A check is registered in the catalog at warning severity the first time it is reported; operators adjust from there.
Each entry carries a base severity and optionally conditional rules, evaluated against the check's own detail, the report's server-wide detail, and the server's effective tags, so the same check can be graded differently by context.

Canopy's own checks are catalogued like any other source's, so operators control the severity of Canopy-raised conditions (reachability, staleness, backup signals) the same way as device-reported ones.

## Reporting semantics

A source's report for a server carries that source's complete current set of checks.
Canopy trusts the reporter: a check the source previously reported but omits from its current report has recovered, and its state records that.
Omission by one source never affects another source's checks.

## Source staleness

A source that has reported on a server is expected to keep reporting.
When a source's most recent report for a server is older than the server's down threshold, Canopy raises a staleness check for that (server, source) under the `canopy` source, and clears it when the source reports again.
A server all of whose sources are stale is presented as unreachable.

## Health rollup

A server's health is derived from its current check states across all sources: any failed check makes it unhealthy; otherwise any warning or broken check makes it degraded; otherwise it is healthy.
Passed and skipped checks, and silenced checks, do not count against a server.

## Operator controls

**Silences** suppress a (source, check) at server, group, or Canopy-wide scope: matching checks are still recorded and their state kept current, but they present as skipped in health rollups, raise no severity, and never contribute to incidents.
Silences record who created them and when.

**Snoozes** suppress one state until a chosen time, after which it contributes again if still degraded.

**Resolution** is an operator marking one state as dealt with, recording who and why.
A resolved state that is reported degraded again reopens: the resolution is cleared and the state contributes anew.

**Notes** attach free-form operator commentary to a state.

## Manual conditions

Operators can raise a condition directly against a server, under the `manual` source, with a chosen check name, severity, and message.
A manual condition behaves as a reported check whose reporter is the operator: it stays active until an operator resolves it or raises it again as recovered.

## Monitoring gate

Server-targeted checks on a server that is not monitored are recorded and presented for visibility but do not contribute to incidents.
Group and Canopy-wide checks are not subject to any server's monitoring gate.
