# Spec: canopy-jobs-maintenance-inspection

**Component:** `canopy-jobs-maintenance` (repo: `canopy`)
**Authoritative design:** [`../backup-credentials.md`](../backup-credentials.md) (and the blind-relay stub
[`../backup-credentials-blind-relay.md`](../backup-credentials-blind-relay.md)).
This spec implements the Canopy-owned **maintenance**, **read-only inspection**, **S3-metrics**, and
**repo-creation init** paths — the scheduler loops in the `jobs` crate and the Kubernetes Jobs they spawn.

This is the **first Kubernetes API client** and (jobs-side) the first IRSA usage anywhere in canopy. "Like
reachability" describes only the `spawn()` + `loop { sleep(60); pool.get; … }` shape — the machinery to *spawn
k8s Jobs* is entirely net-new.

---

## 1. Purpose

Canopy owns kopia repository lifecycle for every backup-configured server-group: repo creation, retention
enforcement, snapshot expiry, blob GC/compaction, ground-truth inventory, poisoning detection, and the bucket
billing-size readout. Devices never run these (they have no `DeleteObject`); the control plane does, off the
client servers, as Kubernetes Jobs running the kopia image.

This component delivers four scheduler loops in `crates/jobs/src/bin/` plus the per-group k8s Jobs they spawn:

1. **Maintenance scheduler** — per-group cycle `assert-retention → kopia snapshot expire → kopia maintenance
   run`; quick-daily / full-weekly; hash-jittered per group; writes `backup_maintenance_runs`.
2. **Inspection scheduler** — read-only `kopia snapshot list` + repo stats + repo verify (poisoning detection);
   writes `backup_repo_snapshots` and the repo-derived fields of `backup_repo_stats`.
3. **S3-metrics task** — CloudWatch `BucketSizeBytes` → `backup_repo_stats.bucket_bytes` (best-effort, separate
   permissions, separate cadence).
4. **Repo-creation init** — one-shot Job triggered by the onboarding UI: `kopia repository create` + assert
   initial retention, using the maintenance role's IRSA.

Out of scope here (other specs/components own them): the public-server device endpoints
(`/backup-credentials`, `/backup-target`, `/backup-report`), the AWS-SDK client on `public-server`'s
`AppState`, staleness detection over `backup_runs` (signal 1), the per-group upstream **preflight**, the
operator UI, and all Pulumi `backups`-stack bucket/role changes. Where this component *depends on* those, it is
called out in §6/§7. The **scheduler binaries themselves do the spawning**; the *job pod* image (kopia +
entrypoint) is described as a contract in §5 but its non-canopy build lives in ops.

---

## 2. Where it lives & the loop template

Four new binaries under `crates/jobs/src/bin/`, each following `reachability.rs` / `pingtask.rs`:

```
crates/jobs/src/bin/backup_maintenance.rs   # maintenance scheduler loop
crates/jobs/src/bin/backup_inspection.rs    # read-only inspection scheduler loop
crates/jobs/src/bin/backup_s3_metrics.rs    # CloudWatch BucketSizeBytes task
```

Repo-creation is **not** a scheduler loop — it's triggered on demand from the onboarding UI (private-server).
The Job-spawning logic is shared with the maintenance scheduler, so it lives in a shared library module the
private-server handler calls (see §3 "shared library"). It does not get its own bin.

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

