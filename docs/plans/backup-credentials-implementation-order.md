# Backup-credentials — cross-repo implementation order

Direction / ordering doc for building the backup-credentials system across the
four repos (`canopy`, `ops/pulumi`, `bestool`, `pgro`). It does **not** restate
the design — read [`backup-credentials.md`](./backup-credentials.md) for that,
and each component spec for the how. This document answers one question: **in
what order, and on which tracks, do we build it so nothing waits on something
that isn't there yet.**

## The eight component specs

| # | Component | Repo | Spec |
|---|-----------|------|------|
| 1 | canopy-database (tables, models, migrations) | canopy | [specs/canopy-database.md](./specs/canopy-database.md) |
| 2 | canopy-public-server (device endpoints + AWS/kube on AppState) | canopy | [specs/canopy-public-server.md](./specs/canopy-public-server.md) |
| 3 | canopy-jobs-maintenance-inspection (maintenance/inspection/S3-metrics/init Jobs) | canopy | [specs/canopy-jobs-maintenance-inspection.md](./specs/canopy-jobs-maintenance-inspection.md) |
| 4 | canopy-jobs-detection-preflight (staleness, reconciliation, group-level alerting, preflight) | canopy | [specs/canopy-jobs-detection-preflight.md](./specs/canopy-jobs-detection-preflight.md) |
| 5 | canopy-operator-ui (private-server fns + private-web) | canopy | [specs/canopy-operator-ui.md](./specs/canopy-operator-ui.md) |
| 6 | ops (per-bucket roles, IRSA/ServiceAccounts, OIDC, scheduler Deployments) | ops/pulumi | [../../../ops/pulumi/docs/canopy-backup-credentials.md](../../../ops/pulumi/docs/canopy-backup-credentials.md) |
| 7 | bestool (device `backup-credentials` / `backup` subcommands) | bestool | [../../../bestool/docs/canopy-backup-credentials.md](../../../bestool/docs/canopy-backup-credentials.md) |
| 8 | pgro (restore consumer + signal-3 restore-verification) | pgro | [../../../pgro/docs/canopy-backup-integration.md](../../../pgro/docs/canopy-backup-integration.md) |

---

## Dependency graph (derived from each spec's provides / depends_on)

Arrows mean "depends on / must exist or be stubbed first".

```
                         ┌─────────────────────────────────────────────┐
                         │  SPIKE: kopia vs GOVERNANCE-default-retention │
                         │  bucket, no client-side PutObjectRetention    │
                         │  (gates ops A2 action-set + bestool kopia)    │
                         └───────────────┬─────────────────────────────-┘
                                         │ (verifies an assumption; doesn't block code start)
                                         ▼
   (1) canopy-database  ◄──────────────────────────── everything in canopy reads/writes these tables
        │  tables, models, lib.rs re-exports, commons-types enums,
        │  Option-B group-scoped-issues migration handled with (4)
        │
        ├──────────────┬───────────────────────┬──────────────────────┐
        ▼              ▼                       ▼                      ▼
   (2) public-server  (3) jobs-maint/insp   (4) jobs-detect/preflight  (5) operator-ui
   AWS SDK + kube      kube client + Job-     group-level alerting       private-server fns
   on AppState;        spawn lib; init Job;   (Option-B issues);         + private-web; reads
   /backup-* endpoints maintenance/inspection staleness + reconcile;     status/stats; reveal
        │              /S3-metrics schedulers preflight (AWS)            escrow (needs kube)
        │                    │                      │
        │   contracts: HTTP endpoint shapes, IRSA role ARNs / ServiceAccount subs, billing labels
        ▼                    ▼                      ▼
   (6) ops  ◄────────────────┴──────────────────────┘   provides per-bucket role ARNs, IRSA roles,
        │   OIDC providers, scheduler Deployments; consumes canopy SA names + OIDC issuer URL
        │
   (7) bestool ◄── public-server endpoints (2) + the kopia spike
        │
   (8) pgro    ◄── restore endpoint + external-restore grant + first-party auth (canopy, later)
                   + a non-chained / longer-lived restore-cred decision
```

