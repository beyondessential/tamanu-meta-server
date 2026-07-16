# Canopy codebase logic-bug audit — 2026-07-16

Audit of the whole workspace (database, public-server, private-server, jobs,
commons crates, canopy-mcp, private-web React app, and SQL migrations) for
**logic bugs**: code whose behaviour diverges from its evident intent. Out of
scope by design: style, missing features, hypothetical hardening, and pure
simplification. **No fixes are applied in this branch** — this is a findings
report only.

Base commit: `58e95dc`.

Each finding was cross-checked against callers, tests, DB constraints, and the
documented contract before inclusion. Two of the highest-severity items
(backup-staleness scan set, manual-only interval collapse) were additionally
re-verified by hand against the current source while writing this report; the
`version_updates` view and duplicate-detected items were reproduced live on
Postgres 16 by the auditing pass.

## How to read the two axes

- **Severity/impact** — blast radius and consequence. *Critical* = silent
  fleet-wide monitoring blindspot, data loss, or auth bypass; *High* = wrong
  data on a hot path or a class of alerts/backups silently failing; *Medium* =
  wrong result in a real but narrower scenario; *Low* = cosmetic, wrong status
  code, or a rare edge.
- **Test difficulty** — how hard a regression test for the fix is to write.
  *Easy* = a pure-function or single `TestDb`/HTTP-harness test; *Moderate* =
  multi-step setup, a stub, or Playwright with seeded state; *Hard* = needs
  time control, concurrency interleaving, or fault injection.

## Summary matrix (severity × test difficulty)

| | Easy to test | Moderate | Hard |
|---|---|---|---|
| **Critical** | C1 | | |
| **High** | H1, H2, H3, H4, H7¹ | H5, H6, H8 | H9¹ |
| **Medium** | M1, M2, M6, M7, M8, M9, M10, M17, M19 | M3, M5¹, M12, M20 | M4¹, M11, M13¹, M15¹, M16, M18¹ |
| **Low** | L1, L2, L3, L4, L7, L8, L10, L11, L12, L14, L15, L17, L19, L20, L22 | L9, L13, L16, L21, L23 | L5 |

¹ confidence *medium* rather than *high* — see the finding. Everything else is high-confidence.

Totals: **1 critical, 9 high, 20 medium, 23 low** (53 findings). Several were
independently reported by more than one reviewer and are consolidated here.

---

## Critical

### C1 — Backup staleness/reconcile monitoring silently skips every group on the default interval
- **Files**: `crates/database/src/backup/staleness.rs:115-154` (`scan_rows`), consumed by `staleness::sweep` and `reconcile::sweep` (`crates/database/src/backup.rs:28-33`), run from `crates/jobs/src/bin/monitor.rs`.
- **Bug**: The scan set is documented as every enabled `(server, type)` in a ready group "whose **effective** schedule has a non-NULL `expected_interval`". The effective interval everywhere else is `schedule override ?? backup_type_defaults.default_interval` (`effective_interval_for_type`, `crates/commons-servers/src/backup_jobs.rs:142-158` — verified). But `scan_rows` **inner-joins** `server_group_backup_schedule` and filters `expected_interval IS NOT NULL`, with no fallback to the type default. A `(group, type)` with no override row — the normal, out-of-the-box case, since `tamanu-postgres` is seeded with a canopy-wide 6h default (`migrations/2026-06-21-151343-0000`) and rows are only written by explicit `set_schedule` — is never scanned. The same rows feed `reconcile::sweep`, so report-gap, size-mismatch, and reconcile-missing checks are also skipped.
- **Failure scenario**: Operator marks a group's repo ready, enables `tamanu-postgres` on a server, leaves the inherited 6h default (the intended path). Backups run fine for weeks via `backups_due_now_for_server`; the device then breaks and stops backing up. No `backup-staleness` / `backup-never` issue is ever filed, and none of the reconcile checks run either. Backups can silently stop fleet-wide for any group without an explicit override.
- **Fix direction**: left-join the schedule table and coalesce the interval with the type default, matching `effective_interval_for_type`.
- **Test**: easy — `TestDb`: ready config + enabled capability + a `backup_type_defaults` row with an interval and **no** schedule row; assert `scan_rows` is non-empty.
- Reported independently by three reviewers (database-backup, jobs, and confirmed by hand).

---

## High

### H1 — Semver range resolution uses a per-component `>=` prefilter, excluding valid matches
- **Files**: `crates/database/src/versions.rs:179-210` (`Version::get_latest_matching`); the same filter is copy-pasted into `crates/public-server/src/versions.rs:72-86` (`latest_matching_ready`), which drives the public update/download/HTML endpoints.
- **Bug**: The SQL prefilter compares each semver component independently against the range's `min_version()` (`major >= … AND minor >= … AND patch >= …`), as if semver ordering were component-wise. It is lexicographic: `2.6.0` satisfies `^2.5.1` but fails `patch >= 1`; `3.0.0` fails `minor >= 5`. Rows that satisfy the range are dropped before the in-memory `range.satisfies` check, which can only shrink the set.
- **Failure scenario**: Published `1.0.5` and `1.1.0`; a client requests `>=1.0.5` (or a hyphen range). The filter requires `patch >= 5`, excluding `1.1.0`, so the endpoint resolves to `1.0.5`'s artifacts — or 404s (`NoMatchingVersions`) if only `1.1.0` exists. Out-of-date clients are pointed at stale versions or told no update exists. Common inputs (`2.10.x`, `^X.Y.Z` rewritten to `~X.Y.Z`) stay within one minor line, which is why existing tests don't catch it.
- **Test**: easy — db test on `get_latest_matching`, or an HTTP test with a URL-encoded cross-minor range.
- Reported independently by two reviewers.

