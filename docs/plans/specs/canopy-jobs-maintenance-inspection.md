# Spec: canopy-jobs-maintenance-inspection

**Component:** `canopy-jobs-maintenance` (repo: `canopy`)
**Authoritative design:** [`../backup-credentials.md`](../backup-credentials.md) (and the blind-relay stub
[`../backup-credentials-blind-relay.md`](../backup-credentials-blind-relay.md)).
This spec implements the Canopy-owned **maintenance**, **read-only inspection**, **S3-metrics**, and
**repo-creation init** paths — the scheduler loops in the `jobs` crate that drive kopia **in-process**.

UPDATE (shipped) — the architecture changed fundamentally from the original
"spawn one-shot k8s Jobs that report back" design to **a single long-lived
`backups` Deployment that runs kopia as an in-process subprocess** for each due
group. There are **no Kubernetes Jobs** anywhere: the loops parse kopia's
`--json` output and write results **inline** to the DB. Throughout this spec,
where older text describes Job manifests, a `JobSpawner`/`jobspec`, Job reaping,
a `/job-report` endpoint, a pod termination-message, or a separate `kopia-job`
image/binary, it is **superseded** — see the inline "UPDATE (shipped)" notes and
§5/§8. The reasons: every Job already shared the one `canopy-jobs` IRSA
identity, so collapsing into one process loses no isolation; and a long-lived
process can hold a **refreshing** per-group credential, fixing the 1-hour cap
that one-shot static creds hit.

This is (jobs-side) the first IRSA usage anywhere in canopy, and the Kubernetes
API client is now used only for **Secret reads** (the repo passphrase), not Job
create/watch. "Like reachability" describes only the `spawn()` +
`loop { sleep(60); pool.get; … }` shape.

---

## 1. Purpose

Canopy owns kopia repository lifecycle for every backup-configured server-group: repo creation, retention
enforcement, snapshot expiry, blob GC/compaction, ground-truth inventory, poisoning detection, and the bucket
billing-size readout. Devices never run these (they have no `DeleteObject`); the control plane does, off the
client servers. UPDATE (shipped): it runs them **in-process** in the long-lived
`backups` Deployment (kopia is a bundled subprocess), **not** as one-shot
Kubernetes Jobs.

RESOLVED (impl) — **the four scheduler loops ship as ONE bin**,
`crates/jobs/src/bin/backups.rs`, which runs four modules
(`crates/jobs/src/backup/{maintenance,inspection,preflight,s3_metrics}.rs`)
concurrently via `tokio::try_join!`. (Preflight, originally specced in the
sibling detection-preflight doc, is one of those modules.) The text below
still describes the work as four loops for clarity of each loop's job; read
"four scheduler loops" as "four modules in the one `backups` bin" throughout,
and §2/§4 are updated accordingly.

This component delivers (as four modules of the single `backups` bin). Each due
group's kopia work runs as an **in-process subprocess** of the same pod (UPDATE
(shipped) — no per-group k8s Jobs):

1. **Maintenance scheduler** — per-group cycle `assert-retention → kopia snapshot expire → kopia maintenance
   run`; quick-daily / full-weekly; hash-jittered per group; writes `backup_maintenance_runs` inline.
2. **Inspection scheduler** — read-only `kopia snapshot list` + repo stats + repo verify (poisoning detection);
   writes `backup_repo_snapshots` and the repo-derived fields of `backup_repo_stats` inline.
3. **S3-metrics task** — CloudWatch `BucketSizeBytes` → `backup_repo_stats.bucket_bytes` (best-effort, separate
   permissions, separate cadence).
4. **Repo-creation init** — driven by the maintenance loop (not a Job): for each `provisioning` group it runs
   `kopia repository create` + asserts initial retention in-process, using the group's per-bucket role.

Out of scope here (other specs/components own them): the public-server device endpoints
(`/backup-credentials`, `/backup-target`, `/backup-report`), the AWS-SDK client on `public-server`'s
`AppState`, staleness detection over `backup_runs` (signal 1), the per-group upstream **preflight**, the
operator UI, and all Pulumi `backups`-stack bucket/role changes. Where this component *depends on* those, it is
called out in §6/§7. UPDATE (shipped): kopia is a **bundled subprocess** of the
`backups` bin. The kopia binary is copied into the single shipped
`ghcr.io/beyondessential/canopy` image (`.github/Dockerfile.native`), so the
backups pod runs that same image — there is no separate job-pod image and no
inter-process contract — §5 now documents how the bin invokes the kopia CLI
directly.

---

## 2. Where it lives & the loop template

RESOLVED (impl) — **one** bin, not four, following `reachability.rs` /
`pingtask.rs` for the outer `spawn()`/`main()` shape but driving four loop
modules concurrently:

```
crates/jobs/src/bin/backups.rs                  # the single long-lived bin
crates/jobs/src/backup.rs                        # backup module root
crates/jobs/src/backup/maintenance.rs           # maintenance scheduler loop (+ drives init)
crates/jobs/src/backup/inspection.rs            # read-only inspection scheduler loop
crates/jobs/src/backup/preflight.rs             # upstream preflight (see detection-preflight spec)
crates/jobs/src/backup/s3_metrics.rs            # CloudWatch BucketSizeBytes task
crates/jobs/src/backup/kopia.rs                  # in-process kopia execution layer (subprocess wrappers + parsing)
crates/jobs/src/backup/worker.rs                 # shared Worker: pool, kube client (Secret reads), concurrency, in-flight set
crates/jobs/src/backup/complete.rs               # inline DB writes from a kopia op's typed outcome
```

UPDATE (shipped) — the bin's `main()` builds a shared
[`Worker`](#) (DB pool, `kube::Client` for Secret reads, concurrency semaphore,
in-flight group set) **once**, then launches the four loops under one
`tokio::try_join!`. Maintenance and inspection share the `Worker`; preflight and
s3-metrics build their own pool/AWS clients. The kopia ops for each due group run
**in-process** as subprocesses (the kopia CLI is bundled in the image), parse
kopia's `--json`, and write results inline via `complete.rs`. (Original sketch was
three separate `backup_*` bins each as its own Deployment, each *spawning k8s
Jobs* — both superseded: one bin, in-process kopia.)