Two cross-cutting net-new capabilities sit underneath most of canopy and are
the real gate (see Critical path):

- **AWS SDK + kube client** — first use anywhere in canopy. Lands on
  `public-server` (component 2) and the `jobs` crate (components 3/4).
- **ServiceAccount + IRSA + OIDC** — first ServiceAccount canopy has ever had.
  Owned by ops (component 6), consumed by 2/3/4.

The canopy↔ops boundary is **mutually dependent** and resolved by contract, not
by serialising: canopy publishes the SA names + central-cluster OIDC issuer
URL; ops publishes the per-bucket role ARNs + IRSA role ARNs. Each side codes
against the agreed names and the two meet at deploy.

---

## The early blocker: kopia-behaviour verification spike

> **Concluded (from kopia docs/source + S3 semantics) — Branch A:** device
> creds = `AWS_S3_MULTIPART_ACTIONS` (no `PutObjectRetention`/no delete);
> repo created **non-lock-aware**; rely on the bucket's default GOVERNANCE
> retention + versioning + lifecycle. `--session-token`,
> `--override-hostname`, and `--point-in-time` are all supported. Two items
> still want a **live confirm** (the no-`PutObjectRetention` write path, and
> PIT on real AWS S3 per issue #4346). Full verdict + test script:
> [`backup-credentials-kopia-spike.md`](./backup-credentials-kopia-spike.md).

**Do this first, in parallel with stage 0, before committing the ops action-set
and the bestool kopia wiring.** It's cheap, it's a known unknown, and it
changes two specs if it comes out the wrong way.

The question (from ops spec A2 / bestool open-Q 2 / canopy-database H3): does
kopia **write and maintain** against an S3 bucket with **GOVERNANCE 30-day
default Object-Lock retention** when the client has **no `s3:PutObjectRetention`**
(the device action set is `AWS_S3_MULTIPART_ACTIONS`, delete- and
retention-free)? Also confirm:

- kopia's S3 backend honours `AWS_SESSION_TOKEN` temporary creds (pgro open-Q 2;
  bestool credential_process path).
- `--override-hostname` exists on the installed kopia for source-host = server-id
  (bestool open-Q 3).
- which `BucketSizeBytes` `StorageType` dimension a versioned+locked bucket emits
  (jobs-maint open-Q 6) — needed by the S3-metrics task, lower-stakes, can trail.

Outcome drives:
- **ops A2**: device role is exactly `AWS_S3_MULTIPART_ACTIONS`, or that **plus**
  `s3:PutObjectRetention` (safe under GOVERNANCE-without-bypass — can only
  lengthen a lock). Don't finalise the managed policy until this is known.
- **bestool kopia helpers**: connect/snapshot wiring and how creds reach kopia.

Run it against a throwaway dev bucket the ops `backups` stack can stand up. If it
comes back "kopia insists on PutObjectRetention", the fallback is already
specified — re-grant it — so this never blocks, it just picks a branch. Start
the spike at day 0; it must conclude before ops merges the action-set change and
before bestool finalises the kopia connect path.

---

## Critical path

The longest chain of hard dependencies, and what unblocks the most downstream
work, is:

1. **Net-new enabling work** (the gate for all of canopy):
   - **(1) canopy-database** — tables, models, `lib.rs` re-exports, and the
     shared `commons-types` enums (`Purpose`/`Outcome`/`kind`). Nothing in
     canopy compiles against these until they exist. This is the true
     foundation; land it first.
   - **AWS SDK + kube client deps + AppState wiring** (inside component 2) and
     the **kube client + Job-spawn library** (inside component 3). First AWS/k8s
     code in the repo; verify crate versions against the registry (no guessing),
     pin `k8s-openapi` to the cluster's control-plane version.
   - **(6) ops IRSA/ServiceAccount/OIDC + per-bucket role ARNs** — without the
     ServiceAccount + IRSA trust, no canopy pod can `AssumeRole`, and without the
     role ARNs there's no `target_role_arn` to put in config. This runs in
     parallel with the canopy enabling work, joined by the ARN/SA-name contract.

