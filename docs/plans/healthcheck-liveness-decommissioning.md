# Plan: healthcheck liveness & decommissioning (spec A)

Implements the [CHK](../../.workhorse/specs/monitoring/checks.md) "Liveness and
decommissioning" and "Health rollup" requirements and the [SELF](../../.workhorse/specs/private-server/self-alerts.md)
30-day condition.

Motivating case: `bestool-alertd` was a reporting source that went silent in May
2026 (renamed to `alertd`). Its orphaned `issues` rows keep 14 servers showing a
phantom `stale/bestool-alertd` warning and 27 servers falsely "Unhealthy" via
frozen `effective_result = failed` rows that no server-page UI even renders. Two
readers trust the `issues` table as live state without garbage-collecting dead
checks. This plan adds fleet-wide `(source, check)` liveness, an operator
decommission action that soft-resolves a dead check everywhere, and fixes the
readers.

## Data model

`just migration add_check_liveness` — add to `check_policies`:

- `last_seen timestamptz NULL` — global (fleet-wide) most-recent report for the
  `(source, check)`; reconciled by a worker, not stamped on ingestion.
- `decommissioned_at timestamptz NULL`
- `decommissioned_by text NULL`

Regenerate `schema.rs` (scrub to only these lines per the schema.rs dev-DB
pollution rule). Extend `CheckPolicy` (`crates/database/src/check_policies.rs`)
and `CheckPolicyData` (`crates/private-server/src/fns/healthchecks.rs`) with the
new fields.

Add a `Decommissioned` variant to `commons_types::issue::ResolvedReason`.

## Liveness reconciler (worker)

Rides the existing 60s DB-only loop in `crates/jobs/src/bin/monitor.rs`
alongside `sweep_staleness` / `sweep_lingering_incidents`. A few minutes' lag is
fine; ingestion is untouched.

New `database` fn (in `check_policies.rs`), one set-based statement:

```
UPDATE check_policies cp
SET last_seen = f.max_seen
FROM (SELECT source, check_name, max(last_seen) AS max_seen
      FROM issues
      WHERE server_id IS NOT NULL AND check_name IS NOT NULL
        AND source NOT IN ('canopy', 'manual')
      GROUP BY source, check_name) f
WHERE cp.source = f.source AND cp.check_name = f.check_name
  AND (cp.last_seen IS NULL OR cp.last_seen < f.max_seen);
```

Deriving from `issues` (not raw `statuses`) is cheap: it is already the
denormalised per-check state carrying `last_seen`, one small GROUP BY, no JSONB
unnesting. Synthetic `canopy`/`manual` sources are excluded — they are not
subject to decommissioning.

### Re-animation

Same reconcile pass: any decommissioned row whose recomputed `last_seen` is newer
than `decommissioned_at` is reset to the newly-registered state — `decommissioned_at
= NULL`, `ceiling = 'warning'`, `reviewed_at = NULL`, `reviewed_by = NULL`. This
puts it back in the review list capped at warning, so a resurrected check never
silently resumes a retired policy. `documentation` is preserved (still accurate);
`rules` are cleared for the same reason the ceiling resets (a warning ceiling
would cap them anyway, but a re-vetted check starts clean).

## 30-day self-alert

The reconciler also files/clears one coalescing Canopy-wide check (see
`crates/database/src/self_alerts.rs`): active while any non-decommissioned
`(source, check)` has `last_seen` older than 30 days; detail lists the offending
checks; clears when none remain. Warning-level, non-escalating.

The 7-day candidate surface is a read-time filter on the catalog, not a stored
condition (see Surfaces).

## Reader fixes

- `health_from_check_state` (`crates/database/src/issues.rs:959`): the rollup
  query must count only contributing states. Add `issues.active = true AND
  issues.resolved_at IS NULL`, and exclude checks whose catalog row is
  decommissioned (join `check_policies` / `decommissioned_at IS NULL`). This is
  the direct fix for the false "Unhealthy": the frozen `bestool-alertd` row has
  `resolved_at` set, so it drops out immediately even before decommissioning.
- `source_freshness` (`crates/database/src/issues.rs:2243`): exclude
  `(source, check)` rows whose catalog row is decommissioned, so a source whose
  checks are all decommissioned stops being "expected" and its per-server
  `stale/<source>` checks auto-close on the next `sweep_staleness`.

## Decommission action (operator)

New handler `crates/private-server/src/fns/healthchecks.rs` → `decommission`
(`TailscaleAdmin`), args `{ source, check_name }`:

1. Set `decommissioned_at = now`, `decommissioned_by = <admin>` on the catalog
   row.
2. Resolve every outstanding issue state for that `(source, check)` across all
   servers with `ResolvedReason::Decommissioned`, reusing the `Issue::resolve`
   path so incident membership re-evaluates (a decommissioned check leaves its
   incidents cleanly). A bulk helper in `database` iterating the affected issue
   ids.

Mount in `routes()`. Run `just gen-openapi` and regen `api-types.ts`.

## Surfaces (settings)

- `CheckPolicyData` gains `last_seen` and `decommissioned_at`; `healthchecks/list`
  already returns the catalog.
- `private-web` healthchecks settings (`routes/Healthchecks.tsx` /
  `HealthcheckSettings.tsx`): a "gone quiet" view — catalog rows with `last_seen`
  older than 7 days and not decommissioned — each with a **Decommission** action
  (admin-gated). Multi-select so a whole retired source's checks go in one pass.

## One-time cleanup

No bespoke migration. After deploy, an operator decommissions the entries in the
7-day list (the `bestool-alertd/*` checks). That resolves the 27 frozen rows and,
once the source has no live checks, closes the 14 `stale/bestool-alertd`
warnings on the next sweep.

## Tests

`TestDb`:
- reconciler stamps catalog `last_seen` from issues; ignores `canopy`/`manual`.
- decommission resolves a check's states fleet-wide and drops it from both
  `health_from_check_state` and `source_freshness`; a source with all checks
  decommissioned stops being expected.
- the original bug: a resolved `failed` state does not make a server unhealthy.
- re-animation resets `decommissioned_at`/`ceiling`/`reviewed_at` on a new report.
- 30-day self-alert files and clears.

Playwright (`private-web/e2e`): seed a quiet check, assert it appears in the
gone-quiet list, decommission it, assert it leaves and the server's health
recovers.

## VCS

This plan stacks below the spec-B docs. A's implementation commits land between
`plan: … (spec A)` and the spec-B commits (jj rebases the B docs on top).