### H2 — Reconcile "missing snapshot" alert can never fire when nothing ever landed
- **File**: `crates/database/src/backup/reconcile.rs:93-121,170`.
- **Bug**: Inventory freshness is derived from the per-`(server, type)` `backup_repo_snapshots` row's `observed_at`, and the **absence** of that row is treated as "inventory stale → skip". But the inspection job (`crates/jobs/src/backup/complete.rs:83-100`) writes a snapshot row only for sources actually found in the repo. In the exact case the arm exists for — a device reports success but nothing landed — `snap` is `None`, `inventory_fresh` is false, and the match falls into the empty `(true, false)` arm. The group-level freshness signal the doc-comment actually describes (`last_inspected_at_for_group` / `BackupRepoStats.observed_at`) is never consulted.
- **Failure scenario**: A device (buggy or malicious) reports `outcome=success` every cycle while uploading nothing. Inspection runs fresh against other sources, staleness sees fresh successes, and `backup-reconcile-missing` — the headline "the report is false or the upload didn't persist" alert — is never raised. Data loss is discovered only at restore time.
- **Test**: easy — mirror `reconcile_files_missing_when_report_fresh_but_no_snapshot` but skip the snapshot upsert; assert an issue is filed.
- Reported by two reviewers.

### H3 — A "manual-only" schedule override (NULL interval) falls through to the type default
- **Files**: `crates/commons-servers/src/backup_jobs.rs:147-155` (`effective_interval_for_type`, verified); the same pattern in `crates/private-server/src/fns/backups.rs:595-607` and `:1380-1384` (`group_schedules`).
- **Bug**: `.and_then(|s| s.expected_interval)` collapses "no override row" (inherit default) with "override row whose interval is NULL". The model and API both document a present-but-NULL interval as **manual-only** — distinct from an absent row (`crates/database/src/backups.rs:681-684`; `set_schedule` OpenAPI at `fns/backups.rs:1214`), and deliberately different from the same row's `retention: None` which does mean "inherit". The resolver instead falls back to `default_interval`, so manual-only is impossible for any type with a canopy-wide default. Meanwhile the staleness scan (C1) *does* honour NULL as manual-only, so the two halves disagree.
- **Failure scenario**: Operator sets `expected_interval: null` for `tamanu-postgres` to make a group manual-only. `backups_due_now_for_server` still resolves 6h and keeps commanding scheduled backups; `group_schedules` shows `effective_interval: 21600` and a `next_run_at` despite the just-saved override — and no staleness alert can ever fire for that pair.
- **Test**: easy — upsert a schedule row with `None` interval + a type default with an interval; assert `backups_due_now_for_server` does *not* return the type as due and `group_schedules` reports null.
- Reported by three reviewers.

### H4 — Slack outbox has no retry backoff: any Slack outage beyond ~1 minute permanently drops notifications
- **Files**: `crates/jobs/src/bin/slacker_outbox.rs:33-34,194-237` with `crates/database/src/slack_outbox/mod.rs:278-295`.
- **Bug**: `mark_failed` only increments `attempts`; it never advances `deliver_after`, even though `claim_pending` filters on it. A failed row is re-claimed on the very next 5-second tick, so all `MAX_ATTEMPTS = 10` are burned in ~45-90 seconds, after which the row is stamped `gave_up_at` and never retried. The give-up escalation (`file_self_event` → self-alert → a new outbox row) is delivered through the same failing Slack path, so it is dropped the same way.
- **Failure scenario**: Slack webhooks return 5xx for ~3 minutes (a routine Slack incident). Every incident open/resolve enqueued in that window exhausts its 10 attempts and is permanently given up; when Slack recovers, operators are never paged for those incidents.
- **Test**: easy — call `mark_failed`, then assert `claim_pending` does *not* immediately re-return the row (i.e. `deliver_after` moved forward).

### H5 — Weekly passphrase rotation deterministically collides with maintenance and is starved
- **Files**: `crates/jobs/src/backup/rotation.rs:271`, `crates/commons-servers/src/backup_jobs.rs:421-444`, `crates/jobs/src/backup/maintenance.rs:98`.
- **Bug**: `jitter_slot(group, window) = hash % window`. The rotation period (default 7d = 604800s) equals the full-maintenance window, so rotation's slot is the *identical second* as the maintenance deadline (and because 86400 divides 604800, the daily quick-maintenance time-of-day too). Maintenance uses `slot_deadline_due` (stays due until it runs, so it always wins) while rotation uses `slot_is_due` (fires only if a tick lands inside a single 60s window, no catch-up), and both share the per-group in-flight set. On the collision minute maintenance claims the group first; rotation's only eligible tick sees it in-flight, skips, and its window passes — deferring rotation a full period, every period.
- **Failure scenario**: Any ready group whose full maintenance takes longer than a minute (i.e. effectively all of them): the passphrase never rotates (or rotates far less often than configured), silently — only a debug-level "rotation tick ok" is logged.
- **Test**: moderate — the slot-offset equality is a pure unit assertion; end-to-end starvation needs time control.

