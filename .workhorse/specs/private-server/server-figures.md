---
id: FIG
---

# Reported server figures

A server's detail view and its point-in-time status snapshot each present a row of figures describing the software the server runs.
The figures are derived from the server-wide detail its sources report (see [STA](../public-server/statuses.md)), not from anything an operator enters.

## Sourcing

Several sources report on one server, each pushing its own server-wide detail, and they do not all report the same figures.
Each figure is taken from the most recent source to report that figure, rather than from whichever source pushed most recently.
So a figure holds its last reported value when the newest push comes from a source that does not carry it, and two figures presented together may come from different sources reporting at different times.

A figure no source has reported in the last thirty days is not presented.
Reads over status history are bounded, because a server accumulates enough history that an unbounded search for the last report of a figure is not affordable; the cost is that a server quiet for longer than that reads as having reported nothing.

A figure that has never been reported is omitted from the row rather than presented empty.

## Figures

The application version is presented with how far behind the latest published version it is, and with the minimum embedded browser version that release requires.

The platform names the operating system family the server runs, derived from the reported database engine.

The timezone is the server's own configured timezone, presented so an operator can read the server's local time.

The database engine version and the runtime version are presented as reported.
When no source reports the runtime version, it falls back to the runtime named by the reporting device's connection metadata.

The bestool version is the version of bestool, the first-party agent that reports on the server, as it reports it in its server-wide detail.
A server reported on only by sources other than bestool presents no bestool version.

## Point in time

The status snapshot presents the same figures as of the moment being viewed.
Each figure is taken from the most recent source to report it at or before that moment, within the same thirty-day bound measured back from that moment.
