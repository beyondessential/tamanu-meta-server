# Incident pipeline rework

Implements the monitoring specs: [CHK](../../.workhorse/specs/monitoring/checks.md),
[INC](../../.workhorse/specs/monitoring/incidents.md),
[STA](../../.workhorse/specs/public-server/statuses.md), plus the amended
[SELF](../../.workhorse/specs/private-server/self-alerts.md) and
[MCP](../../.workhorse/specs/private-server/mcp.md).

Supersedes `events-retention.md` (deleted): the `events` table goes away
instead of being partitioned.

End state in brief: statuses gain a `source`; the events table is deleted;
issues become the current-check-state table (one row per (target, source,
check) including passing checks, observed + effective results); severities
are replaced by result-transform policy (ceiling + rules + escalates,
scoped fleet → group → server, silences = scoped skipped-ceiling);
incidents generalise to per-target (group or canopy-wide); internal
producers file through one catalog-driven function; `POST /events` is
removed; checks get operator-authored markdown documentation surfaced in
MCP.

Each phase ships/CIs independently. Within a phase, commit per step.

## Phase A — statuses.source and per-source scoping

Fixes multi-source flapping; no model reshape yet.

1. Migration: `statuses.source TEXT NOT NULL DEFAULT 'alertd'`; data
   migration `issues.source 'status' → 'alertd'` and the same on
   `server_silenced_refs` / `server_group_silenced_refs`. Scrub schema.rs
   regen against main.
2. `StatusPayload.source: Option<String>` — validated non-empty, reserved
   names (`canopy`, `manual`) rejected; absent ⇒ `alertd`; doc comment +
   openapi notes it is transitionally optional and will become mandatory.
   Stored on the status row.
3. `file_health_events` files and closes under the push's source instead of
   the fixed `"status"`: closes consult only open issues at (server,
   push-source, `health/…`). Constant `STATUS_SOURCE` goes away.
4. Tests: two sources pushing disjoint check sets don't close each other's
   issues; reserved/empty source rejected; default attribution; silence
   still honoured under the new source value. Regen public-server openapi.

## Phase B — delete the events table

Independent of the reshape; do while A is in CI.

1. Remove readers: `Event::list_for_issue`/`count_for_issue`, private
   `/api/issues/list_events`, IssueRow event-history expander, MCP
   `get_issue.recent_events`, `event_count` in `Incident::stats_for` and
   `IncidentData` (UI + MCP fields, openapi regen both servers).
2. Rework the two save paths (`NewEvent::save`, `raise_group_event`) to
   stop inserting event rows: drop hash/occurrences coalescing; issue
   upsert behaviour unchanged. `NewEvent` stays (it's the filing input),
   loses `occurred_at` plumbing where only events consumed it.
3. Migration: drop `events`. e2e seeds untouched (they never seeded
   events).

## Phase C — check-state model and policy

The reshape. Sub-phased; each lands green.

- **C1 schema + filing function.** Issues table gains: `check` (from
  `ref`, dropping the `health/` prefix — data migration), tri-scope target
  (server_id / server_group_id / neither = canopy-wide; replace the
  exactly-one CHECK; migrate nil-server self-alert rows to canopy scope),
  `observed_result`, `effective_result`, `detail` (latest per-check extras),
  passing-check rows allowed. `healthcheck_severities` →
  check catalog keyed `(source, check)`: `ceiling` (result), `rules`
  (result transforms), `escalates bool`; migrate severity values
  (Critical→failed+escalates, Error→failed, Warning→warning, Info→passed,
  Debug→skipped). One filing function: (target, source, check, observed
  result, detail) → resolve policy (fleet catalog → group → server scoped
  transforms) → upsert state → incident re-evaluation. Sticky-broken
  implemented here. Status ingest files every check in the push (passing
  included) and recovers omitted ones per source.
- **C2 incidents per-target.** `incidents.server_group_id` → nullable
  target (NULL = canopy-wide) with one-open-per-target partial indexes;
  membership/auto-close/grace/published/escalation keyed on effective
  results + `escalates`; canopy-target incidents notify the operator
  channel; self-alert direct-Slack path deleted.
- **C3 internal producers.** Reachability, tailnet key expiry, backup
  staleness/reconcile/preflight/corruption, restore verification, MCP
  token expiry, self-alerts, manual events all file through the filing
  function under `canopy`/`manual` with results instead of severities;
  their catalog entries register with the policy they warrant. Per-(server,
  source) staleness check replaces the bespoke reachability sweep;
  unreachable = all sources stale.
- **C4 read model.** Health rollup + attention page + group member health
  read check state (delete `reporting_check_with_servers`, latest-row
  JSONB scans); silences become scoped policy rows (migrate both silence
  tables; keep silence UI); UI: severity chips/filters → observed+effective
  results, healthcheck severities page → policy page (ceiling, escalates;
  rules stay JSON); MCP find/get issue/incident on results; seeds + e2e
  updated; openapi regen.
- **C5 ingest endgame.** Legacy pushes → synthetic `("tamanu", tasks:
  passed)`; `create_legacy_status` carry-forward and `allow_legacy_status`
  flag removed; `POST /events` removed (public openapi regen; bestool
  contract-test note for its repo); source-scoped push response (policy
  per pushed check; backup_now only to `alertd`).

## Phase D — check documentation

`check_documentation` markdown column on the catalog; UI editor seeded
with the template (`## Description` / `## Results` with per-result
bullets / `## Solve`); shown on attention page + issue rows; MCP
`get_check_documentation` tool; canopy's own checks ship documented.

## Cross-cutting cautions

- `just migration NAME` for every migration; `just migrate` regenerates
  schema.rs — scrub other-branch pollution.
- Public-server openapi is drift-checked too; regen after any handler
  change.
- Slack payload builders read issue severity/source/ref — update in C2/C3,
  not before.
- Daniel's tam-6867 branch collides on STA spec + status handler; ours
  lands first, no proactive notify.
- ERRORS.md for any new problem types (e.g. reserved-source rejection).