### H6 — `upsert` create path leaves a half-created backup config when the passphrase Secret can't be stored
- **File**: `crates/private-server/src/fns/backups.rs:1063-1084`.
- **Bug**: The sibling `create` (`:840-846`) and `create_shared` (`:931-937`) handlers explicitly roll back the just-inserted config row if `kube.create_password` fails, with a comment stating the invariant ("a failed Secret create must not leave a half-created config stuck in `provisioning` with no passphrase"). The `upsert` create path inserts the config and then creates the Secret with a bare `?` — no rollback.
- **Failure scenario**: IaC calls `/api/backups/upsert` for a new group; the Secret store write fails transiently. The config row persists in `provisioning` with a `repo_password_ref` pointing at nothing. Every retry now takes the *update* path (config exists), which never creates the Secret, so the repo init job can never find the passphrase — the group is permanently stuck until an operator deletes and recreates it.
- **Test**: moderate — needs a secret-store stub that fails once (the in-memory harness always succeeds).

### H7 — XFCC client-cert parsing takes the first (untrusted) element instead of the last
- **File**: `crates/commons-servers/src/device_auth/mtls.rs:57-62`; used by every device-authenticated public-server endpoint and by enrollment's `resolve_spki`.
- **Bug**: In Envoy's `x-forwarded-client-cert` convention each trusted proxy *appends* its element, so the element describing the immediate TLS client of the terminating proxy is the **last** one; the first is whatever arrived from upstream. The code splits on `,` (so it anticipates multiple elements) but takes `.next()` — the untrusted end.
- **Failure scenario**: With a two-hop proxy chain (or an append-forward XFCC mode), an attacker sends `x-forwarded-client-cert: Cert=<PEM of a victim device's public cert>`; the terminating proxy appends its own element, canopy reads the attacker-supplied first element, derives the victim's SPKI, and authenticates as the victim device. Device certificates are not secret, so this is a device-auth bypass. Safe only if the proxy is configured to fully replace the header (`SANITIZE_SET`) — which this code cannot assume given it parses multiple elements.
- **Confidence**: medium (conditional on proxy configuration).
- **Test**: easy to unit-test the parser against a two-element header; hard end-to-end.

### H8 — `IncidentsLink` passes a server id where a group id is required, so issues always read empty
- **File**: `private-web/src/components/IncidentsLink.tsx:34-38,66-84`.
- **Bug**: The component is handed a **server** UUID (`ServerDetail` passes `data.server.id`) but feeds it to `issues.list` as `serverGroupId`, which the backend resolves as `servers.group_id = gid` (`crates/database/src/issues.rs:1298-1305`). A server id never equals a group id, so the query matches zero servers and always returns `[]`. Unlike `incidents.list_for_server`, `issues.list` does no server→group resolution. Both `/incidents?group=${serverId}` links carry the same mismatch (the Incidents page feeds the param straight back into `issues.list`).
- **Failure scenario**: A server has active issues but no open incident. The header button should read "Active issues"; instead `hasActive` is always false so it reads "Past issues", and clicking it opens a page that filters against a nonexistent group ("No issues match the current filters", plus an invalid value in the group dropdown). An operator concludes the server has no issues when it does.
- **Test**: moderate — Playwright: seed a server with an active issue and no incident; assert the button label and linked-page contents.

### H9 — SQL playground executes queries even when Tailscale authentication fails
- **File**: `crates/private-server/src/fns/sql.rs:125,135` (and `:249-251` for `get_last_user_query`).
- **Bug**: `execute_query` is annotated `security(("tailscale-user" = []))` and documents recording each query under the caller's identity, but it wraps the extractor in `Result` and `unwrap_or_default()`s it. In release builds (where the dev auth bypass is off) a failed extraction is swallowed and the query runs anyway, attributed to the empty-string login. Sibling read endpoints enforce with a bare `_user: TailscaleUser`; only `commons::is_current_user_admin` legitimately uses the `Result` form, and it has a comment saying why — `sql.rs` has none.
- **Failure scenario**: In a release deployment, a request reaching the private-server without identity headers from a non-tailnet client IP (e.g. in-cluster traffic bypassing the Tailscale ingress; `tailnet_guard` only rejects header-less callers whose IP is in the tailnet ranges) can run arbitrary read-only SQL against `RO_DATABASE_URL`, with the audit row's `tailscale_user` blank.
- **Confidence**: medium (the debug-build bypass means the failure path only exists in release).
- **Test**: hard.

---

## Medium

### M1 — Artifact de-duplication uses `dedup_by_key` after sorting by a different key
- **File**: `crates/database/src/artifacts.rs:94-98` (`get_for_version`; served by the public `list_artifacts`, HTML artifact pages, and `/mobile`).
- **Bug**: `Vec::dedup_by_key` removes only *consecutive* duplicates, but `sort_by_specificity` reorders so all exact-version artifacts precede ranges — destroying the `(artifact_type, platform)` adjacency from the SQL `ORDER BY`. Non-adjacent duplicate `(type, platform)` pairs survive, so the "most specific wins" / "one entry per type+platform" contract is broken.
- **Failure scenario**: A version with `(mobile, android, exact)`, `(mobile, android, range)`, and `(desktop, windows, exact)` sorts to `[exact-android, exact-windows, range-android]` — no adjacent duplicates — so both android artifacts are returned with conflicting download URLs. Existing tests only cover the single-combination case (where the pair is adjacent).
- **Test**: easy — add one extra exact artifact of another type to the existing specificity fixture.
- Reported by two reviewers.

