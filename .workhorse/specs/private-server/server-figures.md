---
id: FIG
---

# Reported server figures

A server's detail view and its point-in-time status snapshot each present a row of figures describing the software the server runs, and a fleet view presents how those figures are spread across every server.
The figures are derived from the server-wide detail its sources report (see [STA](../public-server/statuses.md)), not from anything an operator enters.

## Sourcing

Canopy keeps each source's server-wide detail as that source's current report on the server: an ingested push replaces what the source previously reported, and reports from other sources on the same server are untouched.
Canopy's own generated statuses carry no reported detail and leave a server's reports unchanged.
A source's current report is kept for as long as the server exists, so a figure remains available however long the server has been quiet, and is discarded with the server.

The application version is the exception to a report replacing what came before: a report that carries no version keeps the version that source last reported.
An agent omits the version when it cannot read it — the application is down, or mid-upgrade — which says nothing about what the server is installed to run.

Several sources report on one server, and they do not all report the same figures.
Each figure is taken from the most recent source to report that figure, rather than from whichever source pushed most recently.
So a figure holds its last reported value when the newest push comes from a source that does not carry it, and two figures presented together may come from different sources reporting at different times.

A figure that has never been reported is omitted rather than presented empty.

## Figures

The application version is presented with how far behind the latest published version it is, and with the minimum embedded browser version that release requires.

The platform is the operating system the server runs, as the server reports it, qualified by the operating system version where the server reports one.
A server that reports no operating system falls back to the family the reported database engine gives away, which distinguishes Windows from anything else but nothing finer.

The timezone is the server's own configured timezone, presented so an operator can read the server's local time.

The database engine version and the runtime version are presented as reported.
When no source reports the runtime version, it falls back to the runtime named by the reporting device's connection metadata.

The bestool version is the version of bestool, the first-party agent that reports on the server, as it reports it in its server-wide detail.
A server reported on only by sources other than bestool presents no bestool version.

## Fleet spread

The fleet view presents how each figure is spread across the fleet: for every distinct value, how many servers currently report it, ordered by how many, and which servers those are.
Servers reporting no value for a figure are counted together as a group of their own, so the size of the unreported population is visible rather than hidden by omission.
The view covers every live server; archived servers and canopy itself are not part of the fleet.

Beyond the figures, an operator can name any field a source reports and see its spread the same way.
The fields the fleet currently reports are offered as suggestions, so an operator can find a field without knowing its name in advance.
A field whose values are near-unique across the fleet presents its largest groups with the remainder collapsed, rather than a line per server.

A field a source reports on one of its healthchecks rather than server-wide is named through the check that reports it, as `check.field`, and spreads across the fleet the same way.
A check's own graded result is available as one of those fields, so the fleet spread of a check's outcome reads like any other field.
What presents here is what the server's own check list presents: a silenced check reads as skipped, and a decommissioned check doesn't present at all.
Checks are named by check alone, though a check's identity is the source and the check together: two sources reporting the same check name present as one, the more recently reported field winning, as elsewhere.

An operator can also cross two fields, presenting a table of one against the other: for each combination of values, how many servers report both, with the servers behind each combination available.
Servers reporting no value for either field occupy their own row and column, so a combination is never silently dropped.

## Active versions

The status view summarises which release branches the production fleet is actively running: how many distinct branches, which they are, and the range of exact versions across them.

A production server counts once, at the version the most recent source to report one gave — a source reporting no version does not drop the server from the summary by having pushed last.
Unlike the figures elsewhere, this summary is bounded by recency: a server that has not reported within the last week is not running anything as far as the summary is concerned, so a decommissioned server that was never archived stops inflating the count.

## Point in time

The status snapshot presents the same figures as of the moment being viewed, from each source's most recent report at or before that moment.
Reconstructing a moment reads the server's status history, which is bounded: a source silent for the thirty days before that moment contributes nothing to the snapshot.