Deviation from the DB-only sweeps: at startup each bin builds a **kube client** (and the maintenance/inspection
bins an AWS config provider for the metrics task only — see below). If kube-client init fails the loop must log
and **keep ticking** (so a transient API-server blip doesn't kill the pod), retrying client construction inside
the loop — mirroring how `reachability` tolerates a `None` directory rather than panicking.

Each scheduler runs as its **own single-replica `Recreate` Deployment** in `ops/pulumi/tamanu/meta/src/jobs.ts`
(see §4). The spawned per-group work is a k8s **Job** (kopia image), not part of the loop pod.

### Tick vs. cadence

The loop ticks frequently (default 60s, matching reachability) but **per-group work is gated by hash-jittered
cadence**, so a tick mostly finds nothing due. The loop's job each tick is: enumerate configured+`ready`
groups, compute each group's due-ness for *this* binary's cadence, and spawn a Job only for those due. This
keeps "is anything due" cheap (a DB read + arithmetic) and the heavy work in spawned Jobs.

---

## 3. Concrete changes (canopy)

### 3.1 New crate dependencies (`crates/jobs/Cargo.toml`)

Net-new; **do not pin versions without checking the registry** (per global rule). Needed:

- `kube` (with `runtime`/`client` features as required) and `k8s-openapi` (pinned to a Kubernetes API
  feature matching the cluster, e.g. `v1_30` — verify against the deployed control-plane version, do not
  guess).
- `aws-config` + `aws-sdk-cloudwatch` for the S3-metrics task, and `aws-sdk-sts` only if the metrics task
  must assume a cross-account role itself (see §3.5). The maintenance/inspection **schedulers** do **not**
  need the S3/STS SDK — the *Jobs* talk to S3 via their own IRSA; the scheduler only creates Jobs. Keep the
  AWS deps off the maintenance/inspection bins if the metrics task is a separate bin (it is).
- Likely `rand`/hashing already available via workspace; the hash-jitter uses a stable hash of the group UUID
  (`Sha256` is already a dependency transitively via `database`; a stable non-crypto hash is fine — just must
  be **stable across restarts**, so not `DefaultHasher` with a random seed).

These deps are jobs-crate-local. (The AWS SDK also lands on `public-server` per the endpoints spec; the kube
client also lands on `public-server` for Secret-read per `/backup-target` — both are *separate* additions owned
by the endpoints component, not duplicated here.)

### 3.2 New shared library module: `crates/jobs/src/lib.rs` + `backup/` submodules

The jobs crate is currently bin-only (no `lib.rs`). Introduce a small library so the schedulers, the init-Job
trigger (called from private-server), and tests share code without duplicating the kube/Job-template logic:

```
crates/jobs/src/lib.rs
crates/jobs/src/backup/mod.rs
crates/jobs/src/backup/jobspec.rs   # build the k8s Job manifest (image, args, labels, secretKeyRef, IRSA SA)
crates/jobs/src/backup/schedule.rs  # hash-jitter + due-ness computation
crates/jobs/src/backup/billing.rs   # ServerGroup -> billing.* pod labels
crates/jobs/src/backup/spawn.rs     # create-Job-and-record helpers (kube client wrapper)
```

> If exposing `jobs` as a library for private-server to call introduces an awkward dependency direction
> (private-server depending on the jobs crate), the alternative is to put the Job-template + spawn helpers in a
> **commons** crate (e.g. `commons-servers`) that both `jobs` and `private-server` already depend on. **Decision
> to make (§8):** library location. Default recommendation: `commons-servers::backup_jobs` so private-server's
> "create repo" button and the jobs schedulers share one code path.

`jobspec.rs` builds a `k8s_openapi::api::batch::v1::Job` with:

- `metadata.namespace` = canopy's namespace (env, like the loop's own DATABASE_URL plumbing).
- `metadata.generateName` = `canopy-backup-<kind>-<group-short>-` so each spawn is uniquely named and Job GC
  can prune by label. `kind` ∈ `{maint-quick, maint-full, inspect, init}`.
- `metadata.labels` and `spec.template.metadata.labels` carry the three **billing** pod labels plus a
  `canopy-group=<uuid>` label and `canopy-backup-kind=<kind>` for selection/cleanup.
- `spec.template.spec.serviceAccountName` = the IRSA service account appropriate to the kind:
  - maintenance + init → the **maintenance** SA (assumes the per-bucket full-access role, incl. delete).
  - inspection → the **read-only** SA (assumes the per-bucket role downscoped to read-only). The inspection
    Job's creds are **chained** (read-only restore-level) so the 1-hour cap applies — fine for a list/verify
    pass; maintenance/init use a **direct** web-identity (no chain, no cap — a full run can exceed an hour).
    See design "Credentials for the maintenance Job".