### M2 — `version_updates` view / `get_updates_for_version` hides a whole minor line when its newest patch is draft or yanked
- **Files**: `migrations/2025-11-25-053943-0000_version_status_enum/up.sql:22-29` (view), consumed by `crates/database/src/versions.rs:150-177`; surfaced by the canopy-mcp `get_version` tool's `available_updates`.
- **Bug**: The view reduces each `(major, minor)` line to its single highest-patch row (`ROW_NUMBER() … WHERE rn = 1`) with **no status filter inside**; the caller then filters `status = 'published'`. If the newest patch in a minor is draft or yanked, that row is the only one the view exposes for the line, so the published filter drops the entire minor rather than falling back to the newest published patch. The public-server `update_for` endpoint does filter-then-reduce in Rust precisely to avoid this (`crates/public-server/src/versions.rs:508-511`).
- **Failure scenario**: Reproduced live on Postgres 16 — with `2.46.2` published and `2.46.3` draft, the caller's query returns 0 rows for the 2.46 line, so MCP `get_version("2.45.0")` stops listing any 2.46 update even though 2.46.2 is published and offered by the public endpoint.
- **Test**: easy — db test with the harness.
- Reported by two reviewers.

### M3 — Resolve notification suppressed when a pending escalation open is cancelled
- **File**: `crates/database/src/issues.rs:1178-1185` with `crates/database/src/slack_outbox/mod.rs:122-144`.
- **Bug**: `enqueue_slack_resolve_inner` assumes "a pending open row exists" ⇒ "Slack never heard about the incident", so it cancels the pending open and returns early. But the escalation feature enqueues a *second* `incident_open` row for the same incident (`issues.rs:689-691`), so an incident whose original open was already delivered can still have a pending (escalation) open. Cancelling that row and skipping the resolve leaves Slack showing an unresolved incident.
- **Failure scenario**: Incident opens at Error, open delivered to Slack; a Critical issue joins → escalation open enqueued; the incident resolves before the drainer ships the escalation. The resolve is skipped, so Slack permanently shows the incident as open.
- **Test**: moderate — force-deliver the original open, push a Critical join, resolve, assert an `incident_resolve` row exists.

### M4 — TOCTOU: a Warning-severity issue can open a brand-new incident
- **File**: `crates/database/src/issues.rs:631,644-665` with `find_or_open_incident` (`:1046-1084`).
- **Bug**: For sub-Error issues, `should_join` is justified solely by `group_open`, read before any lock. If the open incident is closed by a concurrent transaction between that read and `find_or_open_incident` (the close path locks only the incident row, not the group), the re-query finds nothing open and creates a new incident, enqueuing a Slack open for a Warning/Info issue — violating the invariant that only Error/Critical open incidents.
- **Confidence**: medium. **Test**: hard (needs a controlled two-connection interleave).