Repo-creation is **not** its own loop and **not** handler-driven — UPDATE
(shipped): the **maintenance loop** runs `init` in-process for `provisioning`
groups (gated by the same in-flight set), then advances the status inline (see
§3.8). The onboarding handler only sets `status = 'provisioning'`; private-server
holds no kube/jobs dependency.

Each bin keeps the established structure verbatim:

```rust
pub fn spawn() -> JoinHandle<()> {
    let pool = database::init();
    task::spawn(async move {
        // build the kube client + scheduler config ONCE at startup
        // (like reachability builds the TailnetDirectory once)
        loop {
            sleep(Duration::from_secs(TICK)).await;
            let Ok(mut db) = pool.get().await else { error!(...); continue; };
            // … per-tick work …
        }
    })
}

#[derive(Debug, Parser)]
struct Args { #[command(flatten)] logging: LoggingArgs }

#[tokio::main]
async fn main() -> miette::Result<()> { /* identical to reachability.rs main() */ }
```

Deviation from the DB-only sweeps: at startup the bin builds a **kube client** (used only for repo-password
**Secret reads**, not Jobs) and the s3-metrics/preflight tasks build AWS clients. UPDATE (shipped): the kube
client is built once in `main()` (a hard failure there exits the pod); a transient API blip when *reading a
Secret* mid-loop is per-group and just skips that group's op for the tick, so it doesn't kill the pod.

RESOLVED (impl): the four loops share **one** single-replica `Recreate`
Deployment (`backups`) in `ops/pulumi/tamanu/meta/src/jobs.ts` (see §4). UPDATE
(shipped): the per-group work runs **in-process** (kopia subprocess) inside the
loop pod — it is **not** a k8s Job.

### Tick vs. cadence

The loop ticks frequently (default 60s, matching reachability) but **per-group work is gated by hash-jittered
cadence**, so a tick mostly finds nothing due. The loop's job each tick is: enumerate configured+`ready`
groups, compute each group's due-ness for *this* loop's cadence, and — for those due and not already in-flight —
**claim a per-group + concurrency slot and run the kopia op in-process**. This keeps "is anything due" cheap (a
DB read + arithmetic) and the heavy work in bounded in-process subprocesses.

---

## 3. Concrete changes (canopy)

### 3.1 New crate dependencies (`crates/jobs/Cargo.toml`)

Net-new; **do not pin versions without checking the registry** (per global rule). UPDATE (shipped) — the
shipped `crates/jobs/Cargo.toml` carries:

- `kube` and `k8s-openapi` — used **only for Secret reads** (the per-group repo passphrase, via
  `worker::read_repo_password`). UPDATE (shipped): **no** Job create/list/watch/delete — there are no Jobs.
  `k8s-openapi` is pinned to a feature matching the cluster (verify against the deployed control-plane version,
  do not guess).
- `aws-config` + `aws-sdk-cloudwatch` + `aws-sdk-s3` + `aws-sdk-sts` for **preflight** (upstream reachability)
  and the **S3-metrics** task. The maintenance/inspection loops do **not** use the AWS SDK directly — kopia's
  own bundled AWS SDK talks to S3 (the bin only overrides `AWS_ROLE_ARN` per subprocess).
- **No `axum`** — UPDATE (shipped): there is no `/job-report` HTTP server; results are typed Rust values
  written inline.
- Hash-jitter uses a stable hash of the group UUID (stable across restarts — not a randomly-seeded hasher);
  the helpers live in `commons_servers::backup_jobs`.

(The AWS SDK also lands on `public-server` per the endpoints spec; the kube client also lands on `public-server`
for Secret-read per `/backup-target` — both are *separate* additions owned by the endpoints component.)

### 3.2 In-process execution model: `kopia.rs` + `worker.rs` + `complete.rs`

UPDATE (shipped) — there is **no** k8s-Job manifest builder (`jobspec.rs`), no `JobSpawner`, and no
`spawn_*_job` helpers. The kopia work runs in-process. The code splits three ways:

```
crates/jobs/src/backup/kopia.rs      # subprocess wrappers + parsing + per-kind orchestration (run_init/run_maintenance/run_inspect)
crates/jobs/src/backup/worker.rs     # shared Worker: pool, kube client (Secret reads), Slots (semaphore + in-flight set)
crates/jobs/src/backup/complete.rs   # inline DB writes from a kopia op's typed outcome
```

Pure scheduler logic (hash-jitter, due-ness, billing labels, retention floor, `JobKind`) still lives in
**`commons_servers::backup_jobs`**, reused by the loops and by private-server (so private-server need not depend
on the `jobs` crate). UPDATE (shipped): private-server does not call any spawn helper — init is scheduler-driven
(§3.8).

**`kopia.rs`** (in-process execution layer):

- Builds a per-op `KopiaEnv { target_role_arn, region, password }` and applies it to each `tokio::process::Command`:
  it sets `AWS_ROLE_ARN` = the group's `target_role_arn` (overriding the pod's shared `canopy-jobs` IRSA role),
  `AWS_REGION`/`AWS_DEFAULT_REGION` = the group's region, and `KOPIA_PASSWORD` = the repo passphrase. The
  projected `AWS_WEB_IDENTITY_TOKEN_FILE` is **inherited** from the pod env, so kopia's own AWS SDK does
  `AssumeRoleWithWebIdentity` against the per-bucket role **directly** (not chained → up to the role's
  `MaxSessionDuration`, auto-refreshed — no 1h cap). This replaces the per-kind IRSA-SA distinction: every op
  uses the same pod SA and overrides the role per subprocess.
