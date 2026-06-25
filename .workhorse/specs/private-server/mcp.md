---
id: MCP
---

# Fleet query interface

A read-only query interface to the Canopy fleet, exposed for AI agents and other automated clients that operators run.
It lets such a client discover servers and groups, read their status and health, learn what Tamanu versions exist and which are deployed, and inspect backup state and problems — without granting any ability to change the fleet.

## Why it exists

Operators increasingly investigate the fleet through AI agents rather than by hand.
The operator web UI answers these questions for a human, and the operator API answers them for the web UI, but neither is a surface an agent can discover and call on its own: the agent would have to be told each endpoint and its request shape out of band.
This interface gives an agent a single, self-describing entry point: it advertises the questions it can answer and the inputs each takes, so a client can enumerate the available queries and call them without prior knowledge of the fleet's internals.

## Access and identity

The interface is part of the operator-facing surface, alongside the operator API and web UI, and is never exposed on the device-facing surface.
It speaks the Model Context Protocol over HTTP at `/api/mcp`.

Every caller is identified by a tailnet user identity, the same identity the rest of the operator surface uses.
The interface is available to any human tailnet user, not only to administrators, and is not reachable by tagged automation devices.
Each query a caller makes is attributable to that identity.

## Read-only

Every query in this interface only reads.
Nothing it exposes creates, modifies, deletes, or triggers any fleet action, and no query has a side effect beyond being recorded as having happened.
Mutating the fleet is out of scope for this interface.

## Queries

The interface advertises a fixed catalogue of named queries, each with a described set of inputs and a structured result.
A client can enumerate the catalogue and read each query's description and inputs before calling it.
Results carry resolved human-readable labels (such as group and server names) alongside the identifiers they correspond to, so a result is legible on its own without a follow-up lookup.

### Discovery

**Find servers** takes an optional free-text term (matched against name, host, or identifier) and optional filters for kind, rank, and group, plus a flag to include archived servers.
It returns a bounded list of matching servers in compact form — identifier, name, host, kind, rank, owning group, when each was last seen, its last known version, and its current health.
When the result is truncated to its bound, the result says so, so the client does not mistake a partial list for the whole.

**Find groups** takes an optional free-text term and returns the matching groups, each with its live member count, its effective version, the highest rank among its members, its backup configuration state, and when it last backed up.

### Detail

**Get server** takes a server identifier and returns the full record for one server: its own fields, its latest reported status (version, overall health and per-check health, host platform, database version, reachability, and when last seen), its owning group, the count of its siblings in that group, which backup types it is capable of, and the most recent successful backup for each.

**Get group** takes a group identifier and returns the full record for one group: its own fields, its members in compact form with each member's version and health, its backup configuration and per-type schedules, its repository statistics, its recent backup and maintenance activity, and when its repository was last inspected.

### Versions

**List versions** optionally includes drafts and returns the known Tamanu versions, each with its number, release status, head release date, a changelog summary, and how many live servers currently report running it.

**Get version** takes a version and returns its detail: its changelog, its known issues, the later versions available as updates from it, and which servers and groups currently run it.

### Fleet triage

**Fleet summary** takes no input and returns a fleet-wide overview: server counts by kind and rank, the distribution of deployed versions, a rollup of server health, the number of groups, and a rollup of backup health.

**Find backup problems** optionally narrows to one group, otherwise scans the whole fleet, and returns the current backup problems with a severity for each: server-and-type pairs whose last successful backup is overdue against its schedule, types that have never reported a backup, groups whose backup repository is in an error state, recent failed backup runs, and maintenance runs that appear stuck.

### Incidents and issues

An issue is a per-server (or per-group) condition raised from a known source under a stable reference, carrying a severity, a current active state, and a history of events; an incident aggregates the issues active for a group over a span of time, from when it opened until it closes or an operator resolves it.

**Find incidents** takes a look-back window (in days, defaulting to a week) and optionally one group, and returns the incidents that were open at any point within that window — those still open, plus those that closed no earlier than the window start.
Each is returned with its group, its status (open, closed, or operator-resolved), when it opened, closed, and was resolved, who resolved it and why, whether it ever escalated, how long it was open, whether it was published, and how many issues and events it covers.
A status filter can narrow the result to only open or only resolved incidents.

The window includes incidents that flapped open and shut within their group's grace period and so never surfaced to anyone.
Each incident therefore carries a **published** flag — true when it actually notified operators, which happens only when it stayed open past its group's grace period or it escalated (a critical issue joined, which bypasses the grace) — and the result reports how many of the returned incidents were published.
The raw event count an incident accumulated is not a measure of its duration or severity: a high count can belong to a sub-minute flap.
A summary or ranking of incidents should count published incidents rather than raw rows unless raw activity is explicitly wanted.

**Get incident** takes an incident identifier and returns the incident with the issues attached to it: each issue's severity, source, reference, message, owning server, active state, and when it joined and (if applicable) left the incident.

**Find issues** returns issues across the fleet, filtered by active state, by severity, by group, by server, and by recency (issues last seen within a look-back window).

**Get issue** takes an issue identifier and returns the issue with its recent events and the incidents it is or was part of.

## Result semantics

A server's reported status reflects reports received within the recent-activity window; a server silent beyond that window reads as not recently seen rather than as a stale "up".
A server's last known version is retained without that window, so a long-offline server still reports the last version it ran.

The health classifications this interface reports — a server's healthy / warning / unhealthy / unreachable state, and a backup's overdue, never-reported, failed, or stuck classifications — are the same classifications the operator web UI presents for the same data.
A client and an operator looking at the same server or group reach the same conclusion about whether it is healthy and whether its backups are in good order.

Version adoption counts and the version distribution count live servers by the version each currently reports, so they reflect what is deployed now rather than what has ever been seen.

An incident counts as open within a look-back window if it was open at any point in that window, not only if it opened within it: a long-running incident that opened earlier and is still open, or that closed during the window, is included.
