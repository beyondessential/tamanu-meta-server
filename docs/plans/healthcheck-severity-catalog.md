# Phase out bestool's "overall health" signal; add a canopy-owned healthcheck severity catalog

## Context

bestool currently reports two related-but-overlapping things to canopy:
1. The `healthy: bool` top-level on the status payload — bestool's own determination of "is the server overall healthy."
2. The `health: [{check, healthy, ...}]` array — per-check breakdown.

Canopy currently treats `healthy=false` as authoritative: it files a roll-up issue `(status, health)` at severity Error, which opens an incident. Per-check failures additionally file `(status, health/<check_name>)` issues, at Warning while overall is true and at Error while overall is false.

We are phasing alerts out of bestool and moving everything onto the healthcheck model. As part of that, **canopy should stop trusting bestool's overall-healthy determination** and instead derive the "system healthy" judgement from per-check results, with the severity-per-check being operator-configurable. Bestool itself is out of scope; if a bestool change ends up being required, flag it but don't make it here.

The work is staged in two deployable steps (v0, v1) with v2 deferred:

- **v0** — Stop filing the roll-up issue. Resolve existing roll-ups. Per-check severity *keeps the current coupling to bestool's overall signal* during this transitional window so we don't regress incident sensitivity before the catalog ships. Small, surgical change.
- **v1** — Introduce a global catalog of healthchecks with operator-configurable severities. Per-check failures source their severity from the catalog (this is the point at which canopy is no longer reading bestool's overall signal for any decision). New checks auto-insert as "pending review" defaulting to Warning. Catalog UI lives at a new top-level `/healthchecks` page.
- **v2** (future direction, not part of this plan) — Mappings that read per-check `extra` data (e.g. raise Warning when `disk_space.used_pct > N`).

Both v0 and v1 are intentionally small enough to ship independently, but v1 is what completes the user-stated goal.

## v0 — Drop the roll-up, keep per-check as-is

### Code changes

**`crates/public-server/src/statuses.rs`** — `file_health_events()`:
- Delete the "Roll-up open" block (lines 209–224) that files `NewEvent` at `(STATUS_SOURCE, HEALTH_REF)` when `!status.healthy`.
- Delete the "Roll-up close" block (lines 244–259) that files the close event when transitioning back to healthy.
- Keep `per_check_severity` selection (Warning vs Error based on `status.healthy`) and all per-check open/close logic intact.
- Keep the `healthy_by_proxy_of_silence` correction at the top of `create()` (lines 134–139) — it stops becomes a no-op for the roll-up but still correctly drives `per_check_severity` (an all-silenced unhealthy push degrades to Warning per-check, matching prior behaviour minus the deleted roll-up open).
- Drop the now-unused `HEALTH_REF` const reference (still needed as the prefix for per-check refs — keep the const, just verify no orphan use).
- Drop `roll_up_unhealthy_message` / `roll_up_unhealthy_description` helpers if they end up unreferenced.

**Status `healthy` field retention** — `StatusPayload.healthy` and the `statuses.healthy` column both stay. We still accept and persist the value so historical analysis and the UI's status snapshot can render it; we just stop letting it open an issue. Update the doc-comment on `StatusPayload.healthy` so future readers don't assume the field gates incident filing.

### Migration

New migration `migrations/<date>_resolve_overall_health_rollup_issues/up.sql`:

```sql
-- Append synthetic close events to every still-active overall-health rollup
-- issue, then deactivate the issues themselves. Mirrors what file_health_events
-- used to emit on a healthy transition, so events stay append-only and the
-- existing read paths (event coalescing, last_seen, etc.) remain consistent.

INSERT INTO events (issue_id, severity, active, message, description, hash, occurrences, occurred_at, last_seen)
SELECT i.id, 'info', false,
       'Overall health signal deprecated; rollup retired.',
       NULL,
       encode(digest(...), 'hex'),  -- match the existing hash recipe used in issues.rs
       1, NOW(), NOW()
FROM issues i
WHERE i.source = 'status' AND i.ref = 'health' AND i.active = true;

UPDATE issues
SET active = false,
    resolved_at = NOW(),
    resolved_reason = 'Overall health signal deprecated; per-check issues remain.',
    updated_at = NOW(),
    last_seen = NOW()
WHERE source = 'status' AND ref = 'health' AND active = true;
```

Verify the exact hash expression by looking at how `NewEvent::save` computes the hash in `crates/database/src/issues.rs` and replicating it in SQL (or compute a sentinel value distinct from any possible real hash — fine as long as it's deterministic and the column is non-null).

The migration must also walk any incidents that *only* had rollup-issue contributors and close them via `re_evaluate_incident_membership` semantics — but since the existing trigger/save logic already handles incident closure when contributors go inactive, the simplest implementation is to call the same Rust path during migration. Two options:

1. Pure-SQL migration that updates the `issues` rows and lets the next status push or a one-shot rerun of `re_evaluate_incident_membership` close orphaned incidents. (The next reachability sweep or status push for each server will trigger it naturally.)
2. A one-shot Rust binary in the migrate crate that iterates active rollup issues and calls `NewEvent::save(... active=false ...)` per the existing pattern. Cleaner state but more code.

**Recommendation: option 1**. The transition window where an incident with only a rollup contributor stays open is short — closed by the next status push or sweep — and avoids new migration tooling.

### Tests to update

`crates/public-server/tests/statuses.rs`:
- `submit_status_with_healthy_false_persists` — flip assertion: no issue at `(status, health)` should be created.
- Any other test that asserts on the roll-up issue — update or remove. Look for tests referencing `ref: "health"` (no `/`).
- Keep all per-check transition tests as-is; severity coupling is unchanged.

Add a migration test in `crates/database/tests/` (mirror style of existing tests) that:
- Seeds an active `(status, health)` issue.
- Runs the migration (or its equivalent SQL).
- Asserts `active = false`, `resolved_reason` set, and a close event was appended.

### Critical files (v0)

- `crates/public-server/src/statuses.rs` (edit)
- `crates/public-server/tests/statuses.rs` (edit)
- `migrations/<date>_resolve_overall_health_rollup_issues/{up,down}.sql` (new)
- `crates/database/tests/<new test>.rs` (new)

### Verification (v0)

- `nice just check` — no warnings.
- `nice just test-package public-server` — status tests pass with updated assertions.
- `nice just test-package database` — migration test passes.
- Manual: spin up local stack (`just watch-private-api` + `just watch-private-web`), push a status with `healthy=false` from `bestool` (or a curl with mTLS cert), confirm no rollup issue appears in `/issues` but per-check failure issues do.

---

## v1 — Catalog of healthchecks with operator-configurable severities

### Data model

New migration `migrations/<date>_healthcheck_severity_catalog/up.sql`:

```sql
CREATE TABLE healthcheck_severities (
    check_name TEXT PRIMARY KEY,
    severity TEXT NOT NULL DEFAULT 'warning'
        CHECK (severity IN ('emergency','alert','critical','error','warning','notice','info','debug')),
    first_seen TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_at TIMESTAMPTZ,            -- NULL ⇒ pending review
    reviewed_by TEXT,                   -- admin email when reviewed
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    notes TEXT
);

CREATE INDEX healthcheck_severities_reviewed_at_idx ON healthcheck_severities (reviewed_at);
```

No `severity_when_healthy` column — close events stay hardcoded at `Severity::Info` per user decision (revisit alongside a possible broader rethink of events later).

Diesel model in new file `crates/database/src/healthcheck_severities.rs`, re-exported from `lib.rs`. Methods needed:
- `upsert_default(conn, &check_name) -> Result<()>` — `INSERT ... ON CONFLICT (check_name) DO NOTHING`. Idempotent. Called from status ingestion for every check seen.
- `severity_for(conn, &check_name) -> Result<Severity>` — read row, fall back to `Severity::Warning` if (race) the row doesn't exist yet. In practice always exists because we upsert before reading.
- `list(conn) -> Result<Vec<HealthcheckSeverity>>` — for the catalog UI.
- `update(conn, &check_name, severity, reviewed_by) -> Result<HealthcheckSeverity>` — sets severity, `reviewed_at = NOW()`, `reviewed_by`, `updated_at`.

### Ingestion change

`crates/public-server/src/statuses.rs` — `file_health_events()`:
- Drop `per_check_severity` computation (the line currently selecting Warning/Error from `status.healthy`).
- For each entry in `status.health` (failing **and** healthy), call `healthcheck_severities::upsert_default(conn, &check.check_name)` first. We do this for healthy entries too so the catalog covers every check operators might want to pre-configure.
- For each *failing* check, look up severity via `healthcheck_severities::severity_for(conn, &check_name)` and pass that into `NewEvent.severity`.
- `status.healthy` is no longer consulted anywhere in this function. Per the user goal of fully ignoring the signal, this is the point at which we stop reading it.

Close events: continue to file at `Severity::Info` regardless of catalog content.

### Private-server API

New module `crates/private-server/src/fns/healthchecks.rs`, mounted at `/api/healthchecks` by `crate::fns::routes()`. Handlers (all `TailscaleAdmin`-gated):

- `POST /api/healthchecks/list` — `{}` body, returns `Vec<HealthcheckSeverityData>` (catalog row + a `pending_review: bool` derived from `reviewed_at IS NULL`).
- `POST /api/healthchecks/update` — `{ check_name, severity }` body, sets severity and stamps `reviewed_at = NOW()`, `reviewed_by = <admin email>`. Returns the updated row.

If we want a "mark reviewed without changing severity" affordance, that's a degenerate case of update (pass current severity). Keep one endpoint, not two.

Follow the existing pattern in `crates/private-server/src/fns/issues.rs` (struct of args, `#[utoipa::path]`, `routes()` helper).

After adding handlers, run `just gen-openapi` and commit both `private-web/openapi.json` and `private-web/src/api-types.ts`.

### React UI

New route file `private-web/src/routes/Healthchecks.tsx`. Wired in `private-web/src/App.tsx`:
- New `NavItem { label: "Healthchecks", to: "/healthchecks" }` in `BASE_NAV` between Status and Incidents (or wherever fits the existing visual order — place after Incidents seems natural since healthchecks feed incidents).
- New `<Route path="/healthchecks" element={<Healthchecks />} />`.

UI structure (Material UI, matching existing patterns):
- Table with columns: Check name, Severity (dropdown when admin), Pending review badge, First seen (TimeAgo), Last reviewed by, Last reviewed (TimeAgo).
- A small "X checks pending review" summary at the top, with quick filter to show only those.
- Severity dropdown uses the existing `SeverityChip` component for display; on edit, an MUI `Select` with the eight severity values (in RFC 5424 order).
- Editing severity calls `useApiAction("healthchecks", "update", ...)`; the row updates in place via the existing `canopy-data-changed` event broadcast (see `useReloadInterval` for the polling-and-refresh pattern).

Reuse:
- `SeverityChip` at `private-web/src/components/SeverityChip.tsx`.
- `TimeAgo` at `private-web/src/components/TimeAgo.tsx`.
- `useApi` / `useApiAction` from `private-web/src/api.ts`.
- `useIsAdmin` from `private-web/src/hooks/useIsAdmin.tsx`.

### Tests (v1)

Database (`crates/database/tests/healthcheck_severities.rs`, new):
- Upsert is idempotent.
- Update stamps `reviewed_at` and `reviewed_by`, bumps `updated_at`.
- `severity_for` returns the stored value, and a sensible default if the row is somehow missing.

Public server (`crates/public-server/tests/statuses.rs`):
- Pushing a status with a previously-unknown check name inserts a catalog row with severity=warning, reviewed_at=null.
- Failing check files an issue at the catalog's current severity.
- After updating the catalog to Error, the next status push files the per-check issue at Error.
- A check still failing with bestool reporting `overall healthy = false` is no longer escalated to Error solely because of the overall flag — severity comes from the catalog now.

Private server (`crates/private-server/tests/endpoints.rs` or a new file):
- `list` returns all rows with `pending_review` set.
- `update` requires admin; rejects bad severity; stamps reviewer correctly.

E2E (optional, `private-web/e2e/healthchecks.spec.ts`):
- Catalog page renders, edit dropdown changes value, pending-review badge clears after save.

### Documentation

`docs/plans/issues-followups.md` (the existing follow-ups file mentions a per-group severity floor as a future enhancement) — note that v1 lands an orthogonal version of this (per-check, not per-group). Consider whether the floor idea is now redundant or still wanted.

`ERRORS.md` — only update if v1 introduces new error problem types (e.g. invalid severity in update). Look at `crates/commons-errors` for the appropriate `AppError` variant; `BadRequest` likely suffices without new entries.

### Critical files (v1)

- `migrations/<date>_healthcheck_severity_catalog/{up,down}.sql` (new)
- `crates/database/src/healthcheck_severities.rs` (new)
- `crates/database/src/lib.rs` (edit — re-export)
- `crates/database/src/schema.rs` (regenerated by migration tooling)
- `crates/public-server/src/statuses.rs` (edit `file_health_events`)
- `crates/private-server/src/fns/healthchecks.rs` (new)
- `crates/private-server/src/fns/mod.rs` (edit — mount routes)
- `private-web/openapi.json` (regenerated)
- `private-web/src/api-types.ts` (regenerated)
- `private-web/src/App.tsx` (edit — nav + route)
- `private-web/src/routes/Healthchecks.tsx` (new)
- Test files as above.

### Verification (v1)

- `nice just check`.
- `nice just test-package database`, `public-server`, `private-server`.
- `nice just typecheck` for frontend types after regenerating api-types.
- `nice just gen-openapi` ran cleanly and the two regenerated files are committed.
- Manual: in dev stack, push a status with a never-seen check name → catalog row appears with pending-review badge, severity = warning. Push the check as failing → issue filed at Warning. Edit catalog to Error → push again → next issue is Error. Confirm `healthy: false` at the top level no longer changes per-check severity.
- Playwright (if added): `npm run test:e2e -- healthchecks` passes.

---

## Out of scope / call-outs

- **bestool changes**: none. If we discover that bestool needs an update to drop `healthy` from its outgoing payload or stop using `/events` for alerts, flag the change in a separate followup rather than touching that repo here.
- **v2 (extra-data mappings)**: explicitly deferred. Spec at a later date.
- **Removing the `statuses.healthy` column**: not part of this plan. The column stays so historical analysis and the status snapshot UI can still render the value. Deletion could happen once bestool stops sending the field, in a much later migration.
- **Per-group severity overrides**: not in scope. Catalog is global, per the user's framing.
- **Renaming/retiring checks**: catalog rows for retired check names will accumulate. Either accept this or add a delete endpoint later — not in v1.