- `spec.template.spec.containers[0]`:
  - `image` = the kopia job image (see §5; ops-provided; reference by the same `image()` mechanism `spec.ts`
    uses, or a dedicated image ref — **decision §8**).
  - `args` = the kopia driver entrypoint args (subcommand + flags: bucket, region, role ARN, snapshot/expire
    flags, retention JSON, callback URL/run id — see §5 contract).
  - `env`/`secretKeyRef`: the repo password is mounted via **`secretKeyRef`** from the group's
    `repo_password_ref` Secret (never via plain env value, never logged). The bucket/region/role are env or
    args (non-secret).
  - resource requests/limits and the spot tolerations/affinity mirroring `spec.ts` defaults.
- `spec.backoffLimit` small (e.g. 2) and `spec.ttlSecondsAfterFinished` set so finished Jobs self-prune
  (avoids unbounded Job accumulation — the loop also reaps, see §3.6).
- `restartPolicy: Never` (matches the migrator/chrome-versions Jobs).

`billing.rs` computes labels from the group:

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

  For a group whose members are all unranked (`highest_member_ranks` omits them), fall back to `prod` **or**
  omit `billing.stage` — **decision §8**; recommend omit, since a wrong `prod` mis-attributes cost. (The
  `Production → "prod"` mismatch is the load-bearing gotcha; the others happen to coincide but map them all
  explicitly so a future `Display` rename can't silently break CUR tags.)

`schedule.rs` provides hash-jittered due-ness:

```rust
/// Stable per-group slot within a cadence window. Stable across restarts:
/// hashes the group UUID bytes, NOT a randomly-seeded hasher.
pub fn slot_offset(group_id: Uuid, window: Duration) -> Duration;

/// "Is this group due now for `kind`?" — combines the group's last
/// run-of-this-kind (from backup_maintenance_runs / observed_at on the
/// stats/snapshots tables) with the cadence + jitter slot.
pub fn is_due(group_id: Uuid, kind: Kind, last: Option<Timestamp>, now: Timestamp) -> bool;
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

### 3.4 Maintenance scheduler loop (`backup_maintenance.rs`)

Per tick:

1. `BackupConfig::all_ready(conn)` → candidate groups.
2. For each group, decide quick vs full and due-ness:
   - **quick**: due daily, slot-jittered by `slot_offset(group, 1 day)`; due if no successful `quick` (or
     `full`, which subsumes quick) maintenance newer than ~1 day adjusted by slot.
   - **full**: due weekly, slot-jittered by `slot_offset(group, 1 week)`.
   - If both are due, run **full** (it subsumes quick).
3. For each due group: skip if a maintenance Job for this group is already running (label selector query against
   the kube API — avoid double-spawn) **or** if an unfinished `backup_maintenance_runs` row exists for it.
4. `NewMaintenanceRun::start(...)` → `run_id`; spawn the Job with args including `run_id` and the callback URL
   so the Job reports completion (see §5 contract). The scheduler **records start**; the Job (or a watch) records
   finish.
5. Cadence defaults: quick-daily, full-weekly, deployment-wide. Per-group override is later (design non-goal);
   wire the default as constants, not magic numbers.

**Finish recording — decision §8.** Two viable mechanisms:
- (a) the Job POSTs an internal maintenance-report endpoint on completion (symmetrical with `backup-report`),
  which calls `MaintenanceRun::finish`; or
- (b) the scheduler **watches** the Jobs it spawned (`kube` watch on the `canopy-backup-kind` label) and writes
  `finish` from observed Job status (`succeeded`/`failed`) + reads `bytes_reclaimed` from a Job
  result/annotation.
Recommend (b) for maintenance/inspection (no new endpoint, the loop already holds a kube client), with a
**reconcile-on-startup** pass (like reachability's) that closes out `backup_maintenance_runs` rows whose Job is
gone — so a scheduler restart mid-run doesn't leave a row stuck `outcome = NULL` forever.

The maintenance cycle's **three steps run inside the Job** (kopia entrypoint), not the scheduler:
`assert retention → kopia snapshot expire → kopia maintenance run [--full]`. The scheduler passes the retention
JSON (from `server_group_backup_config.retention`, **floor-enforced in code**: never below `keep_daily 7,
keep_weekly 4, keep_monthly 6`; a per-group value may only raise) so the declared policy — not a drifted in-repo
one — governs. The floor-enforcement helper belongs in the shared lib (`backup::retention::enforce_floor`) so
init and maintenance use the identical clamp.

### 3.5 Inspection scheduler loop (`backup_inspection.rs`)

Per tick, same enumerate-and-gate shape, on its **own cadence** (default ≈ `expected_interval`, tunable; floor
weekly for manual-only `NULL`-interval groups that still hold backups). Spawns a **read-only** Job that:

- `kopia snapshot list` → write `backup_repo_snapshots` (latest snapshot per source; parse `server_id` from
  source).
- repo stats (`kopia repository status` / `kopia content stats`) → `backup_repo_stats` repo fields.
- **repo verify** (`kopia snapshot verify` / content hash check) → **poisoning/corruption detection**.

**Poisoning → critical group-level alert.** On detected corruption (hash mismatch / unreadable index — the
overwrite-poisoning signature), raise a **`Severity::Critical`** event (Critical satisfies `OPENS_INCIDENT`).
This is a **group-level** alert that must fire **regardless of any server's `is_monitored`** — see §3.7 for the
server-independent path; do **not** route it through a plain per-server `NewEvent` that inherits the monitored
gate.

Inspection results vs signal-1 reconciliation (report-said-success-but-no-snapshot, etc.) is **owned by the
signal-1 staleness component**, which reads `backup_repo_snapshots`/`backup_runs`. This component's job is to
*write the ground truth* and to raise the *corruption* alert; the cross-signal reconciliation alerts are the
staleness component's.

### 3.6 S3-metrics task (`backup_s3_metrics.rs`)

Separate bin so it carries **CloudWatch** permissions the read-only inspector deliberately doesn't. Per tick (own
cadence, ≈ `expected_interval`, weekly floor):

- For each `ready` group, read CloudWatch `AWS/S3 BucketSizeBytes` (`StorageType=StandardStorage` or
  `AllStorageTypes` per what the bucket reports — **verify which dimension the versioned bucket emits**, §8)
  via `GetMetricStatistics`. The metric lives in the **deployment** account → a **cross-account** CloudWatch
  read; the task either uses a dedicated least-privilege IRSA with cross-account CloudWatch, or assumes the
  group's role for the read — **implementation choice (§8)**; design allows either ("dedicated least-privilege
  IRSA role for the S3-metrics task or folded into a canopy-wide IRSA").
- `RepoStats::upsert_bucket_bytes(conn, group_id, bytes)` — best-effort; on error log + continue, never alert
  (best-effort/nullable per design).

This task does **not** spawn k8s Jobs — it reads CloudWatch directly from the scheduler pod (lightweight). It is
the one scheduler bin that needs the AWS SDK.

### 3.7 Group-level alerting path (shared concern, must be settled here)

Maintenance failure (stuck/failed maintenance) and inspection corruption are **group/control-plane** concerns
that must **not** pass the per-server `is_monitored` gate (design "Group-level checks alert regardless of
`is_monitored`"). But the incident model (`issues.rs`) is **server-keyed**: `NewEvent::save(conn, server_id,
device_id)` and `re_evaluate_incident_membership` gate on the server's `is_monitored`. There is no
"group-level issue with no server" path today.

**This is a real mechanism gap flagged in the design and it must be resolved as part of this work** (§8). Do
**not** silently route group-level alerts through a per-server `NewEvent` (they'd inherit the monitored gate and
go quiet on intentionally-intermittent prods). Options to weigh and decide with the issues-model owner:

- a **group-scoped incident** variant (incident keyed on `group_id`, bypassing `is_monitored`);
- raising the event against the group rather than a member, with a membership rule that always opens for
  control-plane `(source, ref)` pairs;
- a dedicated control-plane alert sink that still drains to the same `slacker_outbox`.

`(source, ref)` conventions for this component (mirroring reachability's `source="canopy"`):
- maintenance stuck/failed → `ref = "backup-maintenance"`, `Severity::Error` (opens incident).
- repo corruption/poisoning → `ref = "backup-corruption"`, `Severity::Critical`.
Recovery is the **same `(source, ref)`** event with `active: false` / lower severity, so the issue leaves the
incident and auto-closes (same pattern reachability uses). `slacker_outbox` drains to Slack unchanged.

(The `backup_maintenance_runs` staleness scan — "a group whose maintenance silently stopped" — can live in the
maintenance bin's loop or the signal-1 staleness component; **recommend** it lives with signal-1 so all
staleness logic is in one place, with this component only emitting the corruption alert and writing the runs
table. **Decision §8.**)

### 3.8 Repo-creation init Job

Triggered from the onboarding UI (private-server `TailscaleAdmin` handler), not on a loop. The handler calls the
shared `backup::spawn::spawn_init_job(group_id)`:

- Uses the **maintenance** IRSA SA (creating the repo format blob needs more than the no-delete device set).
- Args: `kopia repository create` against the group's bucket/region + `assert` the floor-enforced initial
  retention.
- On success the handler advances `server_group_backup_config.status` `provisioning → escrow_pending` (then the
  escrow UI flips `→ ready`). Status transitions are owned by the onboarding component; this component provides
  the *Job spawn + completion signal*.

---

## 4. IaC changes (ops — `ops/pulumi/tamanu/meta`)

Owned jointly with the ops/IaC spec; the canopy-jobs-relevant pieces:

- **Three new single-replica `Recreate` Deployments** in `jobs.ts` mirroring `reachability`/`pingtask`:
  `backup-maintenance` (`['backup_maintenance']`), `backup-inspection` (`['backup_inspection']`),
  `backup-s3-metrics` (`['backup_s3_metrics']`), each `dependsOn: [migrator]`, with `costLabels`.
- **ServiceAccount + IRSA, net-new to canopy.** `spec.ts` injects no `serviceAccountName` today. Add an optional
  `serviceAccountName` to the `spec()` container args (or a sibling helper) and create the SAs via the existing
  **`common/eksServiceAccount.ts`** helper (currently unused by canopy). Needed:
  - a **scheduler SA** for the loop pods with **RBAC to create/list/watch/delete Jobs** in canopy's namespace
    and **`get` Secrets** (to template `secretKeyRef`); plus IRSA only if the metrics task assumes via the pod
    SA.
  - the **Job pods'** IRSA roles — maintenance/init SA (assumes per-bucket full-access role, direct
    web-identity), inspection SA (read-only). These trust the per-bucket roles cross-account; the per-bucket
    role trust + reduced action set + `s3:GetBucketObjectLockConfiguration` are **`backups`-stack** changes
    owned by the ops spec.
  - **OIDC-provider-per-account** wiring so the Job's web-identity can assume cross-account (ops/IaC).
- **The kopia Job image** must be published and referenceable (ops). See §5.
- RBAC for create-Jobs and get-Secrets must be a `Role`/`RoleBinding` in the namespace (least privilege — not
  cluster-wide).

This component's canopy code must read the namespace + SA names from **env/config** (like DATABASE_URL), not
hardcode them, so the same binary works across stacks.

---

## 5. Interfaces / contracts

### Consumes

- **DB config:** `server_group_backup_config` (read): `group_id`, `bucket`, `prefix`, `target_role_arn`,
  `region`, `expected_interval`, `retention` (JSONB), `repo_password_ref`, `status`. Only `status = 'ready'`
  groups are scheduled.
- **`server_groups`:** `ServerGroup::highest_member_ranks`, `rank_priority`, `tags` (`TagMap`) for billing
  labels.
- **kopia repo password Secret** named by `repo_password_ref`, in canopy's namespace — mounted into Jobs via
  `secretKeyRef`. Owned by the repo-password/onboarding component; consumed here read-only.
- **Per-bucket IAM roles** (full-access maintenance role; read-only for inspection), trusting the Job IRSA SAs
  cross-account. Owned by the ops `backups`-stack spec.
- **`issues::NewEvent::save`** + the (to-be-built) group-level alert path. `Severity` from
  `commons_types::issue` (`OPENS_INCIDENT = [Critical, Error]`).
- **kube API** + **CloudWatch** (metrics task).

### Provides

- **DB writes** other components read:
  - `backup_maintenance_runs` (start/finish; consumed by signal-1 staleness + the stats UI panel).
  - `backup_repo_snapshots` (ground-truth inventory; consumed by signal-1/2 reconciliation + UI).
  - `backup_repo_stats` repo fields + `bucket_bytes` (consumed by the operator stats panel).
- **Shared library** (`commons-servers::backup_jobs` per §3.2 decision): `spawn_init_job(group_id)`,
  `spawn_maintenance_job(...)`, `spawn_inspection_job(...)`, billing-label and retention-floor helpers — called
  by **private-server** (init Job from the onboarding button) and the schedulers.
- **Group-level alerts** `(source="canopy", ref ∈ {backup-maintenance, backup-corruption})` feeding the
  existing incident → Slack pipeline.

### Contract with the kopia Job image (ops-built; canopy passes args, image honours them)

The scheduler builds the Job; the **image entrypoint** must accept (stable contract — agree with ops/bestool):

- **All kinds:** bucket, prefix, region, target role ARN (assume via IRSA/web-identity), repo password (via
  `secretKeyRef` env), repo source-host convention `canopy@<server-id>` for any source addressing.
- **maint-quick / maint-full:** retention JSON to assert; run `assert-retention → snapshot expire →
  maintenance run [--full]`; emit `bytes_reclaimed` if kopia surfaces it (annotation/exit JSON).
- **inspect:** read-only; emit snapshot inventory (source + latest snapshot ts) and repo stats (snapshot/source
  counts, logical/physical bytes) and a verify result; **how results return to canopy** (Job watch reading an
  annotation/result file, vs the Job POSTing an internal endpoint) is the §3.4(a/b) decision.
- **init:** `kopia repository create` + assert initial retention; set the repo maintenance **owner** to the
  Canopy maintenance identity and **disable client-side maintenance/expiry** (so devices never attempt
  delete-needing ops).

If results come back via Job watch, the **completion contract** is: Job `succeeded`/`failed` Pod phase +
optional result annotation; if via endpoint, an internal `(run_id, outcome, error, bytes/counts)` POST.

---

## 6. Data shapes (Rust)

```rust
// crates/jobs/src/backup/mod.rs (or commons-servers::backup_jobs)
pub enum JobKind { MaintQuick, MaintFull, Inspect, Init }

pub struct BillingLabels {
    pub product: String,            // default "tamanu"
    pub deployment: String,         // default = group name
    pub stage: Option<String>,      // None => omit label (all-unranked group)
}

pub struct RetentionPolicy {        // mirrors server_group_backup_config.retention JSONB
    pub keep_latest: u32,           // default 1, not floored
    pub keep_daily: u32,            // floor 7
    pub keep_weekly: u32,           // floor 4
    pub keep_monthly: u32,          // floor 6
    pub keep_annual: u32,           // default 0
}
impl RetentionPolicy { pub fn enforce_floor(self) -> Self; }
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
- **Pure-logic unit tests** (in the jobs lib, plain `#[test]` / `#[tokio::test(flavor = "multi_thread")]`):
  - `slot_offset` is **stable** for a fixed UUID across calls (regression guard against a randomly-seeded
    hasher) and spreads across the window for distinct UUIDs.
  - `is_due` boundaries (just-before / just-after the window, full subsumes quick).
  - **billing label mapping** — especially `Production → "prod"` (the gotcha) and all-unranked → `None`.
  - `JobSpec` builder: asserts the manifest carries the billing labels, the `canopy-group`/`canopy-backup-kind`
    labels, `restartPolicy: Never`, the correct SA per kind, `ttlSecondsAfterFinished`, and the password as
    `secretKeyRef` (**never** a plain-value env / never present in args) — a snapshot/structured assertion on
    the serialized `Job`. This is the cheap, high-value test since spawning real Jobs in CI isn't feasible.
- **Kube interaction:** do **not** stand up a real cluster in tests. Keep the kube client behind a thin trait
  (`JobSpawner`) so the schedulers' due-ness/spawn-decision logic is testable with a fake spawner that records
  what would be created. Test "already-running group is skipped" and "due group spawns exactly one Job" against
  the fake.
- **Alerting:** assert the corruption path builds a `NewEvent` with `Severity::Critical` and `ref =
  "backup-corruption"`, and that recovery emits the matching `active: false` event. Since the group-level path
  is the open mechanism (§3.7), test at the `NewEvent` construction boundary until that path lands, then extend.
- **No e2e/Playwright here** — this component has no rendered UI (the onboarding/stats UI is a separate
  component and carries its own Playwright per AGENTS.md). The init-Job *trigger* is exercised by the
  onboarding component's tests against the fake `JobSpawner`.
- Run per-package while iterating: `just test-package jobs` and `just test-package database`; let CI run the
  full suite (no final local full-suite run, per memory). `just check` for compile/warnings.

---

## 8. Open questions / decisions to make

1. **Shared-library location** — `crates/jobs` as a lib vs `commons-servers::backup_jobs` so private-server can
   call the init-Job spawn without depending on the `jobs` crate. *Recommend `commons-servers`.*
2. **Maintenance/inspection completion signal** — Job-watch (kube) vs an internal maintenance-report endpoint.
   *Recommend watch + startup reconcile to close orphaned `outcome = NULL` rows.*
3. **Group-level alert mechanism** — the incident model is server-keyed with an `is_monitored` gate; group-level
   maintenance/corruption alerts must bypass it. Needs a server-independent path (group-scoped incident, or a
   membership rule for control-plane `(source, ref)`). **Must be resolved with the issues-model owner; do not
   ship a per-server `NewEvent` workaround.**
4. **Where the maintenance-staleness scan lives** — this bin vs the signal-1 staleness component. *Recommend
   signal-1 owns all staleness; this component writes `backup_maintenance_runs` + emits only the corruption
   alert.*
5. **kopia Job image reference** — how canopy names/pins the image (shared `image()` vs a dedicated ref); the
   entrypoint arg contract (§5) must be agreed with ops/bestool.
6. **S3-metrics cross-account read** — dedicated least-privilege CloudWatch IRSA vs assume-the-group-role; and
   the correct `BucketSizeBytes` `StorageType` dimension for a versioned bucket (verify empirically).
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

- **Kubernetes API client** (`kube` + `k8s-openapi`) on the jobs schedulers — to create/list/watch/delete Jobs.
- **ServiceAccount + IRSA** plumbed through `spec.ts` for the scheduler pods (create-Jobs + get-Secrets RBAC)
  and for the Job pods (maintenance/inspection/init IRSA roles assuming per-bucket roles cross-account, direct
  web-identity for maintenance/init, chained read-only for inspection).
- **AWS SDK** (`aws-config` + `aws-sdk-cloudwatch`, maybe `aws-sdk-sts`) on the S3-metrics bin only.
- **The kopia Job image** + its entrypoint arg contract (ops-built).
- **OIDC-provider-per-account** wiring for cross-account Job web-identity (ops/IaC).

These are the same net-new pieces the design's "Repo-alignment outcomes" section enumerates; this component owns
the **jobs-side** kube client + Job-spawning IRSA, while `public-server`'s kube/AWS additions are owned by the
endpoints component.