2. **The issuance hot path**: (1) → (2) public-server `/backup-credentials` +
   `/backup-target` + `/backup-report`, against (6)'s role ARNs and Secret-read
   RBAC. This is the contract bestool consumes.

3. **bestool (7)** — needs (2)'s endpoint shapes live (or contract-frozen) and
   the kopia spike concluded.

4. **pgro (8)** — additive last stage; needs the restore endpoint, the
   external-restore grant, first-party auth, and a longer-lived-cred decision,
   none of which exist until canopy ships its restore surface.

The single highest-leverage item is **canopy-database (1)**: components 2, 3, 4,
and 5 all import its models. Land it, with the `commons-types` enums, before the
four canopy tracks fan out. The second is the **ops IRSA/OIDC plumbing (6)**,
because it's the longest-lead infra item and gates every AWS-touching code path
at deploy time even though the code can be written against the contract earlier.

---

## Build order (stages)

Stages are sequencing guidance, not hard gates — within a stage, tracks run in
parallel. A later stage starts when its named dependencies from the earlier
stage are merged (or contract-frozen and stubbed).

### Stage 0 — foundations (must land first)

- **Spike**: kopia-behaviour verification (above). Parallel, concludes before
  ops action-set + bestool kopia.
- **(1) canopy-database**: the `backup_credentials` migration (all 7 tables),
  `backups.rs` models + `lib.rs` re-exports, the `commons-types`
  `Purpose`/`Outcome`/`kind` enums. Resolve its open decisions up front because
  they ripple: enum representation (shared enums vs validated String), the
  `backup_runs` client-supplied-PK → `AppError::Conflict` mapping, cascade
  policy for stats/requests vs the no-cascade audit rule, and the
  `backup_repo_snapshots.server_id` on-delete behaviour. DB-only tests via
  `TestDb::run`.
- **(6) ops — contract freeze + long-lead infra**: agree the names/ARNs both
  sides code against (`canopyIssuerRoleArn`, `canopyJobsRoleArn`,
  `canopy-issuer`/`canopy-jobs` ServiceAccount subs, central-cluster OIDC issuer
  URL, per-bucket `deviceRoleArn`/`maintenanceRoleArn`, the
  `billing.{product,stage,deployment}` label keys). Then start the actual
  Pulumi: Component B (central ServiceAccounts + IRSA + RBAC, `spec.ts`
  `serviceAccountName`), Component A1/A3/A4/A6 (per-bucket trust, Object-Lock
  read action, ARN exports, lifecycle rules), Component C (OIDC provider per
  deployment account). A2 (action-set reduction) waits on the spike.

The canopy↔ops contract is the coordination spine for everything after.

### Stage 1 — issuance hot path + enabling clients (the device-facing MVP)

Depends on Stage 0's (1) and the (6) contract.

- **(2) canopy-public-server**: add the AWS SDK + kube deps, the
  `AppState.sts` / `AppState.kube` fields + `FromRef` impls + async init,
  `AppError::Upstream` (502) + ERRORS.md, and the three handlers
  (`/backup-credentials`, `/backup-target`, `/backup-report`) with the restore
  session-policy builder. This component **owns** the AWS/kube-on-AppState
  capability the rest of canopy reuses. Tests: the 412/409/502 resolution
  matrix with `None` clients, the session-policy unit test, a stubbed-STS 200
  path.

This is the first end-to-end slice: a device can mint creds and report a run.

### Stage 2 — control-plane jobs (parallel canopy tracks)

All depend on Stage 0 (1) and reuse the AWS/kube patterns from Stage 1 (2).
These three run in parallel with each other and with bestool.

- **(3) canopy-jobs-maintenance-inspection**: the shared Job-spawn library
  (recommended `commons-servers::backup_jobs` so private-server can call the
  init-Job spawn without depending on the `jobs` crate), the three scheduler
  bins (maintenance / inspection / S3-metrics), the kopia-Job arg contract, and
  the migrations it owns (`backup_maintenance_runs`, `backup_repo_snapshots`,
  `backup_repo_stats`) — coordinate with (1) on single-vs-split migration
  ownership.
