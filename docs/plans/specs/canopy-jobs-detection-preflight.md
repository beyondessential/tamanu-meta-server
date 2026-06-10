# Spec: canopy-jobs-detection — staleness, reconciliation, alerting & upstream preflight

Component of the **backup-credentials** system. Authoritative design:
[`../backup-credentials.md`](../backup-credentials.md) (stage-2 stub:
[`../backup-credentials-blind-relay.md`](../backup-credentials-blind-relay.md)).

This spec covers the **detection / alerting / preflight** half of the
Canopy control plane: the periodic jobs that decide whether each group's
backups are healthy and raise issues/events when they are not. It does
**not** cover credential issuance (`public-server` endpoints), the
maintenance/inspection/init Jobs themselves, the operator UI, or the IaC —
those are sibling components. It *consumes* the tables those components
write (`backup_runs`, `backup_maintenance_runs`, `backup_repo_snapshots`,
`server_group_backup_config`) and the AWS-client plumbing they introduce.

## Purpose

Three classes of periodic check, all running as loops in the `jobs` crate
and all alerting through the existing issues/events/incidents model
(`NewEvent::save`, `source="canopy"`):

1. **Signal 1 — staleness scan** (DB-only, frequent): scan servers that are
   *expected* to be backed up and alert when no recent successful backup is
   on record. Server-centric. Also catches stuck maintenance.
2. **Signal reconciliation (1 / 2 / 3)**: cross-check what devices
   *reported* (signal 1, `backup_runs`) against what *actually landed*
   (signal 2, `backup_repo_snapshots`) and — later — what PGRO proved
   *restorable* (signal 3, `backup_restore_checks`). Disagreement is itself
   an alert; repo corruption (poisoning) is a group-level critical.
3. **Upstream preflight** (AWS-touching, hash-jittered): Canopy checking its
   *own* access — `GetCallerIdentity` (shared, ~minute) plus per-group deep
   checks (both purposes issue working creds + Object-Lock still in place),
   hourly.

