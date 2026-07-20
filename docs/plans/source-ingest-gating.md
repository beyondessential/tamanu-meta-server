# Plan: source ingest gating (PR2)

Implements the [CHK](../../.workhorse/specs/monitoring/checks.md) "Source policy"
ingest mode and the [STA](../../.workhorse/specs/public-server/statuses.md) push
gating. Stacks on the source-reachability PR (#383); rebase onto main once that
merges.

## Data model

`just migration add_source_ingest_mode`: add `ingest text not null default 'allow'`
to `source_policies`. `IngestMode` enum in `commons-types::source`
(`allow`/`ignore`/`deny`) with the same string round-trip as `ReachabilityMode`.
`SourcePolicy` gains `ingest`, an `ingest_modes()` map helper, and `set_ingest`.
No seed — `bestool-alertd = deny` is set by an operator (it was briefly a real
source, not synthetic).

## Ingest enforcement

Public-server status push handler (`crates/public-server/src/statuses.rs`): look
up the pushing source's ingest mode before recording.

- `allow` — unchanged.
- `ignore` — accept the request (200) but record nothing: no status row, no check
  filing.
- `deny` — reject the push (403).

Per source, so other sources on the same server are unaffected. The reserved
`canopy`/`manual` sources are always `allow` (never gated).

## Reachability interaction

A source that isn't ingested has no fresh data, so it's excluded from
reachability regardless of its reachability mode. The reachability sweep
(`Status::sweep_staleness`) drops from the expected set any source whose
reachability is `off` **or** whose ingest is `ignore`/`deny`.

## Endpoints

`crates/private-server/src/fns/healthchecks.rs`: `SourceData` gains `ingest`;
`set_source_ingest { source, ingest }` (admin), rejecting reserved names.
`SourcePolicy::list_sources` returns the ingest mode too. `just gen-openapi` +
regen `api-types.ts`.

## Frontend

The Sources table gains a second 3-state toggle (ingest: allow/ignore/deny). When
ingest is `ignore` or `deny`, the reachability toggle is disabled and shown as
`off` (no data to judge by).

## Tests

`TestDb`:
- reachability excludes a source whose ingest is `ignore`/`deny` (no warning even
  when stale; an all-non-allow server falls to the status-row backstop).

Public-server `it`:
- a `deny` source's push is rejected;
- an `ignore` source's push returns success but records no status/checks;
- an `allow` source (and the reserved sources) ingest normally.

Playwright: toggling ingest persists; setting `ignore`/`deny` disables the
reachability toggle.

## Follow-up

Then spec B (consolidated multi-source checks view), rebased onto main.