- **(4) canopy-jobs-detection-preflight**: the **Option-B group-scoped-issues
  migration** + the thorough `issues.rs` sweep (this is the largest single
  decision in the system and the central new shared plumbing —
  `raise_group_event` is consumed by the inspection Job in (3) and PGRO ingest
  in (8)), the `backup_staleness` and `backup_preflight` bins, and the shared
  `jitter_slot` helper. Resolve Option A vs B before building; recommend B.
- **(6) ops — scheduler Deployments**: B4 wires the
  `backup-maintenance`/`backup-inspection`/`backup-preflight` (and possibly
  `backup-s3-metrics`/`backup-staleness`) single-replica Deployments on the
  `canopy-jobs` SA, once the bin names are pinned by (3)/(4).

Cross-track coordination inside Stage 2:
- (3) and (4) **share** `commons-servers` helpers (`jitter_slot`,
  retention-floor) and the `(source, ref)` alert keys — agree these once.
- The group-level alerting path from (4) is a **prerequisite** for (3)'s
  corruption alert and (4)'s own group-level refs — (4) should land the Option-B
  plumbing early in the stage so (3) can call `raise_group_event`.

### Stage 3 — operator UI + device client (parallel)

- **(5) canopy-operator-ui**: private-server `/api/backups/*` fns + the React
  screens. Depends on (1) models, reuses (2)'s kube client for `reveal_escrow`
  (resolve open-Q: private-server gets its own `canopy-issuer` SA + Secret-read
  RBAC — coordinate with ops open-Q 2), and depends on (3)'s init-Job contract
  for the `provisioning → escrow_pending/ready` lifecycle. `just gen-openapi` +
  Playwright e2e in the same change.
- **(7) bestool**: the two subcommands + `CanopyClient` methods, against (2)'s
  frozen endpoint shapes and the concluded kopia spike. The "back up now"
  command-channel transport is **deferred upstream** — build the
  transport-independent subcommands now; wire the trigger when canopy defines
  the status-response payload.

(5) and (7) are independent and parallel. (7) can start as soon as (2)'s
endpoint contract is frozen, even before (3)/(4) land.

### Stage 4 — PGRO (additive, last)