The shared thread running through all three: a **group-level** failure
(can't mint creds, lock removed, repo corrupt, restore broken) must page
**regardless of any server's `is_monitored`**, whereas **per-server**
staleness obeys the existing `is_monitored` gate. The incident model is
server-keyed today, so the group-level path needs new plumbing (see
[Group-level alerting](#group-level-alerting-server-independent)).

## Where it lives

New binaries in the **`jobs` crate**, following the
`reachability`/`pingtask` template (`spawn() -> JoinHandle<()>`, a
`loop { sleep(…); pool.get(); … }`, `#[tokio::main]` calling
`spawn().await`):

- `crates/jobs/src/bin/backup_staleness.rs` — signal-1 + reconciliation
  scan (DB-only; no AWS). ~1–5 min cadence. Can ride the existing
  `reachability` minute loop instead of a new pod if we prefer one fewer
  Deployment — **decision below**.
- `crates/jobs/src/bin/backup_preflight.rs` — upstream preflight (AWS SDK;
  STS + S3). Shared `GetCallerIdentity` on a ~minute tick; per-group deep
  checks hash-jittered hourly.

The bulk of the logic lives in the **`database` crate** as model functions
(like `Status::sweep_reachability`), so it's testable with
`commons_tests::db::TestDb::run` without standing up a binary:

- `crates/database/src/backup/staleness.rs` (or extend an existing
  `backup` module the issuance component creates) — the scan + classify +
  file-events logic.
- `crates/database/src/backup/reconcile.rs` — signal 1↔2(↔3)
  reconciliation.
- The preflight's AWS calls live in the **binary** (the `database` crate
  must not gain an AWS dependency); the preflight's *alerting* reuses the
  same `NewEvent` helpers. The binary reads config rows via a `database`
  model function and calls the AWS SDK directly.

Per the workspace memory: `database` is the only crate allowed diesel; the
preflight binary depends on the new AWS-SDK plumbing the issuance component
adds (`aws-config` + `aws-sdk-sts` + `aws-sdk-s3`). The `jobs` crate gains
those deps for `backup_preflight` only.

## Refs and sources (issues/events keys)

All events use `source = "canopy"` (the existing `CANOPY_SOURCE` constant
in `statuses.rs`; promote it somewhere shared if both crates need it, or
re-declare a `const BACKUP_*` set in the backup module). Refs (new
constants — keep them all in one place, e.g. `database::backup::refs`):

| ref | level | severity (active) | severity (recovery) | gate |
|-----|-------|-------------------|---------------------|------|
| `backup-staleness` | server | `Error` | `Info` (`active:false`) | `is_monitored` |
| `backup-never` | server | `Error` | n/a (clears when first success lands) | `is_monitored` |
| `backup-maintenance-stale` | group | `Error` | `Info` | none (group-level) |
| `backup-reconcile-missing` | group | `Error` | `Info` | none (group-level) |
| `backup-reconcile-report-gap` | server | `Warning` | `Info` | `is_monitored` |
| `backup-corruption` | group | `Critical` | `Info` | none (group-level) |
| `preflight-identity` | fleet/group | `Critical` | `Info` | none (group-level) |
| `preflight-assume` | group | `Error` | `Info` | none (group-level) |
| `preflight-object-lock` | group | `Critical` | `Info` | none (group-level) |
| `restore-verification` (signal 3, later) | group | `Error` | `Info` | none (group-level) |

Notes:
- Staleness/never/report-gap are **per-server** → ordinary
  `NewEvent::save(conn, server_id, Some(device_id))`. They inherit the
  `is_monitored` incident gate by design (see plan: some prods are
  intentionally intermittently-alive; per-server backup noise on them is
  unwanted). They are still *recorded* (visible on the server page) even
  when unmonitored — `NewEvent::save` records the issue/event unconditionally
  and only skips the incident contribution.
- Everything marked **group-level** must page even on unmonitored servers,
  so it must **not** go through a per-server `NewEvent::save` (which would
  re-inherit the gate). See [Group-level alerting](#group-level-alerting-server-independent).
- `Error`+ is required for `opens_incident()` (`OPENS_INCIDENT = [Critical,
  Error]`, `commons-types/src/issue.rs`). `Warning`/`Info` only join an
  already-open incident for context; they never open one. So the
  report-gap notice (`Warning`) is deliberately non-paging on its own.

## Signal 1 — staleness scan

Server-centric. The subject is the **server** being protected; the device
is the actor recorded in `backup_runs`/snapshot tags.

### Scanned set

Servers in a group whose `server_group_backup_config` has:
- `status = 'ready'` (dormant configs — `provisioning`/`escrow_pending` —
  are not yet expected to back up), **and**
- a non-NULL `expected_interval` (manual-only groups have no schedule, so
  no staleness alerting — they're simply not in the set).

A manual-only or unconfigured group is therefore never scanned, so
unauthorized/un-set-up devices never alert. Implement as a single query
joining `servers` → `server_group_backup_config` (on `servers.group_id`)
filtered as above, returning `(server_id, group_id, expected_interval,
config.created_at)`.

### Per-server classification

For each scanned server, find its most recent `backup_runs` row with
`purpose = 'backup' AND outcome = 'success'` (the `(device_id, …)` /
`(group_id, reported_at DESC)` indexes support this; a server-centric query
joins runs to the server via `group_id` **and** the server identity — see
the source-mapping note below). Let `grace = expected_interval * 2`.

- **Stale** — a prior successful backup exists but none newer than
  `now - grace` → file `backup-staleness` at `Error`, `active:true`.
- **Never backed up** — *no* successful `purpose='backup'` row ever, **and**
  the server has been expected long enough: `now - anchor > grace`, where

  ```
  anchor = max( MIN(first_seen over this server's device_server_associations rows),
                server_group_backup_config.created_at )
  ```

  → file `backup-never` at `Error`, `active:true`. Below the grace from the
  anchor: no alert yet (freshly-present server or freshly-authorized group
  must not false-alarm).
- **Recovered** — a previously-stale server reporting success again: file
  `backup-staleness` `active:false` at `Info` (the issue leaves the
  incident and auto-closes). Mirror the reachability sweep's
  `(false, Some(issue)) if !issue.active => continue` short-circuit so we
  don't re-file an already-closed recovery every tick.

**Anchor details (do not get these wrong — they're explicit decisions):**
- `first_seen` in `device_server_associations` is per `(device_id,
  server_id)` **pair**, *not* a per-server scalar. Use
  `MIN(first_seen)` over **all** of that server's association rows
  (earliest any device saw it). Schema:
  `device_server_associations (device_id, server_id, first_seen, last_seen)`.
- `created_at` is `server_group_backup_config.created_at` (group-authorized
  time). A server present long ago but whose group was authorized 5 minutes
  ago must use the *later* of the two, so a just-authorized group doesn't
  instantly fire `backup-never` on every member.
- Filter runs on `purpose='backup'` **specifically** — a recent successful
  *restore* must **not** reset backup staleness.

### Mapping a `backup_run` to a server

`backup_runs` carries `device_id` + `group_id` but **not** `server_id`
directly. The protected server is identified via the kopia source
(`canopy@<server-id>:<path>`, recorded in `backup_repo_snapshots.server_id`)
and via the device→server association at report time. For signal 1, resolve
the server from the run's `device_id` via `Server::live_by_device_id` (the
`servers_device_id_unique` partial unique index guarantees at most one live
server per device). Scan-side, it's cleaner to drive **from the server**:
for each scanned server, find runs whose `device_id` is one of that
server's associated devices and whose `group_id` matches. Encode this as a
single classify query rather than per-server round-trips.

### Maintenance staleness

`backup_maintenance_runs` (group-level) feeds the same scan: a group whose
last `outcome='success'` maintenance run (any `kind`) is older than a
maintenance-cadence threshold (full-weekly default → e.g. `8 days`; make it
a constant, not `expected_interval`-derived, since maintenance cadence is
independent of backup cadence) → file `backup-maintenance-stale` at `Error`
via the **group-level** path. Recovery: a fresh successful maintenance run
clears it.

## Signal reconciliation (1 / 2 / 3)

Runs in the same `backup_staleness` loop (after signal-1 classify), reading
`backup_repo_snapshots` (signal-2 ground truth) against `backup_runs`
(signal-1 reports). Per scanned server (resolved to a kopia `source` =
`canopy@<server-id>:<path>`, so the join key is `server_id`):

- **report says success but no recent snapshot** (a `backup_runs` success
  newer than `grace`, but `backup_repo_snapshots.latest_snapshot_at` for
  that source is older than `grace`, or no snapshot row at all) → the report
  is wrong or the upload didn't persist. **`backup-reconcile-missing`**,
  `Error`, **group-level** (a device lying about success / data not landing
  endangers the group's actual recoverability, so it pages regardless of
  monitored). This is the case signal 1 alone cannot catch.
- **recent snapshot but no report** (`latest_snapshot_at` fresh, but no
  recent `backup_runs` success) → backups are fine, the *reporting path* is
  broken. **`backup-reconcile-report-gap`**, `Warning`, **per-server**
  (low-severity, non-paging — it's a telemetry gap, not a backup failure).
- **neither** → genuinely stale; already covered by signal 1, emit nothing
  extra here (avoid double-filing on the same `(server)`).

Signal 2 is only as fresh as the inspection Job's last run; if
`backup_repo_snapshots.observed_at` for a group is itself stale (older than
the inspection floor), reconciliation can't conclude "missing" reliably —
**skip the `reconcile-missing` verdict when signal-2 data is stale** and
instead rely on the inspection Job's own failure to surface (it writes
`backup_repo_snapshots`/stats; a Job that stops running is caught by the
preflight/maintenance-staleness machinery, not here). Record this as a
guard so a lagging inspector doesn't produce false "report lied" alerts.

**Poisoning / corruption** is reported by the inspection Job (signal 2),
not computed here: when inspection detects content-blob hash mismatch /
unreadable index, it raises **`backup-corruption`** at `Critical`,
group-level. This spec owns the *alerting shape* (the constant, severity,
group-level routing, recovery-runbook pointer in the message body); the
*detection* (running `kopia` verify) is the inspection-Job component. To
avoid two components both knowing how to raise a group-level event, expose
a single helper (below) that the inspection Job calls.

**Signal 3 (restore-verification, later/additive):** PGRO reports
per-replica restore outcomes into a future `backup_restore_checks` table; a
failed/stale restorability check is **`restore-verification`** at `Error`,
group-level. Same group-level helper. Stubbed here so the routing is
designed-for, not bolted on; the table + ingest endpoint are out of scope
for this component's first cut.

## Group-level alerting (server-independent)

**The core mechanism wrinkle.** The incident model
(`crates/database/src/issues.rs`) is **server-keyed**: `Issue.server_id` is
`NOT NULL`, `NewEvent::save(conn, server_id, device_id)` requires a server,
and `re_evaluate_incident_membership` gates incident contribution on that
server's `is_monitored`. Incidents themselves are **group-keyed**
(`incidents.server_group_id`). There is no "group-level issue with no
server" path today. Group-level backup checks must page regardless of
`is_monitored`, so routing them through a per-server `NewEvent::save` is
wrong (it would inherit the monitored gate and could be silenced by an
unmonitored member).

**Decision required — pick one (flagged in the plan as
implementation-time):**

- **Option A — representative monitored server.** Pick a deterministic
  server in the group (e.g. the highest-rank live member, reusing
  `ServerGroup::highest_member_ranks` ordering) and file against it, but
  **bypass the monitored gate** for these refs. This needs a new code path
  because `re_evaluate_incident_membership` hard-gates on `monitored`;
  passing `monitored=true` unconditionally for group-level refs is the
  smallest change but is a lie in the data. Fragile if the group has no
  live members.
- **Option B — group sentinel issue (recommended).** Add first-class
  support for a group-scoped issue with no member server. Concretely: make
  `issues.server_id` nullable **or** add an `issues.server_group_id`
  nullable column, and teach `re_evaluate_incident_membership` /
  `find_or_open_incident` to accept a group directly (the incident is
  already group-keyed, so `find_or_open_incident(conn, group_id, …)` works
  as-is — the only gap is producing an `Issue` that points at a group, not
  a server, and skipping the `is_monitored` lookup for it). This is the
  clean model and matches "group/control-plane concern, not any one
  server's." It's a migration + a branch in the membership evaluator.

This spec **recommends Option B** and treats it as the deliverable's
central new piece of shared plumbing. Provide one helper that both this
component and the inspection Job call:

```rust
// database::backup::alerts (new)
pub async fn raise_group_event(
    conn: &mut AsyncPgConnection,
    group_id: Uuid,
    r#ref: &str,
    severity: Severity,
    description: Option<&str>,
    message: &str,
    active: bool,
) -> Result<()>;
```

Internally it find-or-creates a **group-scoped** issue keyed by
`(server_group_id, source="canopy", ref)`, appends/coalesces an event (reuse
`hash_event`), and runs the group-aware membership evaluation that ignores
`is_monitored`. Recovery is the same `(source, ref)` with `active:false` at
a lower severity, which lets the issue leave the incident and auto-close —
identical lifecycle to the per-server path. **Do not** add an
`Incident::open_for`; there is no such function — reuse
`find_or_open_incident` → `enqueue_slack_open` → `SlackOutbox::enqueue`,
which the existing evaluator already drives.

The migration for Option B must be a separate `just migration` step
(`just migration backup_group_scoped_issues`); never hand-create migration
dirs. If `issues.server_id` becomes nullable, audit every existing query in
`issues.rs` that assumes it non-null (the model is large — `list_for_server`,
`list`, `reconcile_open_incidents`, `re_evaluate_incident_membership`'s
`Server::get_by_id`). `reconcile_open_incidents` (run on reachability
startup) must handle group-scoped issues (no server → resolve group
directly, skip the `is_monitored` short-circuit). This is the
refactor-thoroughly cost of Option B and must not be half-done.

## Upstream preflight

Watches **Canopy's own upstream access**, not the devices. Lives in
`backup_preflight.rs`. Alert, **never gate readiness** (a failing check must
not pull the pod out of rotation — that makes it worse).

### Shared check (every ~minute, on the loop tick)

- **`sts:GetCallerIdentity`** — confirms the pod's IRSA web-identity is
  mounted and valid. Cheap; rides the minute loop. On failure → raise
  **`preflight-identity`** at `Critical`, group-level (route it against a
  fleet sentinel — Option B's group-scoped issue keyed to a "control-plane"
  pseudo-group, **or** fan out one `preflight-identity` per configured group
  since "every group's per-group check fails" is the same signal). The plan
  says: a check failing for *every* group points at the shared IRSA
  identity rather than any one bucket — so emitting per-group and letting
  the operator see the fan-out is acceptable, but a single fleet-level alert
  is cleaner. **Decision required** — see open questions.

### Per-group deep checks (hourly, hash-jittered)

For each `status='ready'` group with a config row, on its jittered slot
(`hash(group_id) mod window`, stable per group — same scheme as maintenance
and inspection; factor the jitter helper so all three share it):

1. **Both purposes issue working creds.** Cross-account `sts:AssumeRole` on
   the group's `target_role_arn`:
   - **backup path**: plain assume (no session policy), then a **read-only
     no-op** S3 call against the bucket (e.g. `HeadBucket` or
     `GetBucketLocation` — a harmless op the backup role policy allows).
   - **restore path**: assume **with the read-only restore session policy**
     (the normative JSON from the plan — `GetObject` + unconditioned
     `GetBucketLocation` + conditioned `ListBucket`), then the same
     read-only no-op. This proves the restore session policy actually works
     and catches the `GetBucketLocation`-folded-under-`s3:prefix` class of
     bug **proactively**, while plain backup issuance still looks fine.

   Any failure (assume or no-op) → **`preflight-assume`**, `Error`,
   group-level. Message should distinguish which purpose/leg failed.
2. **Object Lock still in place.** `s3:GetBucketObjectLockConfiguration` on
   the group's bucket; assert it returns an enabled lock with `mode`
   present and `days >= 30` (GOVERNANCE, the `backups` stack's `mode:
   'GOVERNANCE', days: 30`). Missing/weakened lock → **`preflight-object-lock`**,
   `Critical`, group-level (the whole "can't destroy backups" guarantee
   rests on it, and there's no other symptom). This action is **not** in
   `AWS_S3_MULTIPART_ACTIONS`; the issuance/IaC component must add it to the
   per-bucket role Canopy assumes, or the check itself 403s on day one —
   note that dependency, don't silently absorb it.

Prefer **behavioural** checks (assume + harmless S3 op) over IAM/policy
*introspection*: behavioural checks test the real path and need no extra
`iam:Get*`. The Object-Lock read is the one allowed exception.

The **maintenance path** needs no separate preflight: the read-only
inspection Job already connects each group's repo on its cadence (proving
reachability + password), and maintenance-specific failures surface via
`backup_maintenance_runs` → `backup-maintenance-stale` (signal 1 above).

### Reactive rate-tracking (light)

The live paths are signals too: `/backup-credentials` 502s on STS failure
and maintenance failures land in `backup_maintenance_runs`. Out of scope to
build a metrics pipeline here, but note that a spike between hourly
preflights should ideally surface — for the first cut, the hourly preflight
+ maintenance staleness cover it; richer rate-tracking is deferred.

## Loop / scheduling shape

Mirror `reachability`/`pingtask`:

```rust
pub fn spawn() -> JoinHandle<()> {
    let pool = database::init();
    task::spawn(async move {
        loop {
            sleep(Duration::from_secs(60)).await;
            let Ok(mut db) = pool.get().await else { error!(…); continue; };
            // signal-1 + reconcile scan (DB only)
        }
    })
}
```

- **`backup_staleness`**: DB-only, 60 s tick. Runs signal-1 classify +
  reconciliation each tick. Cheap (a couple of indexed queries + per-server
  classify). **Decision:** stand up its own single-replica Deployment in
  `ops/pulumi/tamanu/meta/src/jobs.ts`, *or* fold the scan into the existing
  `reachability` loop (which already does a minute-cadence DB sweep + the
  startup `reconcile_open_incidents`). Folding in avoids a new pod; a
  separate binary keeps concerns isolated and is independently
  schedulable. Recommend a **separate binary** for blast-radius and testing
  clarity, matching the plan's "new `crates/jobs/src/bin/<name>.rs`"
  framing.
- **`backup_preflight`**: AWS-touching. 60 s tick for `GetCallerIdentity`;
  per-group deep checks fire only when the tick lands in the group's
  jittered hourly slot. Needs the AWS client + IRSA (greenfield — the
  issuance/IaC component introduces the SDK deps, the ServiceAccount, and
  the IRSA role; this binary reuses them). Its own single-replica
  Deployment.

Hash-jitter helper (shared with maintenance/inspection):
`fn jitter_slot(group_id: Uuid, window: Duration) -> Duration` →
`hash(group_id) mod window`, stable per group. Put it in `commons-servers`
or a shared `backup` util so all schedulers agree.

## Interfaces / contracts

### Consumes (written by sibling components)

- **`server_group_backup_config`** — `group_id`, `expected_interval`
  (NULL / set states), `created_at`, `status` (`provisioning` /
  `escrow_pending` / `ready`), `bucket`, `target_role_arn`, `region`. Read
  via a new `database` model fn, e.g.
  `BackupConfig::scannable(conn) -> Vec<ScanRow>` and
  `BackupConfig::ready_groups(conn) -> Vec<…>`.
- **`backup_runs`** — `device_id`, `group_id`, `purpose`, `outcome`,
  `reported_at`. (Written by `POST /backup-report`, issuance component.)
- **`backup_maintenance_runs`** — `group_id`, `kind`, `outcome`,
  `started_at`/`finished_at`. (Written by maintenance Jobs.)
- **`backup_repo_snapshots`** — `group_id`, `source`, `server_id`,
  `latest_snapshot_at`, `observed_at`. (Written by inspection Job.)
- **`device_server_associations`** — `(device_id, server_id, first_seen,
  last_seen)`, for the `MIN(first_seen)` anchor.
- **`servers`** / `Server::live_by_device_id`, `is_monitored`, `group_id`.
- **AWS SDK plumbing** (`aws-config`, `aws-sdk-sts`, `aws-sdk-s3`), the
  ServiceAccount + IRSA role, and `s3:GetBucketObjectLockConfiguration` on
  the per-bucket roles — all introduced by the issuance/IaC components.

### Provides (to other components / operators)

- **`database::backup::alerts::raise_group_event(conn, group_id, ref,
  severity, …)`** — the single group-level alerting entrypoint. The
  **inspection Job** calls it for `backup-corruption`; **PGRO ingest**
  (later) calls it for `restore-verification`. Owning this here means there
  is exactly one place that knows how to open a group-level incident
  without the `is_monitored` gate.
- **Stable `(source, ref)` keys** (the table above) — operators silence /
  snooze by these via the existing `silenced_refs` mechanism; the UI / Slack
  reference them. Documenting them is part of the contract.
- **Group-scoped issue support** (Option B migration) — a reusable
  capability beyond backups (any future control-plane-level check can raise
  a group issue).

## Data shapes

No new tables are owned by *this* component except the Option-B schema
change to `issues` (nullable `server_id` or new nullable `server_group_id`)
and — for signal 3, later — `backup_restore_checks` (out of scope for the
first cut, noted for design-for). Everything else is reads.

A small internal struct for the scan, e.g.:

```rust
struct ScanRow {
    server_id: Uuid,
    group_id: Uuid,
    device_id: Option<Uuid>,        // latest-associated device, for NewEvent
    expected_interval: SignedDuration,
    config_created_at: Timestamp,
    min_first_seen: Option<Timestamp>,
    last_success_at: Option<Timestamp>,   // purpose='backup', outcome='success'
    latest_snapshot_at: Option<Timestamp>,// from backup_repo_snapshots (reconcile)
    snapshot_observed_at: Option<Timestamp>, // signal-2 freshness guard
}
```

## Testing approach (per AGENTS.md)

- **Database-level tests** (`commons_tests::db::TestDb::run`) are the
  primary coverage, since the scan/classify/reconcile logic lives in the
  `database` crate as model fns. Use direct model functions, not HTTP.
  Always `use database::ModelName;`. Seed `server_group_backup_config`,
  `servers`, `device_server_associations`, `backup_runs`,
  `backup_maintenance_runs`, `backup_repo_snapshots` directly, then assert
  on the issues/events rows produced.
- Cases to cover (success **and** the boundary/negative cases):
  - stale (success older than `×2`) fires `backup-staleness` `Error`;
  - just-under-`×2` does **not** fire;
  - never-backed-up past anchor fires `backup-never`; just-authorized group
    (recent `config.created_at`) does **not**, even with an old
    `first_seen`; freshly-present server (recent `MIN(first_seen)`) does
    **not**, even with an old `config.created_at` — assert the `max(...)`
    anchor explicitly with both orderings;
  - a recent successful **restore** does **not** clear backup staleness
    (purpose filter);
  - recovery: stale → success files `active:false`, and re-running the scan
    does not re-file (idempotence);
  - manual-only (`expected_interval` NULL) and non-`ready` configs are
    **not** scanned;
  - maintenance staleness fires/clears on `backup_maintenance_runs`;
  - reconcile: report-success-but-no-snapshot → `backup-reconcile-missing`
    (group-level, **pages even when the server is unmonitored** — assert the
    incident opens); snapshot-but-no-report → `report-gap` `Warning`
    (does not open an incident on its own);
  - reconcile **skips** the missing verdict when `snapshot_observed_at` is
    stale;
  - **group-level vs per-server gating**: a `backup-staleness` on an
    unmonitored server records the issue but opens **no** incident; a
    `backup-corruption` / `preflight-object-lock` on a group whose servers
    are all unmonitored **does** open an incident (this is the headline
    behaviour and must be tested directly against `incidents` rows).
- **Reconciliation/incident interplay**: reuse the patterns in the existing
  issues/events tests — assert `incidents` / `incident_issues` rows and the
  `slack_outbox` enqueue (`KIND_INCIDENT_OPEN`) for the paging cases, and
  that recovery enqueues the resolve.
- **Preflight** AWS calls can't hit real STS/S3 in tests; structure the
  binary so the AWS-touching functions take a trait/client object that can
  be faked, and unit-test the **decision logic** (lock-config →
  pass/fail, assume-result → which ref/severity) separately from the SDK
  wiring. The alerting side (given a verdict, the right group event is
  raised) is DB-testable via `raise_group_event`.
- Use `#[tokio::test(flavor = "multi_thread")]`. Tests run on the ramdisk
  Postgres via `just test` / `just test-package`. There's no rendered UI in
  this component, so no Playwright here (the operator stats/onboarding UI is
  a sibling component and owns its own e2e).

## Open questions / decisions to make

1. **Group-level routing (Option A vs B).** Recommend **B** (group-scoped
   issue: nullable `issues.server_id` or new `server_group_id`). It's the
   clean model and is reused by inspection (corruption) and PGRO (signal 3),
   but it's a migration + a thorough sweep of `issues.rs`. Confirm before
   building — this is the largest single decision and the rest of the
   group-level alerting depends on it.
2. **`preflight-identity` fan-out.** One fleet-level alert (needs a
   control-plane sentinel target) vs one-per-group (reuses per-group
   routing, operator sees the fan-out and infers "shared identity"). Lean
   fleet-level if Option B gives us a non-group sentinel cheaply; otherwise
   per-group.
3. **Separate `backup_staleness` binary vs folding into `reachability`.**
   Recommend separate binary (isolation, testability, matches plan
   framing). Folding saves a Deployment. Confirm with the ops/Deployment
   owner.
4. **Maintenance-staleness threshold.** Independent of `expected_interval`
   (maintenance cadence is full-weekly). Proposed constant ~`8 days`;
   confirm and make it a named constant, not magic.
5. **Reconcile severities.** `reconcile-missing` = `Error` group-level
   (pages); `report-gap` = `Warning` per-server (non-paging). Confirm the
   report-gap shouldn't be group-level — argument for per-server: a broken
   *reporting* path is a single device's telemetry problem, not a
   recoverability risk.
6. **Signal-2 freshness floor for the reconcile guard.** What
   `observed_at` age makes signal-2 "too stale to conclude missing"? Tie to
   the inspection cadence floor (weekly for manual-only). Needs the
   inspection component's cadence to be pinned first.
7. **Anchor when a server has zero `device_server_associations` rows.**
   `MIN(first_seen)` is NULL → fall back to `config.created_at` alone (the
   `max` degenerates). Confirm that's the intended behaviour (a config'd
   group with a server that no device has ever reported for: it's `never`
   once `config.created_at` + grace elapses).
8. **`CANOPY_SOURCE` sharing.** It currently lives in `statuses.rs`. Promote
   to a shared location, or re-declare in the backup module? Minor, but pick
   one to avoid drift.
9. **Signal 3 (`backup_restore_checks` + PGRO ingest)** is explicitly
   later/additive — confirm it stays out of this component's first cut
   (only the group-level routing is built now, ready for it).