- `connect(...)` always connects with `--override-username canopy --override-hostname canopy-maintenance` so the
  running identity is the maintenance owner (kopia 0.23.1 requires running identity == owner for
  `maintenance run`; see §5). `run_init` sets that identity as the owner.
- `run_init` / `run_maintenance` / `run_inspect` orchestrate the kopia subcommands and return **typed Rust
  outcomes** (e.g. `MaintOutcome { bytes_reclaimed }`, `InspectOutcome { verify_ok, counts, per-source
  inventory }`) — no JSON-over-HTTP, no termination-log.
- The repo password is **never** logged; it is read from the group's k8s Secret and passed only via the
  subprocess env.

**`worker.rs`** (concurrency + Secret reads):

- `Worker { pool, kube, cfg, slots }` is built once in `main()` and shared (cheaply cloned) by maintenance +
  inspection. `Cfg::from_env()` reads `CANOPY_NAMESPACE`, the Secret password key, and the web-identity token
  file path — so one binary works across stacks.
- `read_repo_password(secret_name)` reads the named key from the group's k8s Secret (the only kube API use).
- `Slots` holds a tokio `Semaphore` (max concurrency from `CANOPY_BACKUP_MAX_CONCURRENCY`, default 4) and an
  in-flight `HashSet<Uuid>`. `try_claim(group_id)` takes a permit then marks the group in-flight, returning an
  `InFlightGuard` that releases both on drop — enforcing **one op per group at a time** across maintenance +
  inspection + init, plus a global concurrency cap.

**`complete.rs`** (inline completion):

- Called inline with the typed outcome: `complete_maint` closes the `backup_maintenance_runs` row (success →
  `bytes_reclaimed`; failure → error), `complete_init` advances `provisioning → escrow_pending`/`ready` or
  records `last_init_error`, and the inspection path upserts inventory/stats and raises/recovers the corruption
  alert off `verify_ok`. There is no report endpoint and no crash-detection: the op runs in the same process, so
  its outcome is known directly.

**Billing labels** (`commons_servers::backup_jobs`) are still computed from the group (for the Deployment's cost
labels, not per-Job pods):

- `billing.product` = group's `billing.product` tag if present else `"tamanu"`.
- `billing.deployment` = group's `billing.deployment` tag if present else the group **name**.
- `billing.stage` = group's `billing.stage` tag if present, else derived from
  `ServerGroup::highest_member_ranks` → `rank_priority`, mapped **explicitly** to the CUR stage strings ops
  already emits — **not** the `ServerRank` `Display` strings, which don't match:

  | `ServerRank` | `Display` | billing stage |
  |---|---|---|
  | `Production` | `production` | `prod` |
  | `Clone` | `clone` | `clone` |
  | `Demo` | `demo` | `demo` |
  | `Test` | `test` | `test` |
  | `Dev` | `dev` | `dev` |

  RESOLVED (impl): `billing.stage` maps explicitly, with `ServerRank::Production
  → "prod"` (the load-bearing mismatch); the others coincide but are mapped
  explicitly so a future `Display` rename can't silently break CUR tags. (See §8
  for the all-unranked fallback.)

`commons_servers::backup_jobs` provides hash-jittered due-ness (UPDATE (shipped) — these live in
`backup_jobs`, not a `schedule.rs`):

```rust
/// Cadence elapsed since the last run-of-this-kind (window arithmetic only).
pub fn is_due(window: Duration, last: Option<Timestamp>, now: Timestamp) -> bool;

/// Stable per-group jitter slot: true only on the tick matching this group's
/// hashed offset within the window. Stable across restarts (hashes the group
/// UUID, NOT a randomly-seeded hasher).
pub fn slot_is_due(group_id: Uuid, window: Duration, tick: Duration, secs_into_window: u64) -> bool;
```

### 3.3 Database changes

The **tables** are defined in the design doc and shared with sibling components; this component **reads** config
and **writes** run/inventory/stats rows. Migrations are created with **`just migration NAME`** (never
hand-authored — per project rule). To avoid two specs both trying to own the same migration, ownership is:

- `server_group_backup_config`, `backup_credential_issuances`, `backup_runs`, `backup_requests` — owned by the
  **endpoints/onboarding** components (this component only **reads** `server_group_backup_config`).
- **This component owns the migrations for** `backup_maintenance_runs`, `backup_repo_snapshots`,
  `backup_repo_stats` (DDL verbatim from the design doc §"Database changes"). If a single migration is
  preferred for the whole feature, coordinate so this component contributes these three tables.

New database-crate model modules (mirroring `chrome_releases.rs` shape: a `Queryable` struct + a `New*` insert
struct + impl methods, re-exported from `lib.rs`):

- `crates/database/src/backup_maintenance_runs.rs` — `MaintenanceRun` / `NewMaintenanceRun`.
  - `NewMaintenanceRun::start(conn, group_id, kind) -> id` (insert with `outcome = NULL`, returns `BIGSERIAL`).
  - `MaintenanceRun::finish(conn, id, outcome, error, bytes_reclaimed)`.
  - `MaintenanceRun::latest_for_group(conn, group_id, kind) -> Option<MaintenanceRun>` (for due-ness +
    staleness).
- `crates/database/src/backup_repo_snapshots.rs` — `RepoSnapshot` / `NewRepoSnapshot`.
  - `NewRepoSnapshot::upsert_many(conn, group_id, rows)` (PK `(group_id, source)`, `ON CONFLICT … DO UPDATE`
    `latest_snapshot_at`/`observed_at`).
  - parse `server_id` from the kopia `source` (`canopy@<server-id>:<path>`) at write time.
- `crates/database/src/backup_repo_stats.rs` — `RepoStats`.
  - `RepoStats::upsert_repo_fields(conn, group_id, snapshot_count, source_count, logical, physical)` — written
    by inspection.
  - `RepoStats::upsert_bucket_bytes(conn, group_id, bytes)` — written by the S3-metrics task; must **not**
    clobber the repo fields (partial upsert), since the two tasks run on different cadences. `bucket_bytes` is
    nullable/best-effort.

