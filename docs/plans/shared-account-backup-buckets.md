# Plan: shared-account backup buckets

Let canopy provision its own kopia backup repos in a **shared AWS account**, for
deployments that have **no BYO AWS account** of their own. Each repo gets its own
**auto-named bucket** (not a shared bucket with per-tenant prefixes — see
"Why individual buckets" below). Entirely **invisible to the device side**
(bestool): the four device contracts (`/backup-capabilities`,
`/backup-credentials`, `/backup-target`, `/backup-report`) and their shapes are
unchanged — the device still receives an auto-named bucket, a region, a
passphrase, and session-scoped STS creds, with no hint of which account the
bucket lives in or who created it. **If this plan ever requires a device-side
change, that's the signal we've drifted.**

## Two topologies, one discriminator

Add a `placement` column to `server_group_backup_config` (migration; existing
rows default to `external`):

- **`external`** (today, unchanged): ops/pulumi creates the bucket + dedicated
  per-bucket IAM roles in the deployment's own account; canopy connects.
- **`shared`** (new): canopy auto-names + creates the bucket in the shared
  account, and uses **shared** device/maintenance roles, with per-group
  **session-scoped** credentials for isolation.

## Why individual buckets (not a shared bucket + prefixes)

1. **Object Lock default retention is bucket-level and set at creation** — the
   whole device-compromise defense (server-side GOVERNANCE retention so device
   creds need no `PutObjectRetention`) can't be varied per prefix and can't be
   retrofitted. Per-bucket lets each deployment have its own retention.
2. **Isolation is structural, not policy-dependent** — a compromised device's
   creds name `arn:aws:s3:::thatbucket/*`; cross-tenant access is impossible by
   construction rather than relying on `s3:prefix` conditions (which are
   error-prone — cf. the `GetBucketLocation`/`ListBucket` subtleties in
   `restore_session_policy`). Critical in a shared account holding unrelated
   clients' backups.
3. **Clean teardown / metrics / billing** — `BucketSizeBytes` is per-bucket (the
   s3-metrics task gets per-group size for free); decommission deletes the
   bucket; cost tags are per-bucket.
4. **Matches the existing model** — canopy is already one-bucket-per-group;
   prefixes would create a second backup topology to maintain.

The one real ceiling is the S3 per-account bucket quota — request an increase on
the shared account up front, sized to expected deployment count; reach for a
second shared account before giving up per-bucket isolation.

## Canopy changes

1. **Config model + migration** — add `placement` (`external` | `shared`;
   commons-types enum mirroring `BackupRepoMode`, with the diesel Text mapping).
   Shared rows store `bucket` = canopy-generated `bes-canopy-backup-<random>`
   (random suffix for global uniqueness + anti-enumeration),
   `target_role_arn`/`maintenance_role_arn` = the shared roles (from env/config),
   `region` = shared default, `mode = from_birth`.

2. **Entry point** — a new admin endpoint (a wizard option: "no AWS account → use
   shared backups") taking just `server_group_id` (+ optional region/retention).
   Canopy auto-names the bucket, fills the shared role ARNs, generates the
   passphrase + Secret, inserts `status=provisioning`, `placement=shared`. **No
   bucket probe** (it doesn't exist yet). Pulumi keeps calling the existing
   `external` `create`/`upsert` path untouched.

3. **Init-time bucket provisioning** — in `crates/jobs/src/backup/maintenance.rs`
   `run_init_op`, when `placement=shared`, **before** `kopia::run_init`: assume
   the **provisioner role** and `CreateBucket` + apply the full security recipe,
   idempotently (skip if the bucket already exists, like `storageconfig::ensure`
   does). New module e.g. `crates/jobs/src/backup/provision.rs`. Recipe (mirrors
   what pulumi's `backups` stack sets today):
   - Object Lock enabled + default **GOVERNANCE** retention (org default, e.g.
     30d; per-config override later).
   - Versioning enabled.
   - Lifecycle: `NoncurrentVersionExpiration` (reclaim deleted/old versions) +
     abort-incomplete-multipart.
   - Public Access Block (all on); TLS-only bucket policy.
   - Intelligent-Tiering (consistent with the `.storageconfig` pack-blob routing).
   - Billing tags (product / deployment / stage).
   Then the existing `from_birth` kopia create path runs unchanged.

4. **Always session-scope device backup creds** — `public-server`'s
   `/backup-credentials` currently attaches no session policy on the `backup`
   purpose. Add `backup_session_policy(bucket, prefix)` (the
   `AWS_S3_MULTIPART_ACTIONS` write set — no delete, no `PutObjectRetention` —
   scoped to the one bucket) and attach it on **every** issuance. Redundant but
   harmless for `external`; **essential** for `shared` (one broad device role →
   creds that can only touch that group's bucket). This is the linchpin of
   shared-account isolation and reuses the `restore_session_policy` mechanism.

## IAM / account model (ops/pulumi — new `backups-shared` stack)

The shared backups AWS account (id supplied via ops config + the role-ARN env
vars; **not** hardcoded in this open-source repo). Three roles, each trusting
canopy's existing identities via cross-account `AssumeRole` (same pattern as
today's BYO per-bucket roles):

- **provisioner role** — `s3:CreateBucket` + `PutBucket*` / lifecycle / tagging /
  policy, conditioned to bucket-name pattern `bes-canopy-backup-*`. Assumed by
  the backups pod only at init. ARN via env on the backups pod
  (`CANOPY_SHARED_BACKUP_PROVISIONER_ROLE_ARN`).
- **shared device role** — `AWS_S3_MULTIPART_ACTIONS` over
  `arn:aws:s3:::bes-canopy-backup-*/*`; only ever used via bucket-scoped session
  creds. ARN goes into each shared config's `target_role_arn`.
- **shared maintenance role** — data-plane incl. delete over the pattern; into
  `maintenance_role_arn`. Used by the backups pod for kopia maintenance /
  inspection.

Other shared env on canopy: `CANOPY_SHARED_BACKUP_REGION` (default region),
bucket-name prefix, default retention.

## Locked decisions

1. **Maintenance/kopia creds: not session-scoped.** Keep the existing
   auto-refreshing `AssumeRoleProvider` (which can't take a session policy);
   rely on the shared maintenance role being scoped to the bucket-name pattern +
   every kopia op explicitly targeting `config.bucket`. The backups pod is
   trusted control-plane; this preserves the long-run auto-refresh (no 1h cap).
2. **Separate provisioner role** (not `CreateBucket` folded into the maintenance
   role) — keeps the data-plane role least-privilege.
3. **Operator/admin-UI-driven entry point** (not pulumi) — the whole point is
   "no AWS account to run pulumi against." Dedicated endpoint, not an overload of
   `create`.
4. **Single shared-account default region** via env, per-group overridable in the
   wizard.

## Test coverage

- DB/migration: `placement` defaults to `external` on existing rows.
- private-server: the new endpoint auto-names the bucket, writes the shared role
  ARNs, a `provisioning`/`placement=shared` row, and the passphrase Secret.
- jobs: the bucket-provision step is idempotent (create-then-skip).
- public-server: the backup session policy is attached and scopes to the bucket.
- e2e: the "shared account" wizard option.

## Out of scope (this plan)

Per-config retention overrides for shared buckets (use the org default first);
multiple shared accounts / quota sharding (single account until the bucket quota
is in sight).
