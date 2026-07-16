# Cross-cutting concerns — systemic fixes — 2026-07-16

Companion to [`logic-audit-2026-07.md`](./logic-audit-2026-07.md) and
[`apperror-variants-analysis.md`](./apperror-variants-analysis.md). The audit
found 53 individual logic bugs; many of them are not independent, they are
instances of a handful of **systemic gaps**. This report names those gaps and,
for each, proposes a *global* mechanism that retires the whole class rather than
patching site by site. No code changes here — this is a design/triage document.

The audit report already lists six narrower patterns in its "cross-cutting
patterns" section (filter-after-LIMIT in the MCP layer, the 7-day status
lookback leaking into "latest" semantics, `AppError::custom`, `security`
annotation without an extractor, TS/Rust logic-mirror drift, and
`slot_is_due` vs `slot_deadline_due`). Several of those are *sub-cases* of the
larger concerns below; this document reframes them at the altitude where a
single mechanism applies.

Priority ordering: **#1 and #2 are the highest-leverage** — between them they
underpin the two critical/high findings and roughly a dozen others. #3–#6 are
more about preventing recurrence than fixing current bugs.

---

## 1. Fail-open defaults in monitoring logic

**The concern.** Canopy is a fleet *health monitor*, but across the alerting
paths, whenever a query returns empty, a row is missing, or a call errors, the
code treats that as **"all clear"** rather than **"unknown — investigate"**. For
a monitoring system the safe default is the opposite: absence of evidence must
not read as evidence of health.

**Findings it subsumes:**
- **C1** (critical) — the staleness/reconcile scan inner-joins the schedule
  table, so default-interval groups produce an *empty* scan set, read as
  "nothing to monitor" → no alerts, fleet-wide.
- **H2** (high) — a *missing* `backup_repo_snapshots` row is treated as
  "inventory stale, skip", so the exact case the check exists for (device
  reports success, nothing landed) never alerts.
- **M5** — any HTTP response, including 5xx, counts as "reachable".
- **M7 / M8 / M9** — a filtered-empty page (filter-after-LIMIT) is reported as
  "clean" / undercounts.
- **L5** — `.ok()` swallows a DB error, reported as "is latest".

**Global mechanism.**
- Give the monitoring verdict enums a first-class **`Unknown` / `Indeterminate`**
  state, distinct from `Ok`. Today `StalenessVerdict` only has `Ok` / `Never` /
  … — there is no way to say "I couldn't determine this", so "couldn't
  determine" silently becomes "fine". Make "query returned nothing / errored"
  resolve to `Unknown`, and surface `Unknown` (a low-severity self-alert or a
  dashboard "monitoring gap" indicator) rather than swallowing it.
- Adopt a review rule for every health/alerting path: **a `None`, an empty
  result set, or an `Err` must never collapse into a benign verdict** — it is
  either propagated or explicitly turned into `Unknown`.
- Push list filters into the query (fixes the filter-after-LIMIT sub-case) so an
  empty result means "genuinely none", not "none on this page".

This single discipline spans three subsystems (backups, reachability, MCP read
tools) and covers the most severe findings in the audit.

---

## 2. A shared resilient job-loop harness

**The concern.** Every `crates/jobs/src/bin/*.rs` worker hand-rolls its own
`loop { … }`, and each one gets a *different subset* of the correctness
properties right. The bugs are really "the loop primitive doesn't exist, so each
author reinvented a slightly-broken one".

**Findings it subsumes:**
- **H4** — Slack outbox `mark_failed` never advances `deliver_after`, so there
  is no backoff; a >1-minute outage burns all 10 attempts and drops pages.
- **H5** — rotation uses no-catch-up `slot_is_due` and collides with
  maintenance's slot every period → starved (the `slot_is_due` vs
  `slot_deadline_due` pattern).
- **M11** — irreversible Slack POSTs run *inside* the claim transaction, so a
  late DB error re-sends already-posted messages.
- **M12** — `ownstatus` `continue`s past its `sleep` on error → busy-loop.
- **M13** — maintenance/inspection re-spawn a failing op every tick (anchored on
  last *success*, not last *attempt*) → the concurrency cap is exhausted by
  retries and healthy groups starve.
- Plus the `tag_reconcile` drift and `chrome_versions`' non-transactional
  refresh noted in the jobs audit.