Use PostgreSQL-native upserts (`ON CONFLICT`) per project DB conventions; keep the per-task partial-update
separation so the two writers don't race over each other's columns.

`backup_repo_config` reader: add `server_group_backup_config` model (likely owned by the config/onboarding
component) — this component needs a read like `BackupConfig::all_ready(conn) -> Vec<BackupConfig>` (status =
`'ready'`, used to enumerate groups to schedule) and `BackupConfig::by_group(conn, group_id)`. If that model
doesn't exist yet, this component adds the read-only accessors it needs.

### 3.4 Maintenance scheduler loop (`backup/maintenance.rs`)

Per tick (UPDATE (shipped) — no Jobs; the op is an in-process subprocess task):

1. `ServerGroupBackupConfig::all(...)` → candidate groups; `provisioning` ones go through init (§3.8),
   `ready` ones through maintenance.
2. For each `ready` group, decide quick vs full and due-ness (`due_kind`):
   - **full**: due weekly (`is_due(WEEK, last_full, now)`) **and** this tick matches the group's hashed slot
     (`slot_is_due(group, WEEK, TICK, …)`).
   - **quick**: due daily, slot-jittered over the day; `full` subsumes quick.
   - If both are due, run **full**.
3. For each due group: `Worker::try_claim(group_id)` — skip if the group is already in-flight (across
   maintenance/inspection/init) or the concurrency cap is hit. No kube API query.
4. `NewMaintenanceRun::start(...)` → `run_id`; read the repo password from the group's Secret, build the
   `KopiaEnv`, **spawn a tokio task** that runs `kopia::run_maintenance(...)` in-process and then calls
   `complete::complete_maint(run_id, …)` inline with the typed outcome. The `InFlightGuard` releases the slot on
   task completion.
5. Cadence defaults: quick-daily, full-weekly, deployment-wide (`TICK`/`DAY`/`WEEK` constants). Per-group override
   is later (design non-goal).

**Finish recording — UPDATE (shipped): inline, in-process.** The op runs as a
subprocess of the same pod, so the loop knows its outcome directly: it calls
`complete::complete_maint(run_id, outcome, error)` → `MaintenanceRun::finish`.
There is **no** `/job-report` endpoint, no axum server, and no kube poll for
reaping/crash-detection. (Superseded design: first a report-endpoint, then a pod
termination-message / pod-log read, then a bearer-authed `/job-report` POST with
a kube reap/crash-detect poll — see §8 #2. The termination-message/pod-log read
proved unreliable in practice — k8s truncates/drops the message and the pod may
be gone before it's read, PGRO's pattern too — and the report round-trip became
unnecessary once kopia runs in-process, since the IRSA was shared anyway.) A
crash now can't leave a row stuck at `outcome IS NULL` via a missing report: if
the in-process op panics or errors, the loop's task records the failure inline.

The maintenance cycle's **three steps run in-process** (`kopia.rs::run_maintenance`), not in a separate Job:
`assert retention → kopia snapshot expire → kopia maintenance run [--full]`, all under the group's per-bucket
role. RESOLVED (impl) — **per-`(group, type)` retention is resolved and applied per source.** The loop calls
`commons_servers::backup_jobs::effective_retention_for_group` (per enabled type: `server_group_backup_schedule`
override → `backup_type_defaults` → floor baseline, each `.enforce_floor()`-clamped — never below `keep_daily 7,
keep_weekly 4, keep_monthly 6`) and builds a `{type → policy}` **map** (`kopia::RetentionMap`). The kopia layer
applies it **per source**: for each `canopy@<server-id>:<type>` source it sets that type's kopia policy
(`policy set <user@host:path>`), then expires — so different types sharing a group's repo get their own
retention. (`init` sets a strictest-of-the-map global baseline since the repo has no sources yet.) The
private-server write path validates operator input against the same floor via the DB-crate
`RetentionPolicy::validate_floor()` (rejects below-floor rather than clamping).

### 3.5 Inspection scheduler loop (`backup/inspection.rs`)

