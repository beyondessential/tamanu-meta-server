# Fleet figures: reported detail across the whole fleet

Spec: [FIG](../specs/private-server/server-figures.md) ("Fleet spread").

The per-server figures (Tamanu version, PostgreSQL, platform, Node.js,
timezone, bestool) answer "what is this server running". This adds the
transpose: what is the *fleet* running — the spread of each figure across
every live server, plus the spread of any field a source reports, plus a
cross-tab of two fields against each other.

## Why a denormalised table

The figures live in `statuses.extra`. That table is range-partitioned by
week, ~100 partitions in prod, ~864k rows for a single server, and a
predicate on `server_id` alone cannot be partition-pruned — the trap PR #404
documents at length. Resolving one server's figures already costs a bounded
`DISTINCT ON` over the last 30 days; doing that 120 times per page load is
the same class of read that measured 217s.

So the fleet view does not read status history at all. Ingest maintains
`server_reported_detail`: one row per `(server_id, source)` holding that
source's latest payload. The fleet page scans ~400 small rows.

This also retires the 30-day lookback on the *live* per-server figures. The
bound existed only because the read was expensive; a denormalised row is
free to keep, so a figure persists for as long as the server does. The
point-in-time snapshot still reconstructs from history and keeps its bound —
that read is inherently historical.

## Schema

`server_reported_detail`, PK `(server_id, source)`:

- `server_id` — FK to `servers`, `ON DELETE CASCADE`. A deleted server takes
  its reported detail with it.
- `source` — the reporting source, as on `statuses`.
- `extra` — jsonb, the source's whole server-wide detail as last pushed.
- `version` — the application version that push reported, nullable. Carried
  so the fleet view can spread Tamanu versions from the same read.
- `reported_at` — when that push landed; the ordering key for
  newest-source-wins.

Migration backfills from each `(server, source)` pair's latest push inside
30 days. Backfilling from all history would be exactly the unbounded scan
this table exists to avoid; a server quiet longer than that gets its row on
its next push.

## Ingest

The status ingest path upserts the row for the pushing source, replacing
what that source previously reported — a push is the source's whole current
truth, the same rule that already governs its checks. Other sources' rows
are untouched.

Canopy's own generated statuses (reachability sweep, pings) carry no
reported detail and do not write. Pushes from a source in `ignore` ingest
mode are recorded nowhere, so they do not write either.

## Reading

`ServerReportedDetail::for_servers` loads the rows and folds each server's
sources through the existing `MergedDetail::from_statuses` resolution
(newest-wins per key), so a figure reads identically on the fleet page and
the detail page. `MergedDetail` grows a constructor taking `(reported_at,
extra)` pairs; the status-slice constructor stays for the snapshot path.

Live per-server reads move onto it:

- `get_detail`'s figures — replaces the `latest_per_source_at` call added in
  PR #405.
- `latest_munin_for_server` — deleted. `munin` is just another key, so it
  comes off the merged detail, removing the non-indexable `jsonb_exists`
  scan and restoring the indefinite grace SVC had to give up.

The snapshot path is unchanged: point-in-time needs history.

## Endpoint

`POST /api/statuses/fleet_detail`, tailnet-user auth, no arguments. Returns
one row per live server (archived servers and canopy's own row excluded):
id, name, rank, kind, group id/name, plus the resolved figures and the full
merged payload.

The client does all grouping. That keeps the arbitrary-key lookup and the
cross-tab instant and free of a server-side whitelist of permitted keys.
~120 rows of small JSON.

## Page

A tab on the fleet section, alongside Groups / Ungrouped / Archived.

- A distribution card per figure: value → count, ordered by count, with an
  explicit group for servers reporting nothing. Expanding a value lists its
  servers as links. Cards show the largest groups and collapse the tail, so
  a near-unique field degrades to "N other values" instead of 120 rows.
- A lookup box for any field, autocompleting from the union of keys the
  fleet actually reports, rendering the same card shape.
- A cross-tab: pick two fields, get a matrix of counts with an unreported
  row and column, cells expanding to the servers in that intersection.

`/servers` is relabelled "Fleet" in the nav and page heading. The routes
stay at `/servers/*` — relabelling is cosmetic, but renaming URLs would
break every bookmark and every link already posted to Slack.