**Global mechanism.** One worker-loop combinator that bakes in the properties
each loop currently reinvents:
- **sleep-at-top** (so an early `continue`/error can't spin — kills M12);
- **exponential backoff with jitter** on failure, persisted where the work is DB-
  backed (kills H4; jitter also de-synchronises the deterministic slot collision
  behind H5);
- **error-latching** — a failing unit is not re-attempted every tick; it backs
  off on `last_error` the way `needs_init` already latches on `last_init_error`
  (kills M13);
- **effect-after-commit ordering** / per-unit commit, so irreversible external
  effects never sit inside a transaction that can roll back (kills M11).

Port the workers onto it rather than patching each loop; the harness becomes the
one place these invariants are tested.

---

## 3. Single source of truth for derived values

**The concern.** The same derived quantity is computed in several places with
rules that have drifted apart. The divergence *is* the bug — not any single
implementation.

**Findings it subsumes:**
- **Effective backup interval** — resolved independently in
  `database/src/backup/staleness.rs`, `commons-servers/src/backup_jobs.rs`
  (`effective_interval_for_type`), and `private-server/src/fns/backups.rs`, with
  three different NULL/default policies (**C1** + **H3**: the scan treats NULL as
  manual-only, the scheduler treats it as inherit-default).
- **Semver ordering / matching** — attempted in raw SQL (**H1** per-component
  prefilter; **M2** the `version_updates` view's status-blind ranking) and
  mirrored again in TypeScript (**M17**), none deferring to the one vetted
  comparator.
- Severity-rule evaluation reimplemented in TS vs Rust (the logic-mirror
  pattern; **M17 / L19**).

**Global mechanism.**
- One `EffectiveSchedule` resolver (interval **and** retention, honouring the
  documented NULL-means-manual-only semantics) that *every* caller — scheduler,
  staleness scan, and API — goes through. No second implementation.
- A hard rule: **SQL must not attempt semver ordering.** SQL may narrow
  candidates by cheap bounds, but ordering and range-matching happen in Rust via
  `node_semver` only. This retires H1 and M2 structurally, not per-query.
- Where a language mirror is genuinely unavoidable (the in-browser rule
  preview), drive both sides from **shared conformance vectors** so drift fails a
  test (see #6).

---

## 4. Inconsistent soft-delete (`deleted_at`) scoping

**The concern.** `deleted_at IS NULL` is applied ad-hoc, per query, so it is
routinely forgotten — archived entities leak back into live reads and monitoring.

**Findings it subsumes:**
- **L4** — `production_versions` omits the filter, so archived servers skew the
  fleet version summary for 7 days.
- **L6** — group un-archive over-restores, resurrecting individually-archived
  members.
- **L22** — the version-cache backfill selects across all servers, not
  `deleted_at IS NULL`.

**Global mechanism.** A shared `not_deleted()` diesel scope helper that reads
opt out of rather than opt into — so *forgetting* it is the conspicuous case in
review, not the default. Audit every archived-entity read once against it. (A
diesel `default_scope` isn't first-class, so a named helper plus a review
checklist item is the pragmatic form.)

---

## 5. DB write + external side effect: no consistent compensation strategy

**The concern.** Operations that pair a DB write with an irreversible external
effect (a k8s Secret, a Slack POST, an HTTP proxy call) each handle failure
differently — some compensate, some don't, some wrap the irreversible effect
*inside* the transaction.

**Findings it subsumes:**
- **H6** — the `upsert` create path inserts the config then creates the Secret
  with a bare `?` and **no rollback**, while its siblings `create` /
  `create_shared` explicitly roll back on the same failure.
- **M11** — Slack POSTs inside the claim transaction (also under #2).
- `chrome_versions` — `delete_all` + row-by-row inserts with no transaction.

**Global mechanism.** A documented, uniform convention for "DB + external
effect": either **effect-after-commit** (do the DB write, commit, then perform
the effect, with the effect's own idempotency/cleanup) or an **explicit
compensation** step on failure — and apply it everywhere. H6 is literally one
handler that forgot the rollback its siblings have; a stated convention plus a
sweep removes the whole class.

---

## 6. Nothing tests handlers against their own OpenAPI annotations

**The concern.** The `#[utoipa::path]` annotations are decorative, not enforced,
so the checked-in spec (and the generated TS client) drift silently from what
handlers actually do.

**Findings it subsumes:**
- The `security`-annotation-without-extractor gap (**H9 / M15 / M16**): the spec
  advertises `tailscale-user` auth the handler never checks.
- The documented-status-vs-actual gap (**L15 / L16 / L17**), covered in detail
  by the AppError report.

**Global mechanism.** A **conformance test** over the route table that, per
route, asserts:
- every error status the handler declares is actually producible, and
- a route declaring `security(("tailscale-user" = []))` actually rejects an
  unauthenticated request (an unauthenticated call must not return 200).

This catches contract drift as a class instead of one endpoint at a time. It
complements the AppError report's structural recommendation to make
`to_http_status` **exhaustive** (drop its `_ => 500` wildcard) so that adding a
variant forces a deliberate status choice — which alone would have prevented
**L11**.

---

## Summary

| # | Concern | Global mechanism | Findings retired |
|---|---|---|---|
| 1 | Fail-open monitoring defaults | `Unknown` verdict state; empty/error ≠ Ok; filters in-query | C1, H2, M5, M7, M8, M9, L5 |
| 2 | No resilient job-loop harness | shared loop combinator (sleep-at-top, backoff+jitter, error-latch, effect-after-commit) | H4, H5, M11, M12, M13 |
| 3 | Duplicated derived-state logic | one `EffectiveSchedule` resolver; no semver in SQL; shared conformance vectors | C1, H1, H3, M2, M17, L19 |
| 4 | Inconsistent soft-delete scoping | `not_deleted()` scope helper + one-time audit | L4, L6, L22 |
| 5 | DB + external-effect consistency | effect-after-commit / explicit compensation convention | H6, M11 |
| 6 | No handler↔OpenAPI conformance test | route-table conformance test; exhaustive `to_http_status` | H9, M15, M16, L11, L15, L16, L17 |

#1 and #2 are where the leverage is. #3 and #6 mostly prevent recurrence; #4 and
#5 are contained sweeps.