- **(8) pgro**: restore-consumer CRD (`canopyBackup.group`), `fetch_restore_creds`
  / `report_restore`, signal-3 `RestoreReport` into a future
  `backup_restore_checks` table + the `restore-verification` group-level alert
  (routed through (4)'s `raise_group_event`).

PGRO is explicitly last because it needs canopy-side surfaces that don't exist
until the earlier stages ship:
- a **restore-credentials** path (purpose=restore creds + target + repo
  password) — built on (2);
- the **external-restore grant** (operator-authorized, audited "consumer pgro
  may read group X read-only") — net-new canopy authz surface;
- a **first-party non-device auth** path (Tailscale now, OIDC later) — joint
  canopy+ops design;
- the **`backup_restore_checks` table + ingest endpoint + signal-3 detection**;
- a **decision on longer-lived / non-chained restore creds** so restores >1h
  survive (mirror the maintenance-Job direct web-identity). **This decision is
  owed by canopy and should be made during Stage 2** (when the maintenance-Job
  direct-web-identity path is built) so PGRO isn't blocked on it in Stage 4.

---

## Parallelizable tracks (one per repo)

Once Stage 0's (1) + (6)-contract land, the repos proceed largely in parallel,
coordinated only by the contracts named below.

- **canopy track**: (1) → then (2), (3), (4), (5) fan out. (2) blocks (7) (HTTP
  contract). (4)'s Option-B plumbing blocks (3)'s corruption alert. (5) needs
  (3)'s init-Job contract and (2)'s kube client.
- **ops track**: (6) runs alongside the canopy enabling work, joined by the
  ARN/SA-name contract; its scheduler-Deployment piece (B4) trails (3)/(4)'s bin
  names.
- **bestool track**: (7) starts when (2)'s endpoint shapes are frozen and the
  spike is done; otherwise independent of (3)/(4)/(5).
- **pgro track**: (8) is last; nothing else depends on it.

### The contracts that let the tracks run independently

1. **HTTP endpoint shapes** (canopy public-server ⇆ bestool, and later ⇆ pgro):
   `POST /backup-credentials`, `GET /backup-target`, `POST /backup-report` —
   request/response bodies, the 412/409/502 semantics, and `backup_runs.id` =
   client-minted UUID PK with `device_id`/`group_id` server-derived. Freeze
   these from spec (2)/(7) before bestool starts; bestool's `canopy_contract.rs`
   `#[ignore]`d suite is the drift detector.
2. **IRSA role ARNs + ServiceAccount subs + OIDC issuer** (canopy ⇆ ops):
   `target_role_arn` (= ops `deviceRoleArn`), `maintenanceRoleArn`,
   `canopyIssuerRoleArn`, `canopyJobsRoleArn`, the central-cluster OIDC issuer
   URL, and the `canopy-issuer`/`canopy-jobs` SA names in namespace
   `tamanu-meta-<stack>`. The hard isolation invariant: the maintenance/
   fullaccess role MUST NOT trust the issuer principal.
3. **Billing label keys** (canopy ⇆ ops): `billing.{product,stage,deployment}`,
   with `ServerRank::Production → "prod"` (the load-bearing mapping gotcha).
4. **Shared `commons-types` enums** (canopy-internal, spec 1): `Purpose` /
   `Outcome` / `kind` shared across public-server, jobs, and the generated
   `api-types.ts`, so the three components don't drift.
5. **`raise_group_event` group-level alert entrypoint** (spec 4, consumed by 3
   and 8): the single place that opens a group-scoped incident bypassing
   `is_monitored`.
6. **Init-Job lifecycle contract** (spec 3 ⇆ spec 5): UI sets
   `status='provisioning'` + clears `last_init_error`; the init Job transitions
   to `escrow_pending`/`ready` or sets `last_init_error`. UI depends only on the
   observable fields, not the handoff mechanism.
7. **kopia Job image + entrypoint arg contract** (ops-built image ⇆ canopy jobs
   ⇆ bestool source conventions): args (bucket/prefix/region/role/retention/
   run-id), `secretKeyRef` password mount, source-host `canopy@<server-id>`,
   snapshot tags `canopy-device`/`canopy-run`.

---

## Cross-cutting decisions to settle before the dependent stage

These appear in multiple specs' open questions; resolving them early prevents
rework. Each is tagged with the latest stage by which it must be decided.

- **Enum representation** (`commons-types` shared vs validated String) — **Stage 0**,
  blocks (1) and the generated `api-types.ts`.
- **Migration ownership** (one `backup_credentials` migration vs split across
  (1)/(3)) — **Stage 0/2**, coordinate (1) and (3).
- **Group-level alerting Option A vs B** — **Stage 2**, recommend B; blocks (3)'s
  corruption alert and all group-level refs in (4).
- **Where `reveal_escrow` reads the Secret** (private-server own kube client) —
  **Stage 3**, ties to ops open-Q 2 (does private-server get `canopy-issuer`).
- **Longer-lived / non-chained restore creds for first-party consumers** —
  **Stage 2** (decided when the maintenance-Job direct-web-identity lands), so
  (8) isn't blocked.
- **"Back up now" command-channel transport** — deferred upstream; **does not
  block** Stage 3's bestool subcommands, which are transport-independent.
- **kopia + default-retention without PutObjectRetention** — the **spike**;
  blocks ops A2 and bestool kopia wiring.

---

## Summary one-liner per stage

- **Stage 0**: land the DB layer + shared enums (1); freeze the canopy↔ops
  contract and start the long-lead IRSA/OIDC infra (6); run the kopia spike.
- **Stage 1**: build the issuance hot path + the AWS/kube-on-AppState
  capability (2) — first end-to-end device slice.
- **Stage 2**: the control-plane Jobs (3) + detection/preflight + group-level
  alerting (4) in parallel, plus ops scheduler Deployments; decide restore-cred
  lifetime here.
- **Stage 3**: operator UI (5) and the bestool device client (7) in parallel.
- **Stage 4**: PGRO restore-verification (8), additive and last.