Per tick, same enumerate-and-gate shape, on its **own cadence** (default ≈ `expected_interval`, tunable; floor
weekly for manual-only `NULL`-interval groups that still hold backups). RESOLVED (impl): the per-group cadence
is `commons_servers::backup_jobs::effective_interval_for_group` (the **min** effective `expected_interval`
across the group's enabled types), floored to weekly. UPDATE (shipped): for each due group it claims a slot and
**runs `kopia::run_inspect(...)` in-process** (no read-only Job), which:

- `kopia snapshot list --all --json` → per-source inventory (latest snapshot per source).
- repo stats (`kopia content stats` — note: **no `--json`**, parsed from text) → snapshot/source counts +
  logical/physical bytes.
- **repo verify** → a `verify_ok` flag (poisoning/corruption signal).

UPDATE (shipped) — **inspection completion runs inline**, in-process. There is no
`/job-report` POST and no kube reap poll. The loop calls `complete.rs` directly
with the typed `InspectOutcome`, which:

- writes `backup_repo_snapshots` (latest snapshot per source; `server_id`/`type`
  parsed from each source) and the repo-derived fields of `backup_repo_stats`.
- on `verify_ok: false`, raises the **`backup-corruption`** `Severity::Critical`
  **group-level** alert (via `raise_group_event`, §3.7), with a matching
  `active: false` recovery when a later inspection verifies clean.

This is a **group-level** alert that fires **regardless of any server's
`is_monitored`** — routed through `raise_group_event` (§3.7), not a per-server
`NewEvent`.

Inspection results vs signal-1 reconciliation (report-said-success-but-no-snapshot, etc.) is **owned by the
signal-1 staleness component**, which reads `backup_repo_snapshots`/`backup_runs`. This component's job is to
*write the ground truth* and to raise the *corruption* alert; the cross-signal reconciliation alerts are the
staleness component's.

### 3.6 S3-metrics task (`backup/s3_metrics.rs`)

A loop module in the one `backups` bin (UPDATE (shipped) — not a separate bin); it builds its own AWS clients.
Per tick (own cadence, ≈ `expected_interval`, weekly floor):

- For each `ready` group, read CloudWatch `AWS/S3 BucketSizeBytes`. It is reported **per `StorageType`**
  (storage class) with no "all storage types" total, and the class depends on bucket config (Standard,
  Intelligent-Tiering tiers, …), so the task **`ListMetrics`-discovers** whichever `StorageType`s the bucket
  actually emits and **sums** the latest `GetMetricStatistics` datapoint across them — no hardcoded class.
  RESOLVED (impl): the metric lives in the **deployment** account, so the task **assumes the group's
  `target_role_arn`** (the same role preflight assumes) and reads CloudWatch with those cross-account
  credentials — no dedicated canopy-side cross-account CloudWatch IRSA. The per-bucket role must grant
  `cloudwatch:GetMetricStatistics` **and `cloudwatch:ListMetrics`** (ops `backups`-stack).
- `RepoStats::upsert_bucket_bytes(conn, group_id, bytes)` — best-effort; on error log + continue, never alert
  (best-effort/nullable per design).

This task reads CloudWatch directly from the `backups` pod (lightweight); it never ran as a Job.

### 3.7 Group-level alerting path (shared concern, must be settled here)

Maintenance failure (stuck/failed maintenance) and inspection corruption are **group/control-plane** concerns
that must **not** pass the per-server `is_monitored` gate (design "Group-level checks alert regardless of
`is_monitored`"). But the incident model (`issues.rs`) is **server-keyed**: `NewEvent::save(conn, server_id,
device_id)` and `re_evaluate_incident_membership` gate on the server's `is_monitored`. There is no
"group-level issue with no server" path today.

RESOLVED (impl): the gap was closed with a **group-scoped issue** path. The
shipped entrypoint is **`database::backup::alerts::raise_group_event(conn,
group_id, ref, severity, …)`**, which find-or-creates a group-scoped issue
(nullable `issues.server_id`, group resolved directly — migration
`2026-06-15-...backup_group_scoped_issues`), runs the group-aware membership
evaluation that **bypasses `is_monitored`**, and drains to `slacker_outbox`
unchanged. Both this component (corruption) and the detection component call it;
its tests cover the all-members-unmonitored paging case. Do **not** route
group-level alerts through a per-server `NewEvent`. (See the detection-preflight
spec for the full `raise_group_event` contract and the migration sweep.)

`(source, ref)` conventions for this component (mirroring reachability's `source="canopy"`):
- maintenance stuck/failed → `ref = "backup-maintenance"`, `Severity::Error` (opens incident).
- repo corruption/poisoning → `ref = "backup-corruption"`, `Severity::Critical`.
Recovery is the **same `(source, ref)`** event with `active: false` / lower severity, so the issue leaves the
incident and auto-closes (same pattern reachability uses). `slacker_outbox` drains to Slack unchanged.

(The `backup_maintenance_runs` staleness scan — "a group whose maintenance silently stopped" — can live in the
maintenance bin's loop or the signal-1 staleness component; **recommend** it lives with signal-1 so all
staleness logic is in one place, with this component only emitting the corruption alert and writing the runs
table. **Decision §8.**)

### 3.8 Repo-creation init

RESOLVED (impl) — **init is scheduler-driven, not handler-driven** (cleaner: no
kube/Jobs dependency in private-server). UPDATE (shipped): it runs **in-process**,
not as a Job. The onboarding handler only sets `status = 'provisioning'`; it does
**not** spawn anything. The **maintenance loop** then:

- enumerates groups in `provisioning`, guarded by `last_init_error IS NULL`
  (cleared by the operator-UI retry) **and** not-already-in-flight,
- runs `kopia::run_init(...)` in-process under the group's per-bucket role
  (creating the repo format blob needs the full-access role, not the device's
  no-delete set): `kopia repository create` (CONFIRMED kopia 0.23.1: exits
  non-zero if the repo already exists → falls back to `connect` and treats that
  as success), connects with the fixed `canopy@canopy-maintenance` identity and
  sets it as the maintenance **owner**, and asserts the floor-enforced initial
  retention,
- on completion, `complete::complete_init(...)` advances the status inline:
  `provisioning → escrow_pending` for **FromBirth** mode (Canopy-minted
  passphrase → escrow flow) / `provisioning → ready` for **Import** mode
  (operator already holds the passphrase), **or** records `last_init_error` on
  failure (operator-UI clears it to retry).

(So both the original "the onboarding handler calls `spawn_init_job`" *and* the
intermediate "scheduler spawns an init Job that POSTs `/job-report`" are
superseded: private-server holds no kube/jobs dependency, and the maintenance
loop runs init in-process and advances the status directly.)

---

## 4. IaC changes (ops — `ops/pulumi/tamanu/meta`)

Owned jointly with the ops/IaC spec; the canopy-jobs-relevant pieces:

- RESOLVED (impl): **one** new single-replica `Recreate` Deployment in `jobs.ts`
  mirroring `reachability`/`pingtask` — `backups` (`['backups']`), running all
  four loop modules — `dependsOn: [migrator]`, with `costLabels`. (Originally
  specced as three separate `backup-maintenance`/`backup-inspection`/`backup-s3-metrics`
  Deployments — superseded.)
- **ServiceAccount + IRSA, net-new to canopy.** UPDATE (shipped) — there is **one** SA, the `canopy-jobs` SA on
  the single `backups` Deployment; **no per-Job SAs**, no per-kind maintenance/inspection SA split (every kopia
  subprocess overrides `AWS_ROLE_ARN` to the group's per-bucket role and reuses the pod's projected web-identity
  token). `spec.ts` injects no `serviceAccountName` today, so add an optional `serviceAccountName` to the
  `spec()` container args (or a sibling helper) and create the SA via the existing
  **`common/eksServiceAccount.ts`** helper. The SA needs:
  - **k8s RBAC: `get` Secrets** in canopy's namespace — to read the per-group repo passwords — **and that's it**.
    UPDATE (shipped): **NO** create/list/watch/delete Jobs, **no** pods, **no** tokenreviews.
  - **AWS/IRSA:** assume the per-bucket roles via **web-identity** (direct, refreshing — up to each role's
    `MaxSessionDuration`; set it high enough to cover a long maintenance run). The per-bucket role must **trust
    the `canopy-jobs` SA's OIDC subject** and (for s3-metrics) grant `cloudwatch:GetMetricStatistics`. The
    per-bucket role trust + action set + `s3:GetBucketObjectLockConfiguration` are **`backups`-stack** changes
    owned by the ops spec.
  - **OIDC-provider-per-account** wiring so the pod's web-identity can assume cross-account (ops/IaC).
- **The shipped `ghcr.io/beyondessential/canopy` image** bundles kopia (the kopia binary is copied into
  `.github/Dockerfile.native` from `kopia/kopia:0.23.1`), so the backups pod runs the same image as the other
  components — there is no separate kopia-job image, and there is **no `CANOPY_BACKUP_IMAGE` env** (no Job image
  to reference). See §5.
- UPDATE (shipped): **no** report Service/Secret (`CANOPY_BACKUP_REPORT_*` / bearer token) — there is no
  `/job-report` endpoint. The k8s RBAC is a least-privilege namespace `Role`/`RoleBinding` granting only
  `get secrets`.

This component's canopy code reads the namespace (and Secret password key / web-identity token file) from
**env/config** (like DATABASE_URL), not hardcoded, so the same binary works across stacks.

---

## 5. Interfaces / contracts

### Consumes

- **DB config:** `server_group_backup_config` (read): `group_id`, `bucket`, `prefix`, `target_role_arn`,
  `region`, `repo_password_ref`, `status`, `mode`, `last_init_error`. Schedule/retention are read from
  `server_group_backup_schedule` / `backup_type_defaults` (the addendum moved `expected_interval`/`retention`
  off the config table). `status = 'ready'` groups are scheduled for maintenance/inspection; `provisioning`
  groups drive the init flow (§3.8).
- **`server_groups`:** `ServerGroup::highest_member_ranks`, `rank_priority`, `tags` (`TagMap`) for billing
  labels.
- **kopia repo password Secret** named by `repo_password_ref`, in canopy's namespace — UPDATE (shipped): **read
  via the kube API** (`worker::read_repo_password`) and passed to the kopia subprocess as `KOPIA_PASSWORD`, not
  mounted via `secretKeyRef`. Owned by the repo-password/onboarding component; consumed here read-only.
- **Per-bucket IAM roles** (`target_role_arn`) trusting the **`canopy-jobs` SA** OIDC subject cross-account; the
  kopia subprocess assumes them directly via web-identity. Owned by the ops `backups`-stack spec. UPDATE
  (shipped): one role per group (no separate full-access vs read-only role per kind).
- **`database::backup::alerts::raise_group_event`** (group-level alert path, §3.7). `Severity` from
  `commons_types::issue` (`OPENS_INCIDENT = [Critical, Error]`).
- **kube API** (Secret reads only) + **CloudWatch** (s3-metrics) + **S3/STS** (preflight, and CloudWatch
  cross-account assume).

### Provides

- **DB writes** other components read:
  - `backup_maintenance_runs` (start/finish; consumed by signal-1 staleness + the stats UI panel).
  - `backup_repo_snapshots` (ground-truth inventory; consumed by signal-1/2 reconciliation + UI).
  - `backup_repo_stats` repo fields + `bucket_bytes` (consumed by the operator stats panel).
- **Shared library** (`commons_servers::backup_jobs`): pure scheduler helpers — `JobKind`, billing labels,
  `RetentionPolicy`/floor (`effective_retention_for_group`), `effective_interval_for_group`,
  `is_due`/`slot_is_due` — shared by the loops and by private-server's validation. UPDATE (shipped): **no**
  `spawn_*_job` helpers (init is scheduler-driven, in-process).
- **Group-level alerts** `(source="canopy", ref ∈ {backup-maintenance, backup-corruption})` feeding the
  existing incident → Slack pipeline.

### kopia invocation (in-process subprocess — no inter-process contract)

UPDATE (shipped) — there is **no inter-process contract** anymore. The earlier
designs (config-via-ENV/args + results-via-POST-to-`/job-report`, and the
separate `images/kopia-job/` image with its `CONTRACT.md`) are **superseded**:
the `backups` bin invokes the **bundled** kopia CLI directly (`tokio::process`)
in `kopia.rs`, parses its `--json`/text output into typed Rust values, and writes
the results inline. No ENV-config handoff, no result JSON over HTTP, no
`terminationMessagePolicy`, no `kopia-job` image.

Per op the bin builds a `KopiaEnv` (`AWS_ROLE_ARN` = group's `target_role_arn`,
`AWS_REGION`, `KOPIA_PASSWORD` from the Secret; projected web-identity token
inherited) and runs:

- **maint-quick / maint-full:** `connect` → per source set that type's policy
  (`policy set <user@host:path> --keep-*`, from the `{type → policy}` map) →
  `snapshot expire` → `maintenance run [--full]`. (`init` sets a
  strictest-of-map global baseline since there are no sources yet.)
- **inspect:** `snapshot list --all --json` → repo stats (`content stats`) → verify.
- **init:** `repository create` + assert initial retention; connect as the canopy
  identity and set it as the maintenance **owner**, **disabling** client-side
  maintenance/expiry (so devices never attempt delete-needing ops).

**Verified kopia 0.23.1 facts** (confirmed against the bundled version, encoded in `kopia.rs`):

- **Connect identity / maintenance owner:** `maintenance run` refuses unless the connected client identity
  equals the maintenance owner. So every op connects with `--override-username canopy --override-hostname
  canopy-maintenance` (constants `MAINTENANCE_USER`/`MAINTENANCE_HOST`), and `init` sets that identity as the
  owner. Devices connect with their own identity, so they never become owner.
- **Per-source policy:** retention is applied per source via `kopia policy set <user@host:path> --keep-*`
  (per-type, keyed by `canopy@<server-id>:<type>`).
- **`kopia content stats` has no `--json`** — physical-bytes are parsed from its text output (a "Total Bytes:"
  line), best-effort (`None` if unparseable).
- **`kopia repository create` exits non-zero if the repo already exists** — `run_init` treats that as success by
  falling back to `connect`.
- **`kopia snapshot list --all --json`** elements carry `source = { userName, host, path }`, parsed into the
  per-source inventory (`server_id`/`type` from `host`/`path`).

Typed outcomes (written inline by `complete.rs`, no wire schema):

- **init:** ok / error → status advance or `last_init_error`.
- **maint:** `MaintOutcome { bytes_reclaimed }`.
- **inspect:** `InspectOutcome { verify_ok, snapshot/source counts, logical/physical bytes, per-source
  inventory }`.

---

## 6. Data shapes (Rust)

```rust
// commons_servers::backup_jobs (kept as a *kind* enum even though there are no Jobs)
pub enum JobKind { MaintQuick, MaintFull, Inspect, Init }

pub struct BillingLabels {
    pub product: String,            // default "tamanu"
    pub deployment: String,         // default = group name
    pub stage: Option<String>,      // None => omit label (all-unranked group)
}

// RESOLVED (impl): RetentionPolicy lives in the DATABASE crate
// (database::backups::RetentionPolicy), over the schedule/type-default JSONB.
pub struct RetentionPolicy {
    pub keep_latest: i32,           // default 1, not floored
    pub keep_daily: i32,            // floor 7  (FLOOR_DAILY)
    pub keep_weekly: i32,           // floor 4  (FLOOR_WEEKLY)
    pub keep_monthly: i32,          // floor 6  (FLOOR_MONTHLY)
    pub keep_annual: i32,           // default 0
}
impl RetentionPolicy {
    // validates (does NOT silently clamp): below-floor → AppError::BadRequest
    pub fn validate_floor(&self) -> Result<()>;
    pub fn from_json(&JsonValue) -> Option<Self>;
    pub fn to_json(&self) -> JsonValue;
}
```

```rust
// database models
pub struct MaintenanceRun { pub id: i64, pub group_id: Uuid, pub kind: String,
    pub started_at: Timestamp, pub finished_at: Option<Timestamp>,
    pub outcome: Option<String>, pub error: Option<String>, pub bytes_reclaimed: Option<i64> }

pub struct RepoSnapshot { pub group_id: Uuid, pub source: String,
    pub server_id: Option<Uuid>, pub latest_snapshot_at: Option<Timestamp>, pub observed_at: Timestamp }

pub struct RepoStats { pub group_id: Uuid, pub snapshot_count: Option<i32>, pub source_count: Option<i32>,
    pub logical_bytes: Option<i64>, pub physical_bytes: Option<i64>,
    pub bucket_bytes: Option<i64>, pub observed_at: Timestamp }
```

Kopia source parse: `canopy@<server-id>:<path>` → `server_id = Uuid::parse(...)` (best-effort; `None` if the
host segment isn't a UUID, e.g. legacy/imported repos — store the row with `server_id = NULL` rather than
dropping it).

---

## 7. Testing approach (per AGENTS.md)

- **DB model tests** with `commons_tests::db::TestDb::run(|mut conn, _url| async move { … })`, calling model
  functions directly (not HTTP), per the project rule. Cover:
  - `NewMaintenanceRun::start` then `MaintenanceRun::finish` (success + failure rows), `latest_for_group`.
  - `RepoSnapshot::upsert_many` idempotency on `(group_id, source)`; `server_id` parse from a real
    `canopy@<uuid>:/path` source and from a non-UUID host (→ `NULL`).
  - `RepoStats` **partial upserts**: `upsert_repo_fields` then `upsert_bucket_bytes` must not clobber each
    other (the two-writer split is the load-bearing invariant — test it explicitly).
  - retention **floor enforcement**: a policy below floor is raised; an above-floor override is preserved;
    `keep_latest` is **not** floored.
  - 404/absent cases (`latest_for_group` for an unknown group → `None`).
  - Always `use database::ModelName;` imports.
- **Pure-logic unit tests** (plain `#[test]` / `#[tokio::test(flavor = "multi_thread")]`):
  - `slot_is_due`/`slot_offset` is **stable** for a fixed UUID across calls (regression guard against a
    randomly-seeded hasher) and spreads across the window for distinct UUIDs.
  - `is_due` boundaries (just-before / just-after the window, full subsumes quick).
  - **billing label mapping** — especially `Production → "prod"` (the gotcha) and all-unranked → `None`.
  - UPDATE (shipped) — instead of a Job-manifest test: **`kopia.rs` parsing/policy helpers** (retention `--keep-*`
    flag building, `snapshot list --json` source parsing, `content stats` text parsing) and **`worker::Slots`**
    concurrency (one-op-per-group exclusion + semaphore cap) — all unit-testable without a cluster.
- **Kube/kopia interaction:** do **not** stand up a real cluster or invoke real kopia in tests. UPDATE (shipped):
  there is no `JobSpawner` trait — concurrency/due-ness is tested via `Slots` + the `commons_servers::backup_jobs`
  helpers; the kopia subprocess itself is not exercised in CI.
- **Alerting:** assert the corruption path raises the group-level event (`Severity::Critical`, `ref =
  "backup-corruption"`) via `raise_group_event` and that recovery emits the matching `active: false` event (the
  group-scoped-issue path is shipped, §3.7).
- **No e2e/Playwright here** — this component has no rendered UI (the onboarding/stats UI is a separate
  component and carries its own Playwright per AGENTS.md). The init flow is exercised via the
  `complete_init` status-advance path.
- Run per-package while iterating: `just test-package jobs` and `just test-package database`; let CI run the
  full suite (no final local full-suite run, per memory). `just check` for compile/warnings.

---

## 8. Open questions / decisions to make

1. **Shared-library location** — RESOLVED: **`commons_servers::backup_jobs`**.
   (And, as shipped, init is scheduler-driven so private-server doesn't call a
   spawn helper at all — §3.8.)
2. **Maintenance/inspection completion signal** — RESOLVED (impl): **in-process.**
   kopia runs as a subprocess of the `backups` pod and its typed outcome is
   written inline (`complete.rs`); there is no completion *signal* to receive.
   This supersedes both earlier ideas: (a) the kube-watch / pod-termination-message
   / pod-log read (reverted — k8s truncates/drops the message and the pod may be
   gone before it's read, unreliable in practice), and (b) the bearer-authed
   `/job-report` POST + kube reap/crash-detect poll (dropped — the report
   round-trip became unnecessary once kopia runs in-process, since the IRSA is
   shared anyway). No `/job-report` endpoint, no axum, no kube poll.
3. **Group-level alert mechanism** — RESOLVED: **group-scoped issue** via
   `database::backup::alerts::raise_group_event` (bypasses `is_monitored`),
   backed by the `backup_group_scoped_issues` migration (nullable
   `issues.server_id`). No per-server `NewEvent` workaround.
4. **Where the maintenance-staleness scan lives** — RESOLVED: the
   staleness/reconcile sweep lives with the detection slice
   (`database::backup::sweep`, run by the `monitor` bin); this component writes
   `backup_maintenance_runs` and raises only the corruption alert.
5. **kopia image** — RESOLVED (impl): there is no separate kopia-job image and no entrypoint contract. kopia is
   **bundled** into the single shipped `ghcr.io/beyondessential/canopy` image (the kopia binary is copied into
   `.github/Dockerfile.native` from `kopia/kopia:0.23.1`) and invoked in-process; no `CANOPY_BACKUP_IMAGE`
   env. (§5.)
6. **S3-metrics cross-account read** — RESOLVED (impl): assume the group's `target_role_arn` and read CloudWatch
   with those creds (no dedicated canopy CloudWatch IRSA); per-bucket role grants `cloudwatch:GetMetricStatistics`
   + `cloudwatch:ListMetrics`. `BucketSizeBytes` is summed across the bucket's actual `StorageType`s
   (ListMetrics-discovered — handles Standard / Intelligent-Tiering), not a hardcoded class.
7. **All-unranked group billing stage** — fall back to `prod` vs omit `billing.stage`. *Recommend omit.*
8. **k8s-openapi API version** — pin to the cluster's actual control-plane version (verify; don't guess), per
   the no-guessing-versions rule. Same for `kube`/AWS-SDK crate versions (check the registry before pinning).
9. **Migration ownership** — confirm with the config/endpoints components whether the feature ships one
   migration or several; this component contributes `backup_maintenance_runs` / `backup_repo_snapshots` /
   `backup_repo_stats` either way.
10. **Cadence/tick defaults** — quick-daily / full-weekly / inspection ≈ `expected_interval` (weekly floor) /
    metrics ≈ `expected_interval` (weekly floor); confirmed-tunable per design but the constants live in code.

---

## 9. Net-new infrastructure summary (none exists in canopy today)

UPDATE (shipped) — the shape changed from "k8s Jobs + per-kind IRSA SAs + a separate kopia image" to "one
long-lived Deployment that runs bundled kopia in-process":

- **Kubernetes API client** (`kube` + `k8s-openapi`) on the `backups` bin — used **only to `get` Secrets** (repo
  passwords). No Job create/list/watch/delete.
- **ServiceAccount + IRSA** plumbed through `spec.ts` for the single `backups` pod: **one** `canopy-jobs` SA with
  k8s RBAC = `get secrets` and an IRSA role; each kopia subprocess overrides `AWS_ROLE_ARN` to the group's
  per-bucket role and assumes it **directly via web-identity** (refreshing, up to the role's `MaxSessionDuration`).
  No per-Job/per-kind SAs, no chained read-only path.
- **AWS SDK** (`aws-config` + `aws-sdk-cloudwatch` + `aws-sdk-s3` + `aws-sdk-sts`) on the `backups` bin for
  **preflight** + **s3-metrics**. kopia's own bundled AWS SDK handles the S3 repo I/O.
- **The shipped `ghcr.io/beyondessential/canopy` image** bundles kopia (its binary is copied into
  `.github/Dockerfile.native` from `kopia/kopia:0.23.1`). No separate kopia-job image, no ENV-config/POST
  contract.
- **OIDC-provider-per-account** wiring for cross-account web-identity (ops/IaC).

This component owns the **jobs-side** kube (Secret-read) client + the `canopy-jobs` IRSA; `public-server`'s
kube/AWS additions are owned by the endpoints component.

---

## Backup types addendum

Per the plan's "Backup types":

- **Retention is per-`(group, type)`.** The maintenance cycle's
  assert-retention step asserts *each type's* effective keep-policy
  (`server_group_backup_schedule.retention ?? backup_type_defaults`, org
  floor applied) as a kopia **per-source/path policy**, so `kopia snapshot
  expire` honours the right policy per type. The maintenance *run* itself
  stays per-group (one repo per group, shared by all types).
- **Scheduling is per-`(group, type)`** — the maintenance/inspection
  schedulers iterate active `(group, type)` (or per-group for the
  repo-wide maintenance run; per-type for retention assertion).
- **Inspection** parses the snapshot's `canopy-type` tag → writes
  `backup_repo_snapshots.type`; `(server, type)` is one source.
- `backup_repo_stats` stays per-group (repo is shared; size is repo-level).