### M5 — `ping_server` counts any HTTP response, including 5xx, as reachable
- **File**: `crates/database/src/statuses.rs:144-165`.
- **Bug**: `reqwest::Response` is `Ok` for any status code; there is no `error_for_status()` check. A 502/503 from a reverse proxy in front of a dead Tamanu server (or a 404 from a parked host) is logged "ping success" and inserted as a fresh `healthy: true` status.
- **Failure scenario**: A legacy device-less server (the pingtask's population) goes down behind nginx, which answers 502. Pingtask keeps writing healthy statuses, so `sweep_reachability` never files an issue and the outage stays invisible while the proxy answers.
- **Confidence**: medium. **Test**: moderate (HTTP stub returning 5xx).

### M6 — `group_details` 404s for every group when no version is published
- **File**: `crates/private-server/src/fns/statuses.rs:213-215`.
- **Bug**: `Version::get_latest_matching(conn, "*")` returns `NoMatchingVersions` (→ HTTP 404) when nothing is published, and `group_details` propagates it with `?`. The sibling endpoints `servers::get_detail` and `statuses::snapshot` deliberately map this to `version_distance: None` (with a comment: "shouldn't 404 just because no versions are published yet").
- **Failure scenario**: A fresh deployment (or all versions still draft) with a healthy group → the status board's `group_details` call 404s for every group and the fleet page renders empty/error.
- **Test**: easy — seed a group + server, zero published versions, assert 200.

### M7 — MCP `find_issues` applies the `server_id` filter after the database LIMIT
- **File**: `crates/canopy-mcp/src/incidents.rs:373-387`.
- **Bug**: `Issue::list` orders by `last_seen desc` with SQL `LIMIT` (default 100); the `server_id` filter is then applied in Rust on that truncated page, unlike `group_id` which is part of the query.
- **Failure scenario**: With >100 active issues fleet-wide, a server whose issues were seen slightly earlier than the top 100 returns `count: 0` — an agent triaging that server is told it is clean.
- **Test**: easy.

### M8 — MCP `find_incidents` applies the `status` filter after the database LIMIT
- **File**: `crates/canopy-mcp/src/incidents.rs:236-245`.
- **Bug**: `Incident::list_open_since` orders by `opened_at desc` with `LIMIT 100`; the open/resolved filter is applied afterward in Rust. `status: "open"` therefore returns only the open incidents among the 100 most-recently-opened rows.
- **Failure scenario**: The window commonly holds >100 flap rows/week; a still-open incident opened 6 days ago sorts below them and is omitted entirely — an active incident is invisible and the counts undercount.
- **Test**: easy.

### M9 — MCP `find_backup_problems` misses failures behind the per-group row limit
- **File**: `crates/canopy-mcp/src/fleet.rs:229-270`.
- **Bug**: The advertised window is "failed runs in the last 24h", but the scan only inspects the 20 newest runs per group (5 for maintenance) and applies the time filter client-side after that row cap, so the effective window shrinks for busy groups.
- **Failure scenario**: A group reporting ~120 runs/day has a failure 8h ago followed by >20 successes; `find_backup_problems` reports no `failed_run` despite it being inside the 24h window. Same shape for `stuck_maintenance`.
- **Test**: easy.

### M10 — Disabling a restore replica leaves its active restore-verification alert open forever
- **File**: `crates/database/src/restore.rs:229-244` (`update`); sweep filter at `:738`.
- **Bug**: `update` recovers stale alerts on a scope change and `delete` recovers them ("the overdue sweep only walks current declarations, so a stale key would otherwise never clear"), but `sweep_overdue` also filters `enabled = true`. Flipping `enabled` false removes the key from the sweep the same way, yet triggers no recovery, and a disabled replica generates no consumer work so `record_report` can't clear it either.
- **Failure scenario**: A verification goes overdue and pages; the operator disables the declaration to decommission it; the alert and its incident stay open indefinitely.
- **Test**: easy.

### M11 — Slack outbox marks a whole batch in one transaction; a late DB error resends posted messages
- **File**: `crates/jobs/src/bin/slacker_outbox.rs:177-247`.
- **Bug**: The irreversible HTTP POSTs run inside the claim transaction. If any later DB write in the loop errors (or the commit fails), the `delivered_at` stamps of rows already posted roll back, and they are re-posted next tick.
- **Failure scenario**: A batch of 5; rows 1-4 post to Slack; the `mark_delivered` for row 5 hits a connection drop → the whole tx rolls back → rows 1-4 are re-posted → duplicate incident pages.
- **Test**: hard (mid-transaction fault injection).

### M12 — `ownstatus` error path busy-loops without sleeping
- **File**: `crates/jobs/src/bin/ownstatus.rs:40-48`.
- **Bug**: On error the loop `continue`s, skipping the `sleep(60s)` at the bottom, so a persistent error (DB unreachable, missing `Server::own` row) spins as fast as the query fails. The sibling `pingtask.rs` puts the sleep at the top precisely to avoid this.
- **Failure scenario**: The database is briefly unreachable and returns errors quickly → thousands of retries per minute and matching `error!` lines until it recovers.
- **Test**: moderate.

### M13 — Failing maintenance/inspection ops retry every 60s with no backoff and can exhaust the concurrency cap
- **Files**: `crates/jobs/src/backup/maintenance.rs:442-464`, `crates/jobs/src/backup/inspection.rs:150-190`.
- **Bug**: `slot_deadline_due` "stays due until a run happens", but both schedulers anchor on the last *successful* run. A group whose kopia op keeps failing (wrong passphrase Secret, deleted bucket, revoked role) is re-spawned every tick forever — contrast `needs_init`, which latches on `last_init_error` to avoid exactly this. Each retry holds one of `CANOPY_BACKUP_MAX_CONCURRENCY` (default 4) permits for the op's (often slow) duration.
- **Failure scenario**: Four groups with revoked roles retry continuously, occupying all permits, so `try_claim` fails for every healthy group and fleet-wide maintenance/inspection/init silently stop until the 8-day `MAINTENANCE_STALE` alert fires.
- **Confidence**: medium. **Test**: hard.

### M14 — Rotation reconcile's "Broken" repo state only logs, though defined as an alerting condition
- **File**: `crates/jobs/src/backup/rotation.rs:133-135,206-213,249-252`.
- **Bug**: `Recovery::Broken` (the repo opens with *neither* passphrase — backups and restores are dead for the group) is documented "alert", but the only sink is `bail!` → an `error!` log line. No `raise_group_event`/self-alert is filed, unlike every comparable failure, and the condition recurs each period.
- **Failure scenario**: A pod crashes between kopia's two format-blob writes during rotation; the next cycle detects Broken, but the dashboard stays green while every device backup fails against an unopenable repo until backup-staleness eventually fires.
- **Test**: moderate.

### M15 — Bestool `save_snippet` swallows auth failure and records the editor as empty string
- **File**: `crates/private-server/src/fns/bestool.rs:132,135`.
- **Bug**: Same `Result<TailscaleUser>` + `unwrap_or_default()` pattern as H9: annotated `security` and documented "recorded under the caller's Tailscale identity", but a failed extraction is defaulted, so in release an unauthenticated caller can create/supersede snippets with `editor = ""` instead of getting a 401.
- **Confidence**: medium. **Test**: hard (debug-build bypass).

### M16 — `silenced_refs` list endpoints declare `tailscale-user` security but enforce nothing
- **File**: `crates/private-server/src/fns/silenced_refs.rs:80-87,104-110`.
- **Bug**: Both list handlers carry the `tailscale-user` security annotation but omit the `TailscaleUser` extractor entirely, unlike every other user-gated read endpoint. The OpenAPI spec (and generated TS client) claim auth is required; the server never checks it. The same annotation-without-extractor pattern also appears in `backups.rs` read handlers, so a fix should sweep the whole `fns` tree.
- **Failure scenario**: In release, a request with no identity headers from a non-tailnet IP to `/api/silenced_refs/list_for_server` returns 200 with the silence list (including `created_by` operator logins) where the contract says 401.
- **Test**: hard (debug-build bypass).

### M17 — Rule-preview `in_range` coerces versions that the Rust evaluator rejects
- **File**: `private-web/src/lib/healthcheck-rule-eval.ts:150`.
- **Bug**: The file declares the Rust `IfLadder` evaluator (`crates/database/src/healthcheck_severities.rs`) the source of truth and itself a mirror, but the Rust side parses strictly (`parse::<node_semver::Version>()`, no coercion) while the TS preview runs the LHS through `semver.coerce()`, which fabricates full versions from partials (`"2.28"` → `2.28.0`) and strips prerelease/build suffixes. Rust rejects `"2.28"` outright and treats `"2.28.0-rc.1"` as a real prerelease that `>=2.28.0` doesn't satisfy.
- **Failure scenario**: An admin authors `status.tamanuVersion in_range >=2.28.0 → critical`; against a server reporting `2.28.0-rc.1` the editor preview shows "would file at critical", but production ingestion evaluates false and files at the base severity — the admin ships a rule believing it works.
- **Test**: easy — unit-test `evaluate()` with a prerelease/partial LHS and assert it matches Rust behaviour.

### M18 — `useApi` keeps the previous entity's data when the query identity changes
- **File**: `private-web/src/api.ts:89-94`.
- **Bug**: The keep-prior-data behaviour is intended for background refetches of the *same* query, but it also applies when `deps` change to a different identity. Detail routes reuse the mounted component across `/servers/A` → `/servers/B`, so A's full data (name, health chip, checks, page title) renders under B's URL with no loading indicator until B resolves. Aggravation: `VersionDetail`'s `StatusControl` seeds `useState(detail.status)` once, so navigating between two versions shows the previous version's status with the "Change" button enabled — submitting would write the stale status onto the new version.
- **Failure scenario**: Clicking a sibling server in "Other servers in this group" shows the old server's Healthy chip and checks under the new server's URL for the whole fetch on a slow connection.
- **Confidence**: medium. **Test**: hard (needs network throttling).

### M19 — `valid_snippet_name` CHECK forbids a trailing `%` instead of a backslash
- **File**: `migrations/2025-12-23-083905-0000_bestool_snippets_name_constraint/up.sql:19`.
- **Bug**: In Postgres `LIKE`, backslash is the default escape character, so `NOT LIKE '%\%'` rejects names *ending in a percent sign*, not names containing a backslash. The intended pattern is `'%\\%'`. Verified live: `'foo\bar' LIKE '%\%'` → false, `'foo%' LIKE '%\%'` → true. There is no app-level validation, so the CHECK is the only guard.
- **Failure scenario**: (a) `report\daily` is accepted despite the comment requiring backslash rejection (names become client-side filenames on Windows bestool); (b) a legitimate name like `top100%` is rejected with a raw constraint 500.
- **Test**: easy.

### M20 — `server_groups` backfill scrambles group assignment when two root servers share a name
- **File**: `migrations/2026-05-22-120000-0000_server_groups/up.sql:56-61` (`root_to_group`).
- **Bug**: The root→group mapping joins inserted groups back to roots **by name**, but `servers.name` is not unique. Two roots with the same name each insert a group with that name; the join produces the full cross product, and the final `UPDATE … FROM` picks arbitrary rows. The comment's claim that "both halves produce the same string in the same row order" is false under collisions (and join order is undefined regardless).
- **Failure scenario**: Reproduced live — two roots named `Fiji`, each with one child, ended with both roots in one group and both children in the other, separating servers from their own parents; incidents were then rekeyed to the wrong group. This is a one-shot backfill, so any damage is already in prod wherever duplicate root names existed at migration time.
- **Confidence**: high on mechanics; prod prevalence of duplicate root names is unverifiable from the repo. **Test**: moderate.

---

## Low

### L1 — `Admin::add` returns 404 for an already-existing admin
- **File**: `crates/database/src/admins.rs:37-45`. `on_conflict_do_nothing()` inserts zero rows on a duplicate, so `.get_result()` yields `NotFound` → HTTP 404, though the handler documents idempotent success. **Test**: easy.

### L2 — Chrome minimum version computed with lexicographic string ordering
- **File**: `crates/database/src/chrome_releases.rs:51-58`. `version` is TEXT holding numeric majors; `order_by(version.asc())` sorts `"100" < "99"`. At any date where two length-differing majors are both live (e.g. 99 and 100), the reported `min_chrome_version` is wrong (recurs at 999→1000). **Test**: easy.

### L3 — Reachability sweep treats `alert_when_down_for` thresholds over 7 days as "never reported"
- **File**: `crates/database/src/statuses.rs:240,256-264`. `latest_for_servers` only looks back 7 days, so a server last seen >7d ago yields `elapsed = None` → unconditional down with the "has never reported" message, silently capping the per-server threshold at 7 days and mislabelling the issue. **Test**: easy (backdated status row).

### L4 — `production_versions` includes archived servers
- **File**: `crates/database/src/statuses.rs:521-535` (surfaced by `crates/private-server/src/fns/statuses.rs:118`). Missing the `deleted_at.is_null()` filter that every sibling query has, so an archived production server contributes its version to `/api/statuses/summary` for up to 7 days after archival. **Test**: easy. Reported by two reviewers.

### L5 — `is_latest_in_minor` swallows DB errors into "is latest"
- **File**: `crates/database/src/versions.rs:294-307`. `.first(db).await.ok()` turns any Diesel error (not just NotFound) into `None`, and `unwrap_or(true)` reports "latest". Since the version under test is itself published, an empty result is only reachable via a genuine error, so this branch is purely the error path — weakening the publish→draft demotion guard in `fns/versions.rs:490`. **Test**: hard.

### L6 — Group un-archive resurrects members that were archived individually
- **File**: `crates/database/src/server_groups.rs:269-293`. Restore is documented as the inverse of `soft_delete`'s cascade but restores *every* archived member, including servers an operator deliberately archived before the group was archived; `is_monitored` survives archival, so they rejoin monitoring and start generating "never reported" alerts. **Test**: easy.

### L7 — "Newer version available" banner recommends and links known-issue versions
- **File**: `crates/public-server/src/versions.rs:341-361`. The page's own version is resolved via `latest_matching_ready` (which excludes known-issue ranges), but the banner's "latest in minor" only filters on `Published`, so it can point at — and link to — a version hidden from every listing whose own page 404s. **Test**: easy.

### L8 — Changelog size cap is 1 GiB where 1 MiB is intended
- **File**: `crates/public-server/src/versions.rs:221-222`. `1024 * 1024 * 1024` is 1 GiB, not the documented "up to 1 MiB" — a ×1024 unit slip, so the intended cap is never enforced. **Test**: easy.

### L9 — MCP failed-auth rate limiter runs after the DB token lookup it is meant to bound
- **File**: `crates/public-server/src/mcp.rs:69-99`. The module comment says the failed-auth budget "bounds the DB lookups a token-guesser can burn", but the limiter is only consulted inside `refuse()`, after `McpToken::find_active` has already hit the database — so an over-budget IP still performs a full connection checkout + lookup per guess. **Test**: moderate.

### L10 — Duplicate check names in a status push: result from the last entry, context from the first
- **File**: `crates/public-server/src/statuses.rs:599-622`. Validation permits multiple `health[]` entries with the same `check` name. `collect_check_results` (a BTreeMap) keeps the *last* entry's result, while `find_health_entry` (message, extras, and the severity-rule `EvaluationContext`) returns the *first*, so the filed event mixes one entry's result with another's data and a value-based severity rule can evaluate against the wrong entry. **Test**: easy.

### L11 — Client-supplied malformed versions/headers return HTTP 500
- **File**: `crates/commons-errors/src/lib.rs:197-224`. `AppError::VersionParse` and `AppError::Header` fall through to the `_ => INTERNAL_SERVER_ERROR` arm, though both arise purely from client input (URL path version segments, the `X-Version` header extractor). The sibling `UnusableRange` maps to 400. Result: `GET /versions/update-for/not-a-version` and a device sending `X-Version: garbage` both 500, polluting 5xx monitoring and making clients retry what can't succeed. **Test**: easy.

### L12 — In-memory `BackupSecrets` test double allows `create_password` over an existing secret
- **File**: `crates/commons-servers/src/backup_secrets.rs:147-156`. The contract is "fails if it already exists" (the Kube variant returns 409 → `Upstream`), but the `Memory` variant silently overwrites and returns `Ok`. Since tests and the e2e fixture run against `Memory`, a double-create path passes in tests but 502s in production, and the rejection can't be exercised against the fixture. **Test**: easy.

### L13 — `find_servers` loses the "retained" version/last-seen after 7 days offline
- **File**: `crates/canopy-mcp/src/servers.rs:162-166,296-318`. The `version` field is documented "retained even when long offline", but `summarize` sources it from `Status::latest_for_servers` (7-day lookback), so a server offline >7d shows `version: null` / `last_seen: null` — diverging from `get_server`, which implements the documented fallback. Fleet version surveys via `find_servers` undercount long-offline servers. **Confidence**: medium. **Test**: moderate.

### L14 — `get_restore_replica` recent checks are the group's newest 50, not the replica's
- **File**: `crates/canopy-mcp/src/restore.rs:166-172`. Fetches the group's 50 newest checks then filters to this replica (filter-after-limit), so a low-frequency replica alongside a chatty one shows `recent_checks: []` and looks never-checked. **Test**: easy.

### L15 — `get_snippet` returns 500 for a missing snippet, not the documented 404
- **File**: `crates/private-server/src/fns/bestool.rs:186`. `AppError::custom("Snippet not found")` maps to 500 `/errors/other`, but the OpenAPI contract declares 404. Should use `diesel::result::Error::NotFound.into()`. **Test**: easy.

### L16 — `attach_tailscale` returns the wrong status for an unresolvable identifier
- **Files**: `crates/private-server/src/fns/devices.rs:938` (500 via `AppError::custom`) and `crates/private-server/src/fns/servers.rs:1204-1208` (400 via `BadRequest`). Both document 404 for "identifier does not resolve to a known tailnet node", and the two handlers disagree with each other and with the spec. **Test**: moderate (needs a tailnet-directory stub resolving to `None`).

### L17 — Documented 400 validation errors actually return 500
- **Files**: `crates/private-server/src/fns/issues.rs:577,778`, `crates/private-server/src/fns/incidents.rs:507`. Empty-`ref` / empty-note-body checks use `AppError::custom` (→ 500) where the endpoints document 400; the codebase convention is `AppError::BadRequest` (used correctly by `mcp_tokens::mint` for the identical empty-name check). **Test**: easy.

### L18 — SQL playground `execution_time_ms` includes endpoint overhead
- **File**: `crates/private-server/src/fns/sql.rs:136-153`. The field is documented to start "immediately after the query has been recorded to history", but `start_time` is taken before the main-pool checkout, the history INSERT, the RO-pool checkout, and the transaction BEGIN, so it measures overhead rather than query execution (badly skewed under RO-pool contention). **Test**: moderate.

### L19 — Rule-preview `==`/`!=` uses reference equality where Rust uses structural JSON equality
- **File**: `private-web/src/lib/healthcheck-rule-eval.ts:61-70`. The comment claims "strict structural equality (JSON-level)", but `===` is reference equality for arrays/objects, so `[1,2] == [1,2]` is true in Rust's `json_equal` and false in the preview. Secondary: `toNumber` uses `Number()` (accepts `0x10`) where Rust uses `parse::<f64>()` (rejects it). The preview can invert an operator's understanding of a live rule. **Test**: easy.

### L20 — `humanSeconds` compounds rounding across units, overstating durations
- **File**: `private-web/src/lib/humanDuration.ts:9-15`. Each unit is rounded from the previous *rounded* unit, not from raw seconds, so 5375s (1h29m35s) renders "2h" and 129,480s renders "2d". Affects incident/run/maintenance duration displays. **Test**: easy (`humanSeconds(5375)` should be `"1h"`).

### L21 — Healthchecks catalog links interpolate raw check names into URLs
- **File**: `private-web/src/routes/Healthchecks.tsx:153,159,199`. `types.ts` documents that check names are arbitrary device strings and every link builder must go through `healthcheckPath` (which `HealthcheckAttention.tsx` does), but the catalog interpolates the raw name, so a name containing `/`, `?`, `#`, or `%` routes to the wrong page. **Test**: moderate.

### L22 — `server_group_version_cache` backfill ignores rank aliases and archived servers
- **File**: `migrations/2026-06-02-071412-0000_add_server_group_version_cache/up.sql:48-56`. The rank `CASE` claims to mirror the Rust helpers but omits the `live`/`prod`/`staging` aliases that `ServerRank::from_str` accepts (sending them to the lowest-priority ELSE bucket), and it selects among all servers rather than `deleted_at IS NULL`. A group whose central server has `rank='live'` alongside a `rank='test'` box can get the test server chosen as its `version_server_id`. **Confidence**: high on mismatch, prod prevalence unknown. **Test**: easy.

### L23 — Partition-creation functions race: check-then-create without `IF NOT EXISTS` or locking
- **Files**: `migrations/2025-11-26-022557-0000_partition_management_functions/up.sql:33-48` and `2025-11-26-074456-0000_device_connections_partition_functions/up.sql:33-48`. The existence probe and `CREATE TABLE` are not atomic and the CREATE uses no `IF NOT EXISTS`/lock, so two concurrent invocations (external cron per the COMMENTs; an overlapping manual run or a second scheduler replica) can both pass the check, the loser aborting and rolling back the whole call — including later weeks it hadn't created. Repeated collisions eventually exhaust the partition buffer and inserts fail with "no partition of relation found for row". **Test**: moderate (two concurrent connections).

---

## Cross-cutting patterns worth a systemic fix

- **Filter-after-LIMIT in the MCP layer** (M7, M8, M9, L14): several read tools
  apply a Rust-side `.retain`/`.filter` after a SQL `LIMIT`, so the limit bites
  the unfiltered set. Push these predicates into the queries.
- **7-day status lookback leaking into "latest" semantics** (L3, L4, L13, and
  part of M5): `latest_for_servers`' fixed 7-day window is reused in places that
  mean "the most recent status, however old", producing false "never reported"
  and dropped versions.
- **`AppError::custom` where a typed variant exists** (L11, L15, L16, L17):
  client-input errors surface as 500s, diverging from the checked-in OpenAPI
  contracts and polluting 5xx alerting. A pass mapping these to `BadRequest`/
  `NotFound` would align the whole `fns` tree. A structured proposal for the
  additional `AppError` variants this needs — and a call-site migration map — is
  in [`apperror-variants-analysis.md`](./apperror-variants-analysis.md).
- **`security` annotation without the matching extractor** (H9, M15, M16): the
  OpenAPI spec advertises auth that the handler does not enforce in release
  builds. Worth an audit sweep across every annotated handler.
- **TS/Rust logic mirrors drifting** (M17, L19, L20): the healthcheck-rule
  preview and duration formatting reimplement Rust logic in TypeScript and have
  diverged; a shared conformance test vector would catch this.
- **`slot_is_due` vs `slot_deadline_due`** (H5, and the `tag_reconcile` variant
  noted during the jobs audit): jobs using the no-catch-up `slot_is_due` in a
  drifting loop can silently skip an entire period. The codebase already added
  `slot_deadline_due` to fix this class; the remaining `slot_is_due` callers
  should be reviewed.
