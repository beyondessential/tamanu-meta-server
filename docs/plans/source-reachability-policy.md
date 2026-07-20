# Plan: source reachability policy (PR1)

Implements the [CHK](../../.workhorse/specs/monitoring/checks.md) "Reachability
mode" + rewritten "Reachability" sections. First of three: this PR adds the
source table + reachability toggle; a follow-up adds ingest gating
(allow/ignore/deny); then the consolidated checks view (spec B).

## Why

Since the source+check rework, the legacy Tamanu heartbeat is its own source
(`tamanu`), globally allowed. Half the fleet still pushes it, half migrated off
it — so `tamanu` is alive fleet-wide (can't be globally decommissioned) yet
legitimately dead per-server on the migrated half, where the new per-source
staleness would raise a perpetual reachability warning. The fix: a global
per-source **reachability mode**, and fold per-source staleness into the single
`reachability` check.

## Data model

`just migration add_source_policies`:

- `source_policies (source text primary key, reachability text not null default 'on', created_at, updated_at)`.
- Seed `('tamanu', 'quiet')` in the same migration — tamanu is effectively
  synthetic (canopy fabricates it from legacy pushes), so a stale tamanu should
  never warn, but a legacy-only server going silent must still read unreachable.

`ReachabilityMode` enum in `commons-types` (`on`/`quiet`/`off`): Serialize +
ToSchema + FromStr/Display (text round-trip for the column). `SourcePolicy` model
in `database` with `list`, `set_reachability`, and a `modes()` helper returning
`HashMap<String, ReachabilityMode>` (defaulting absent sources to `on`).

## Reachability sweep rework

`crates/database/src/statuses.rs`:

- `sweep_staleness` folds in the old per-source arm. For each monitored server,
  from `Issue::source_freshness` (already excludes decommissioned) compute the
  expected sources (drop mode `off`), split fresh vs stale against the server's
  down threshold, and file the one `reachability` check:
  - all fresh → passed;
  - a mode-`on` source stale, not all stale → warning, with the stale source
    names + last-reported times in `detail`;
  - all expected stale → failed (unreachable).
- Delete `sweep_source_staleness`'s `stale/<source>` filing, `STALE_REF_PREFIX`,
  and (dead once `stale/*` is gone) the decommission branch in
  `CheckPolicy::decommission` that resolved lingering `stale/<source>` issues.
- **Transitional cleanup**: the sweep resolves any existing open `canopy`
  `stale/*` issues on sight (they'd otherwise linger forever once we stop filing
  them). Remove once deployed everywhere.

## Endpoints

`crates/private-server/src/fns/healthchecks.rs` (the source list lives on the
healthcheck settings page):

- `sources` — list `{source, reachability, last_seen}` for every non-reserved
  source in the catalog/freshness, ordered by source.
- `set_source_reachability { source, reachability }` (admin).

`just gen-openapi` + regen `api-types.ts`.

## Frontend

`private-web` healthcheck settings: a **Sources** table above the checks catalog,
one row per source with a 3-state reachability control (on / quiet / off),
admin-gated. `canopy`/`manual` never appear.

## Tests

`TestDb`:
- reachability passed / warning (on-source stale) / failed (all stale);
- a `quiet` source stale on a server with another fresh source → passed (no
  warning); a `quiet`-only server all stale → failed;
- an `off` source is excluded entirely;
- the transitional cleanup resolves an existing `stale/<source>` issue.

Playwright: the sources table renders, toggling a source's reachability persists.

## VCS

Own PR off main. The ingest-gating PR and spec B stack after it.
