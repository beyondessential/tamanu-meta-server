# Plan: consolidated multi-source checks view (spec B)

Implements the [CHK](../../.workhorse/specs/monitoring/checks.md) "Presentation"
requirement. Depends on spec A being landed (shares the one grading/classifier).

Today the server page and the status-snapshot panel each render a **single**
source's raw `health[]` array, and classify it by two different rulebooks: the
snapshot uses `Status::health_state_ignoring` on **raw** results, while the
headline chip uses `health_from_check_state` on **policy-graded** `effective_result`.
The snapshot showing one source's raw data by different rules is the defect to
remove. The target: one checks presentation — all sources together, graded and
classified by the same pipeline as incidents — served both live and as of any
past time.

## One classification core

Extract a single classifier used by every health rollup:

- `HealthState::from_results(iter, silenced)` (in `commons-types`) — worst-of over
  effective results, silenced skipped, matching the rollup rules in CHK.
- Factor the ingestion-time `CheckPolicy` grading (`crates/database/src/check_policies.rs`,
  `crates/public-server/src/statuses.rs`) into a fn that maps a raw `health[]`
  entry → graded result, so the snapshot path can grade historical raw data the
  same way ingestion did.

Delete `Status::health_state` / `health_state_ignoring`
(`crates/database/src/statuses.rs:668-714`) and its raw/legacy branch. Route
`health_from_check_state` through the shared classifier so there is exactly one
set of rules.

## Consolidated check state

New `database` read producing a server's checks across all sources, one shape for
both cases:

- **Latest** — from the `issues` table (all sources; already carries
  `effective_result` and `detail`; the incident authority; bulk-efficient for the
  fleet dot/cards). This is what the server page's checks table and headline
  consume.
- **As of T** — reconstructed from `statuses` history: for each source, its most
  recent status at-or-before T; union the `health[]`; grade each entry through the
  shared `CheckPolicy` fn (current policy applied to old raw data — a faithful
  re-grade); classify. Snapshot use is dozens/day vs the live view's constant use,
  so on-demand re-grade is the right trade — no graded history is stored.

Shape returned to the UI: per `(source, check)` the effective result, the check's
detail, silenced/decommissioned flags; plus the rolled-up `HealthState`. Detail is
grouped by source (`{ [source]: { …fields } }`) so the extras panel is
consolidated rather than one source's blob.

## Endpoints

- `statuses/snapshot` (`crates/private-server/src/fns/statuses.rs`): returns the
  consolidated multi-source shape at `at` (or latest), replacing the single-source
  `health_state` path.
- Server detail (`servers/get_detail`): the checks table stops consuming
  `ServerLastStatusData.health[]`; it consumes the consolidated latest state. The
  raw single-source `health[]` and `healthy` fields leave the wire shape (kept
  internal only where still needed for ingestion).

`just gen-openapi` + regen `api-types.ts`.

## Frontend

`private-web`:

- `routes/ServerDetail.tsx` `ChecksTable`/`ChecksTableBody`/`parseChecks`: render
  the consolidated multi-source state; source becomes a column/grouping; drop the
  single-source `overallHealthy`-softening special-case (grading now happens
  server-side). Remove the `checkResultOf`/`Status::health_state`-mirroring
  client logic.
- `components/StatusSnapshot.tsx`: same consolidated shape, `at`-parameterised.
- Extras/detail panel (`components/CheckExtras.tsx`): render the per-source
  `{ [source]: { … } }` object.
- Remove any remaining path that hands a single source's raw `health[]` to a
  component.

## Tests

`TestDb` / private-server `it`:
- consolidated latest merges all sources and matches `health_from_check_state`.
- snapshot at T reconstructs each source's most-recent-≤-T report and grades it
  through current policy; equals the live view when T is now.
- headline chip and checks table agree (same classifier).

Playwright: a server with two reporting sources shows both sources' checks in one
table; the snapshot panel shows the same shape for a past time; extras render
grouped by source.

## VCS

Lands on top of spec A's implementation (jj rebase). The spec-B doc edit (CHK
"Presentation") and this plan already sit above `plan: … (spec A)`; A's
implementation commits are inserted below them.
