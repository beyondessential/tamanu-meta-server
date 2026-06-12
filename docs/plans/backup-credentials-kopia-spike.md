# kopia-behaviour verification spike (day-0 blocker)

Resolves the spike named in
[`backup-credentials-implementation-order.md`](./backup-credentials-implementation-order.md).
Its job: pick the device IAM action-set branch and confirm the kopia
assumptions, so ops can finalize the managed policy and bestool can wire
the kopia connect/snapshot path.

**Method:** authoritative from kopia docs + source and S3 semantics (no
live test was runnable in-session — no kopia binary, no valid AWS creds).
A gold-standard live-test script is at the end; the *decision* doesn't wait
on it, but two items (PIT on real AWS, the bucket-default-retention write
path) warrant a live confirm before relying on them.

## Verdict

**Branch A confirmed: device creds = `AWS_S3_MULTIPART_ACTIONS` (no
`s3:PutObjectRetention`, no `s3:DeleteObject`); the kopia repo is created
*non-lock-aware*; we rely on the bucket's default GOVERNANCE 30-day
retention + versioning + lifecycle.** Ops can finalize the device managed
policy as exactly `AWS_S3_MULTIPART_ACTIONS`.

## Findings (per question)

### 1. PutObjectRetention vs bucket-default retention (the branch decision)

