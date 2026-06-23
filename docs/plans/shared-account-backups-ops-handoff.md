# Ops handoff: shared-account backup buckets

For ops/pulumi. Canopy is gaining the ability to **provision its own kopia
backup buckets in a shared AWS account**, for deployments that have **no BYO AWS
account**. Canopy creates one **auto-named bucket per group** on demand and
drives kopia against it. This handoff is everything ops needs to stand up the
AWS side; the canopy code is a parallel track and the two meet at first
provisioning.

Design rationale + the canopy-side changes shipped in PR #251 (the
implementation plan that drove it has been removed now that it's done). This doc
is the **AWS / pulumi build sheet**.

## Target account

- **Shared backups account: `576557632885`.** All canopy-provisioned shared
  buckets live here. (BES-owned infra account; distinct from any deployment's
  own account.)
- Recommend a **new pulumi stack `backups-shared`** (sibling to the existing
  `backups` stack), since it's one account + a fixed set of roles rather than
  per-deployment resources.

## What canopy does at runtime (so the role perms make sense)

1. **Onboard** (operator action, no AWS account needed): canopy writes a config
   with an auto-generated bucket name `bes-canopy-backup-<random>` and
   `status=provisioning`.
2. **Provision** (backups pod): assumes the **provisioner role**, runs
   `CreateBucket` + applies the security recipe (Object Lock + default
   GOVERNANCE retention, versioning, lifecycle, public-access-block, TLS-only
   policy, intelligent-tiering, billing tags). Canopy owns this recipe — ops does
   **not** pre-create buckets.
3. **Connect / back up** (backups pod + devices): kopia connects with
   per-group, **bucket-scoped** STS credentials minted from the **shared device
   role** (devices) and **shared maintenance role** (the pod).

Object Lock can only be enabled **at bucket creation**, which is why canopy
creates the bucket (not ops) — but the account still needs object-lock-capable
S3 (default; nothing to enable account-wide).

## The three roles (all in `576557632885`)

All three are **cross-account** roles assumed by canopy's identities in the
canopy/meta account — the same trust pattern as today's per-deployment device
and maintenance roles (see
[`backup-setup-wizard-ops-handoff.md`](./backup-setup-wizard-ops-handoff.md)).

### 1. Provisioner role — `s3:CreateBucket` + bucket config
- **Trusted by:** the backups pod's IRSA identity (`canopy-jobs`), the same
  identity that assumes the maintenance role today.
- **Permissions** (all conditioned to bucket-name pattern
  `arn:aws:s3:::bes-canopy-backup-*`):
  - `s3:CreateBucket`
  - `s3:PutBucketVersioning`
  - `s3:PutBucketObjectLockConfiguration`
  - `s3:PutLifecycleConfiguration`
  - `s3:GetBucketTagging` + `s3:PutBucketTagging` (provision **and** the ongoing
    billing-tag reconcile)
  - `s3:PutBucketPublicAccessBlock`
  - `s3:PutBucketPolicy`
  - `s3:PutObject` (only to seed the initial `.storageconfig`) on
    `arn:aws:s3:::bes-canopy-backup-*/*`
- **Not** data-plane (no read/delete of backup objects) — least privilege.

Canopy names buckets `bes-canopy-backup-<group-name>-<random>` (group name
sanitized for S3) and tags every bucket with `billing.product=backups`,
`billing.deployment=<group name>`, `billing.stage=<highest member rank>`
(`Production`→`prod`). It re-applies these tags on a periodic reconcile (group
renames / rank changes), hence the provisioner role's tagging perms above.

### 2. Shared device role — write-without-delete
- **Trusted by:** the canopy device-credential issuer (the same identity that
  assumes per-deployment device roles today — `canopyPrivateRoleArn` /
  public-server issuer).
- **Permissions** = the kopia `AWS_S3_MULTIPART_ACTIONS` set over
  `arn:aws:s3:::bes-canopy-backup-*` and `.../*`:
  `s3:GetObject`, `s3:PutObject`, `s3:AbortMultipartUpload`,
  `s3:ListBucketMultipartUploads`, `s3:ListMultipartUploadParts`,
  `s3:ListBucket`, `s3:GetBucketLocation`.
- **MUST NOT include** `s3:DeleteObject` or `s3:PutObjectRetention` — the
  device-compromise defense relies on the device being unable to delete or shorten
  locks; retention is applied server-side by the bucket default.
- Canopy further restricts each issued credential to a **single bucket** via an
  STS session policy, so the broad role grant never reaches a device unscoped.

### 3. Shared maintenance role — full data-plane
- **Trusted by:** the backups pod's IRSA identity (`canopy-jobs`).
- **Permissions:** full S3 over `arn:aws:s3:::bes-canopy-backup-*` (+ `/*`)
  **including** `s3:DeleteObject` (kopia maintenance reclaims via lifecycle on a
  versioned bucket) and CloudWatch read for bucket metrics — i.e. the same shape
  as today's per-deployment maintenance role.

## Bucket-name pattern

Canopy names buckets `bes-canopy-backup-<random-suffix>` (random for global
uniqueness + anti-enumeration). Scope all role resource ARNs/conditions to
`bes-canopy-backup-*` so the roles can only touch canopy-provisioned buckets.

## S3 bucket quota

Per-bucket isolation means one bucket per backed-up group. **Request a
general-purpose bucket quota increase** on `576557632885` via Service Quotas,
sized to the expected number of shared-account deployments (with headroom). The
account default is low; raise it up front so provisioning doesn't fail at scale.

## What to export back to canopy

Canopy reads these as env vars (set by the canopy deploy from the stack outputs):

| Stack output | Canopy env var | Set on |
|---|---|---|
| provisioner role ARN | `CANOPY_SHARED_BACKUP_PROVISIONER_ROLE_ARN` | backups pod |
| shared maintenance role ARN | `CANOPY_SHARED_BACKUP_MAINTENANCE_ROLE_ARN` | backups pod |
| shared device role ARN | `CANOPY_SHARED_BACKUP_DEVICE_ROLE_ARN` | public-server |
| default region | `CANOPY_SHARED_BACKUP_REGION` | backups pod + public-server |

(The device/maintenance role ARNs are also written into each shared config row
by canopy at onboarding; the env vars are the source canopy fills them from.)

## Ops action items

1. New `backups-shared` pulumi stack targeting account `576557632885`.
2. Three cross-account roles above (provisioner, shared device, shared
   maintenance), each trusting the appropriate canopy identity — reuse the same
   `canopy-jobs` / device-issuer trust used by the existing per-deployment roles.
3. Scope every role to bucket-name pattern `bes-canopy-backup-*`.
4. **Device role must not** grant `DeleteObject` / `PutObjectRetention`.
5. Request a general-purpose **bucket quota increase** on the account.
6. Export the three role ARNs + region; the canopy deploy maps them to the env
   vars above.
7. No buckets, object-lock, lifecycle, or `.storageconfig` to create — canopy
   does all of that at provisioning time. Ops only provides the account + roles +
   quota.
8. **Required — add tagging to the existing per-deployment maintenance roles.**
   Add `s3:GetBucketTagging` + `s3:PutBucketTagging` to **every** per-deployment
   **maintenance** role (the existing `backups` stack, across all deployments), so
   canopy reconciles the `billing.*` tags on the existing BYO buckets too. Canopy
   degrades gracefully (logs + skips a bucket whose role lacks the perm) — but
   that's a **safety net, not a reason to defer**: a skipped account silently
   misses its billing tags. Roll it out to all deployments.
