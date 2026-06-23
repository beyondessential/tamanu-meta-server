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
   Shared rows store `bucket` = canopy-generated
   `bes-canopy-backup-<group-name>-<random>`. **Total length ≤ 63 (the S3 limit)**,
   so only the group portion is truncated to fit the budget left by the fixed
   parts: `63 − len("bes-canopy-backup-") (18) − len("-<random>")`. With an
   8-char random suffix that leaves **≤ 36** chars for the sanitized group name.
   Group name **sanitized** for S3: lowercased, non-`[a-z0-9-]` → `-`, collapsed,
   leading/trailing hyphens trimmed, then truncated to the budget (re-trim any
   trailing hyphen left by truncation); random suffix for global uniqueness +
   anti-enumeration. The group name is for **AWS-side discoverability only** —
   the bucket name is fixed at creation and does **not** follow a later group
   rename (the billing tags do; see reconcile below).
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
   - **Billing tags** (see below): `billing.product=backups`,
     `billing.deployment=<group name>`, `billing.stage=<highest member rank>`.
   Then the existing `from_birth` kopia create path runs unchanged.

4. **Always session-scope device backup creds** — `public-server`'s
   `/backup-credentials` currently attaches no session policy on the `backup`
   purpose. Add `backup_session_policy(bucket, prefix)` (the
   `AWS_S3_MULTIPART_ACTIONS` write set — no delete, no `PutObjectRetention` —
   scoped to the one bucket) and attach it on **every** issuance. Redundant but
   harmless for `external`; **essential** for `shared` (one broad device role →
   creds that can only touch that group's bucket). This is the linchpin of
   shared-account isolation and reuses the `restore_session_policy` mechanism.

5. **Billing-tag reconcile** — a periodic pass in the backups pod (a slot-jittered
   step like s3-metrics) that, for **every** backup config (both `shared` and
   `external`), computes the desired billing tags and `PutBucketTagging` only when
   they've drifted (compare via `GetBucketTagging`). Catches group renames /
   rank changes (the bucket name can't follow a rename, but the
   `billing.deployment` tag does). Per-bucket creds: shared → provisioner role;
   external → the per-deployment maintenance role (needs the tagging perms — ops
   item below). **Graceful**: if the role lacks `PutBucketTagging` (e.g. an
   external account not yet updated), log and skip — never fail the loop.

### Billing tags

- `billing.product` = `backups` (fixed — distinguishes the backup bucket from the
  deployment's `tamanu` product spend).
- `billing.deployment` = the group **name** (the live name; reconciled on change).
- `billing.stage` = the group's **highest member rank** mapped via
  `stage_for_rank` (`Production` → `prod`); **omitted** when the group has no
  ranked members.

Building blocks already exist: `ServerGroup::highest_member_ranks`,
`commons_servers::backup_jobs::stage_for_rank`, and the `BillingLabels` shape
(currently unused — repurpose it, with `product` fixed to `backups`).

## IAM / account model (ops/pulumi — new `backups-shared` stack)

The shared backups AWS account (id supplied via ops config + the role-ARN env
vars; **not** hardcoded in this open-source repo). Three roles, each trusting
canopy's existing identities via cross-account `AssumeRole` (same pattern as
today's BYO per-bucket roles):

- **provisioner role** — `s3:CreateBucket`, `PutBucketVersioning`,
  `PutBucketObjectLockConfiguration`, `PutLifecycleConfiguration`,
  `PutBucketPublicAccessBlock`, `PutBucketPolicy`, **`GetBucketTagging` +
  `PutBucketTagging`** (provision + reconcile), conditioned to bucket-name pattern
  `bes-canopy-backup-*`. Assumed by the backups pod. ARN via env on the backups
  pod (`CANOPY_SHARED_BACKUP_PROVISIONER_ROLE_ARN`).
- **shared device role** — `AWS_S3_MULTIPART_ACTIONS` over
  `arn:aws:s3:::bes-canopy-backup-*/*`; only ever used via bucket-scoped session
  creds. ARN goes into each shared config's `target_role_arn`.
- **shared maintenance role** — data-plane incl. delete over the pattern; into
  `maintenance_role_arn`. Used by the backups pod for kopia maintenance /
  inspection.

**External (BYO) accounts:** the billing-tag reconcile also wants to tag existing
external buckets, which means the **per-deployment maintenance roles** (pulumi)
need `s3:GetBucketTagging` + `s3:PutBucketTagging` added. This is an ops change
across deployments; until a given account has it, canopy logs and skips that
bucket's tag reconcile (no failure).

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
- private-server: the new endpoint auto-names the bucket
  (`bes-canopy-backup-<group>-<random>`, sanitized + ≤ 63), writes the shared role
  ARNs, a `provisioning`/`placement=shared` row, and the passphrase Secret.
- bucket-name sanitization (pure): odd group names → valid S3 names, length cap,
  no leading/trailing/double hyphens.
- billing-tag computation (pure): product/deployment/stage from a group name +
  highest rank (incl. `Production`→`prod`, omit stage when no ranked members).
- jobs: the bucket-provision step is idempotent (create-then-skip); the tag
  reconcile applies on drift and is a no-op when tags already match.
- public-server: the backup session policy is attached and scopes to the bucket.
- e2e: the "shared account" wizard option.

## Out of scope (this plan)

Per-config retention overrides for shared buckets (use the org default first);
multiple shared accounts / quota sharding (single account until the bucket quota
is in sight).