kopia needs `s3:PutObjectRetention` **only when it manages retention
itself** — i.e. the repo is created with `--retention-mode` and locks are
renewed via full-maintenance `--extend-object-locks`. That's the
kopia-documented "ransomware" path, and it's confirmed that even with
`--extend-object-locks` kopia still requires `PutObjectRetention` on the
primary bucket (so it can't be isolated away).

We deliberately do **not** use that mode. Instead we create a *plain*
kopia repo (no `--retention-mode`) against a bucket whose **default object
lock retention** is GOVERNANCE 30d. S3 applies the default retention to
every `PutObject` **server-side**, which requires only `s3:PutObject` — not
`s3:PutObjectRetention`. So the device key needs neither delete nor
retention permission. ✔ matches the plan's H3.

- **Consequence (already accepted):** without `--extend-object-locks`, a
  live blob's lock is fixed at 30d from its last write and never renewed.
  Irrelevant under the device-compromise threat (the device can't delete);
  it only matters against an AWS-level attacker, which is out of scope.
  Re-enabling renewal later = lock-aware mode + `PutObjectRetention` on the
  **maintenance** role (never the device).
- **Caveat:** this is *not* the kopia-documented happy path; it rests on S3
  default-retention semantics (solid) rather than a kopia doc page. Live
  test item (a) confirms kopia writes + maintains happily this way.

### 2. Maintenance deletes on a versioned bucket (H2)

Non-lock-aware kopia issues real `DeleteObject`; on a versioned bucket that
writes a **delete marker** (succeeds, reclaims nothing) — it does **not**
error. Reclamation is via the S3 lifecycle `noncurrentVersionExpiration`
rule, as the plan says. The maintenance role needs `s3:DeleteObject` (it
has it). (Note: kopia *also* has a "hidden marker" soft-delete it uses with
restricted/lock-aware keys — not our path; our maintenance role deletes for
real and lets lifecycle reclaim.) ✔ matches the plan's H2; the earlier
"throws errors on locked deletes" framing was wrong.

### 3. Temporary credentials / `credential_process` (`AWS_SESSION_TOKEN`)

`kopia repository create/connect s3` supports `--session-token` (and the
`AWS_SESSION_TOKEN` env). So the short-lived STS creds (which include a
session token) work, and the `credential_process`-style refresh is viable.
✔ unblocks the bestool credential path.

### 4. Source host = server-id

`--override-hostname` and `--override-username` exist, set at **`kopia
repository connect`** time (connection-level, *not* per-snapshot — the
per-snapshot `--hostname`/`--username` were removed in 0.6.0). bestool
reconnects per run (it re-derives the connection from Canopy every run), so
it passes `--override-hostname=<server-id>` (`--override-username=canopy`)
on connect → source `canopy@<server-id>:<path>`. The **type** goes in the
path and a `canopy-type=<type>` snapshot tag (`kopia snapshot create
--tags`). ✔ matches the plan's per-`(server, type)` source model.

### 5. Point-in-time recovery (H1)

`kopia repository connect … --point-in-time=<ts>` exists and is the
documented recovery path for a versioned+locked bucket (recover to before
a poisoning/deletion). ✔ the H1 recovery runbook is real.

- **Caveat:** GitHub issue #4346 reports `--point-in-time` failing with
  "repository not initialized" on some S3-*compatible* endpoints, and
  #3492 covers a recovery edge case (missing files after deleted objects).
  Real AWS S3 is kopia's primary supported target, but **PIT recovery must
  be live-tested on real AWS S3** before we depend on it operationally —
  it's our break-glass path. Live test item (b).

### 6. CloudWatch `BucketSizeBytes` dimension (lower-stakes, may trail)

`BucketSizeBytes` carries a `StorageType` dimension, and **all object
versions (current + noncurrent) count** toward it per storage class. So the
S3-metrics task sums `BucketSizeBytes` across the relevant `StorageType`s —
`StandardStorage` plus the intelligent-tiering classes (`.storageconfig`
puts pack blobs in `INTELLIGENT_TIERING`). Confirm the exact emitted
dimensions against a real bucket. Lower-stakes; `bucket_bytes` is
best-effort anyway.

## What this unblocks

- **ops** (action-set): the device role = `AWS_S3_MULTIPART_ACTIONS`, no
  `PutObjectRetention`. The repo is created non-lock-aware; the bucket
  keeps its default GOVERNANCE 30d retention + the lifecycle rules.
- **bestool** (kopia wiring): connect with `--session-token` +
  `--override-hostname=<server-id>`; `kopia snapshot create --tags
  canopy-device=… canopy-run=… canopy-type=…`; do **not** pass
  `--retention-mode`.

## Remaining live confirmations (run when a throwaway bucket + creds exist)

These don't change the branch; they de-risk the two assumptions that rest
on semantics/known-issues rather than a kopia doc. Script below.

(a) Plain kopia repo create + snapshot + maintenance against a versioned,
    default-GOVERNANCE-retention bucket, using a **device key without
    PutObjectRetention/Delete** and a **maintenance key with delete** —
    confirm no `AccessDenied` for retention and that maintenance succeeds.
(b) `--point-in-time` reconnect works on real AWS S3.
(c) The `BucketSizeBytes` `StorageType` dimensions emitted.

```bash
#!/usr/bin/env bash
# Operator-run live confirmation. Needs: aws cli with creds, kopia.
# Creates a throwaway bucket — review + delete after.
set -euo pipefail
B="bes-kopia-spike-$(date +%s)"; R="ap-southeast-2"
KP="spike-pass-$(openssl rand -hex 8)"

# 1. Versioned bucket + object lock + 30d GOVERNANCE default retention
aws s3api create-bucket --bucket "$B" --region "$R" \
  --create-bucket-configuration LocationConstraint="$R" \
  --object-lock-enabled-for-bucket
aws s3api put-object-lock-configuration --bucket "$B" \
  --object-lock-configuration 'ObjectLockEnabled=Enabled,Rule={DefaultRetention={Mode=GOVERNANCE,Days=30}}'

# 2. DEVICE creds: NO delete, NO PutObjectRetention (AWS_S3_MULTIPART_ACTIONS).
#    Use a scoped IAM user/role with: s3:GetObject,PutObject,
#    AbortMultipartUpload,ListBucketMultipartUploads,ListMultipartUploadParts,
#    ListBucket,GetBucketLocation on the bucket. Export its creds, then:
kopia repository create s3 --bucket "$B" --region "$R" --password "$KP" \
  --override-hostname server-test --override-username canopy
#    ^ EXPECT: success. FAIL = AccessDenied mentioning PutObjectRetention
#      → fall back to granting PutObjectRetention (safe; lengthen-only).
echo hello > /tmp/spike.txt
kopia snapshot create /tmp/spike.txt --tags canopy-type:tamanu-postgres
aws s3api list-object-versions --bucket "$B" --query 'Versions[0].ObjectLockMode' # EXPECT: GOVERNANCE (default applied on PUT)

# 3. MAINTENANCE creds: full S3 incl. delete. Re-connect with those, then:
kopia maintenance run --full --safety none   # EXPECT: success; deletes become markers
aws s3api list-object-versions --bucket "$B" --query 'DeleteMarkers' # EXPECT: markers present, no errors

# 4. PIT (item b): note a timestamp, mutate, then:
kopia repository connect s3 --bucket "$B" --region "$R" --password "$KP" \
  --point-in-time="$(date -u +%Y-%m-%dT%H:%M:%SZ)"   # EXPECT: connects (watch for issue #4346)

# 5. CloudWatch dimensions (item c): after metrics populate (~a day),
aws cloudwatch list-metrics --namespace AWS/S3 --metric-name BucketSizeBytes \
  --dimensions Name=BucketName,Value="$B"

# cleanup: object-locked objects can't be deleted for 30d; the throwaway
# bucket will linger until the lock lapses (expected). Tag it for teardown.
```
