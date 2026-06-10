# Backup credentials

Issue short-lived S3 credentials to devices on demand, so backups can run
without each server holding its own long-lived bucket credentials. Canopy
becomes the control plane; the actual backup bytes flow device → S3
directly (no proxy through Canopy).

## Goal

- One credential to manage per device — the existing mTLS identity.
- No long-lived AWS access keys on servers or in operator hands.
- Per-group isolation at the **bucket** boundary: each server-group has
  its own backups bucket, and a device only ever gets access to its own
  group's bucket. (Stronger than prefix-sharing inside one bucket — a
  scoping bug can't even name another group's bucket.)
- Within-group cross-device **and cross-account** restore: every device
  in a group can request read-only creds for the group's scope
  (`purpose=restore`), so restoring onto a freshly-rebuilt sibling is
  trivial and the restoring device can't accidentally damage the source
  backups. This explicitly includes the spanning-account case — e.g. a
  pre-prod server (in a different account) restoring from the group's
  prod-account backups — because membership, not the device's account,
  decides the target.
- Cross-*group* restore is explicitly **not** supported.
- Every credential issuance is recorded so we can answer "did device X
  back up today, and when."
- No device can delete backups: `DeleteObject` is never granted to a
  device cred, so a compromised server cannot wipe its group's backups.
- Canopy owns repository maintenance (compaction, GC, retention/expiry),
  running it as Kubernetes Jobs off the client servers — both to keep
  that heavy, slow work off the servers and so that delete rights live
  only in the control plane.

## Non-goals (for this plan)

- Serving S3/WebDAV/kopia-repository traffic through Canopy. Considered
  and rejected: bandwidth on the Canopy data path, availability coupling
  of every backup to Canopy uptime, and a much larger protocol surface.
- Cross-group restore. The endpoint signature can stay minimal because
  this is genuinely out of scope, not "deferred."
- Cost *dashboards* and byte-level reconciliation against S3 inventory.
  (A *basic* cached size/stats readout per group — `backup_repo_stats`,
  including the CloudWatch `BucketSizeBytes` billing basis — *is* in scope
  for display; missed-backup alerting and maintenance/expiry execution are
  owned by Canopy. What's deferred is rich cost/usage *analytics*.)
- Tuning the kopia *retention policy* values (keep N daily / M weekly).
  Canopy now *owns, declares, and enforces* the policy (see "Retention
  policy ownership") — the mechanism is in scope — but the actual values
  start at a sane default and per-group tuning is later.
- WORM immutability is a **required property of every group's bucket**
  (versioning + 30-day S3 Object Lock), not a follow-up. Existing buckets
  already have it; new per-group buckets must enable it at creation (it
  can't be retrofitted — see "Per-group buckets"). The plan has to be
  compatible with it: reclamation is via **S3 lifecycle** on the versioned
  bucket (in scope — see "Interaction with versioning + the 30-day Object
  Lock"), not kopia deletes directly.
- Picking the backup tool. Kopia is the assumed consumer because it
  fits the AWS SDK `credential_process` hook cleanly, but rclone works
  through the same mechanism if it turns out to be preferred.

## Architecture overview

```
┌──────────┐  AssumeRoleWithWebIdentity (IRSA)   ┌─────────┐
│ Canopy   │ ──────────────────────────────────► │   STS   │
│  pod     │ ◄────────────────────────────────── │         │
└──────────┘   irsa-session creds                 └─────────┘
     │
     │  POST /backup-credentials  (mTLS, ServerDevice)
     ▼
┌──────────┐  AssumeRole (group's per-bucket      ┌─────────┐
│  device  │   role, cross-account)              │   STS   │
│ (kopia + │ ◄────── creds_process JSON ──────── │         │
│ bestool) │                                     └─────────┘
└──────────┘
     │
     │  S3 API directly (boto/AWS SDK using temp creds)
     ▼
┌──────────┐
│    S3    │
└──────────┘

Maintenance path (Canopy-owned, no device involvement):

┌──────────┐  spawns k8s Job per group   ┌──────────────────┐
│ Canopy   │ ──────────────────────────► │ maintenance Job  │
│          │                             │ (kopia image,    │
└──────────┘                             │  own IRSA → full │
                                         │  S3 incl delete) │
                                         └──────────────────┘
                                                  │
                                                  │ kopia maintenance
                                                  ▼
                                            ┌──────────┐
                                            │    S3    │
                                            └──────────┘
```

Two STS calls per issuance:
1. The pod's IRSA session is implicit — kubernetes injects it.
2. From that session, Canopy performs a cross-account `AssumeRole` on the
   group's configured per-bucket role (`target_role_arn`), whose own
   policy structurally scopes to that one group's bucket. No session
   policy is involved for the backup purpose; a session policy is added
   only to downscope a `restore` to read-only. See "IAM model".

The resulting temp creds go back to the device verbatim.

## AWS quirks worth knowing up front

- **Chained AssumeRole sessions are capped at 1 hour**, regardless of
  the target role's `MaxSessionDuration`. Since IRSA gives the pod a
  session, all our issuances are chained. This is fine in practice
  because `credential_process` refreshes on demand, but it means Canopy
  must be reachable for the lifetime of a backup, not just at the
  start. If a device can reach S3, it can almost certainly reach
  Canopy, so this is a note rather than a constraint.
- **`credential_process` output format** is fixed by the AWS SDK:
  ```json
  {
    "Version": 1,
    "AccessKeyId": "...",
    "SecretAccessKey": "...",
    "SessionToken": "...",
    "Expiration": "2026-05-21T13:00:00Z"
  }
  ```
  `bestool` produces exactly this on stdout and exits.
- **Scoping is structural, via the per-bucket role.** Each group's role
  policy names only that group's bucket, so the bucket scoping doesn't
  depend on a session policy at all. Session policies enter only for the
  `restore` downscope (read-only), where the AND semantics mean a bug can
  only ever *over*-restrict, never expand — good failure mode. See "IAM
  model".

## Where it lives in the repo

A new endpoint on `public-server` alongside `artifacts.rs` and
`bestool.rs`, mounted at `/backup-credentials` (or similar). Uses the
existing `ServerDevice` extractor so the auth path is identical to other
device endpoints. No new binary needed — the original "probably a
separate binary" framing was for the proxy approach; control-plane only
fits cleanly into `public-server`.

Reasons not to put it in `private-server`:
- `private-server` is for operator/admin access via Tailscale; devices
  reach Canopy through the internet-facing `public-server` with mTLS.
- `public-server` already has the right state (DB via `State<Db>`), the
  right auth extractors (`ServerDevice`), and the right deployment shape.
  **But the AWS client is greenfield:** there is *no* AWS SDK anywhere in
  the workspace today (only the `aws-lc-rs` crypto backend — not the SDK),
  and `AppState` has no AWS field. This feature introduces canopy's first
  AWS SDK usage — net-new `aws-config` + `aws-sdk-{sts,s3,…}` deps, a
  credential/region provider, a client on `AppState`, and a `FromRef`
  impl. (Don't pin crate versions without checking the registry.)

## Database changes

### New table: `server_group_backup_config`

```sql
CREATE TABLE server_group_backup_config (
    group_id          UUID PRIMARY KEY REFERENCES server_groups(id) ON DELETE CASCADE,
    bucket            TEXT NOT NULL,           -- this group's own bucket (one bucket per group)
    prefix            TEXT NOT NULL DEFAULT '', -- usually empty: the repo lives at the bucket root
    target_role_arn   TEXT NOT NULL,           -- per-bucket role Canopy assumes (encodes the account; may be cross-account)
    region            TEXT,                    -- NULL → deployment default (AWS region)
    expected_interval INTERVAL,                -- NULL → manual-only (no schedule, no staleness); set → scheduled cadence + staleness
    retention         JSONB NOT NULL,          -- kopia keep-* policy; Canopy asserts it into the repo
    repo_password_ref TEXT NOT NULL,           -- reference to the secret, NOT the secret
    status            TEXT NOT NULL,           -- 'provisioning' | 'escrow_pending' | 'ready'; backups dormant until 'ready'
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

`bucket` is the group's **own** bucket — one bucket per group, not a
shared bucket with per-group prefixes. Isolation is therefore at the
bucket boundary, and `prefix` is usually empty (the kopia repo sits at
the bucket root); it stays in the schema only for the rare case of
parking a repo under a sub-path. See "Per-group buckets" for naming and
provisioning.

`target_role_arn` is the per-bucket role Canopy assumes to mint creds for
this group; because the ARN names the account, a bucket in a different
account (or a shared account hosting several groups) needs no special
handling — Canopy just assumes the configured ARN. The group → bucket →
role relationship is 1:1; what varies N:M (servers spanning accounts,
groups sharing an account) lives outside this row. See "IAM model".

`region` (an AWS region) is what `GET /backup-target` serves to devices.
`expected_interval` is the group's **declared backup cadence** and the
single source for it: it both paces the devices (see "Backup cadence")
and drives staleness detection — so schedule and alert can't drift apart.
It has three states: **NULL** → manual-only (backups *possible* via
operator one-off, but no schedule and no staleness alerting); **set** →
scheduled cadence + staleness. (A *missing config row entirely* is the
separate "not set up → `409`" case.)

`retention` holds the kopia keep-policy (e.g. `{"keep_daily": 7,
"keep_weekly": 4, "keep_monthly": 6, "keep_latest": 1}`); Canopy asserts
it into the repo at creation and each maintenance run — see "Retention
policy ownership". The org **minimum** (`keep_daily 7, keep_weekly 4,
keep_monthly 6`) is enforced as a floor in code — a per-group override may
*raise* the counts but never drop below it; `keep_latest 1` is the default
but is *not* floor-enforced.

`repo_password_ref` names the group's kopia repository password, held as a
**k8s Secret** (the column is the Secret name, never the password). The
password is Canopy-generated for from-birth repos or operator-supplied for
imported ones — see "Repository password ownership".

A row is keyed by **`group_id`** → `server_groups(id)` (the real grouping;
`server_groups` is a flat first-class table, and servers carry a nullable
`group_id` — there is no server hierarchy or "root server"). A device
whose server has no group, or whose group has no config row, gets
`409 Conflict` from the credential endpoint. (prod + its clones are
already one `server_group`, so a shared bucket maps to that single group —
the 1:1 group→bucket invariant holds; see "Per-group buckets".)

Keeping this in a separate table rather than columns on `server_groups`
because more backup-related fields are likely to land here (retention,
encryption-key id, monitoring thresholds) and they all naturally
co-locate, and most groups won't have backup config.

### New table: `backup_credential_issuances`

```sql
CREATE TABLE backup_credential_issuances (
    id                  BIGSERIAL PRIMARY KEY,
    device_id           UUID NOT NULL REFERENCES devices(id),
    group_id            UUID NOT NULL REFERENCES server_groups(id),
    issued_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at          TIMESTAMPTZ NOT NULL,
    purpose             TEXT NOT NULL,       -- "backup" | "restore"
    sts_assumed_role    TEXT NOT NULL,       -- the per-bucket target_role_arn that was assumed
    sts_request_id      TEXT,                -- from AssumeRole response, best-effort; pointer to the AssumeRole event
    access_key_id       TEXT,                -- the issued temp AccessKeyId; joins this row to downstream CloudTrail S3 activity
    bucket              TEXT NOT NULL,       -- snapshot of config at issuance time
    prefix              TEXT NOT NULL
);

CREATE INDEX ON backup_credential_issuances (device_id, issued_at DESC);
CREATE INDEX ON backup_credential_issuances (group_id, issued_at DESC);
```

This is the audit log of credential *issuance*. It answers "was a device
handed creds" but not "did the backup succeed" — see `backup_runs` for
that. We snapshot bucket/prefix at issuance time so the log stays correct
even if the group config is later changed. `access_key_id` is the durable
join key: any CloudTrail S3 event made with these creds carries the same
`AccessKeyId`, so months later you can map an S3 action back to this row
(device, purpose, bucket, time). `sts_request_id` is the best-effort
pointer to the `AssumeRole` event itself.

### New table: `backup_runs`

```sql
CREATE TABLE backup_runs (
    id              UUID PRIMARY KEY,         -- the run-uuid, minted by bestool at run start
    device_id       UUID NOT NULL REFERENCES devices(id),
    group_id        UUID NOT NULL REFERENCES server_groups(id),
    purpose         TEXT NOT NULL,            -- "backup" | "restore"
    outcome         TEXT NOT NULL,            -- "success" | "failure"
    error           TEXT,                     -- populated on failure
    bytes_uploaded  BIGINT,
    snapshot_id     TEXT,                     -- kopia snapshot/manifest id
    reported_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ON backup_runs (group_id, reported_at DESC);
CREATE INDEX ON backup_runs (device_id, reported_at DESC);
```

Written by `POST /backup-report`. This is the "a backup actually
completed" signal that staleness detection reads. Issuance alone is not
enough: a device can get creds and then crash before uploading anything,
and that must not read as a healthy backup.

`id` is **not** a serial — it's the **run-uuid bestool mints at run
start**, so the device can stamp it into the snapshot's tags (`canopy-run`)
*before* the row exists, and supplies it in `POST /backup-report`. That's
what makes the `snapshot → run → issuance` join real. A client-supplied
PK is safe: `device_id`/`group_id` come from the authenticated
`ServerDevice` context (not the client's claim), so attribution can't be
forged, and a duplicate `id` just fails *its own* insert (PK violation) —
it can't overwrite another row. Only `backup_runs` needs this; the other
audit tables stay Canopy-generated `BIGSERIAL`.

### New table: `backup_maintenance_runs`

```sql
CREATE TABLE backup_maintenance_runs (
    id              BIGSERIAL PRIMARY KEY,
    group_id        UUID NOT NULL REFERENCES server_groups(id),
    kind            TEXT NOT NULL,            -- "quick" | "full"
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ,
    outcome         TEXT,                     -- "success" | "failure"; NULL while running
    error           TEXT,
    bytes_reclaimed BIGINT                    -- if kopia surfaces it
);

CREATE INDEX ON backup_maintenance_runs (group_id, started_at DESC);
```

Written by the Canopy maintenance Jobs (see "Canopy-owned maintenance").
A group whose maintenance silently stops is a slow-motion failure (repo
bloat, retention not enforced), so this feeds the same staleness
alerting as `backup_runs`.

### New table: `backup_repo_snapshots`

```sql
CREATE TABLE backup_repo_snapshots (
    group_id           UUID NOT NULL REFERENCES server_groups(id),
    source             TEXT NOT NULL,            -- kopia source: canopy@<server-id>:<path>
    server_id          UUID REFERENCES servers(id),  -- parsed from source
    latest_snapshot_at TIMESTAMPTZ,
    observed_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, source)
);
```

The ground-truth inventory written by the read-only inspection Job
(signal 2): the latest snapshot actually present in the repo, per source.
`source` encodes the **server id** (the backup subject); the device that
took it lives in the snapshot's tags, not here.

### New table: `backup_repo_stats`

```sql
CREATE TABLE backup_repo_stats (
    group_id         UUID PRIMARY KEY REFERENCES server_groups(id),
    snapshot_count   INTEGER,
    source_count     INTEGER,
    logical_bytes    BIGINT,                  -- kopia: pre-dedup size across snapshots
    physical_bytes   BIGINT,                  -- kopia: deduplicated + compressed repo size
    bucket_bytes     BIGINT,                  -- S3 actual stored bytes (the billing basis; from CloudWatch)
    observed_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

Cached repo/bucket stats for operator display, written by **two distinct
tasks**: the read-only kopia **inspection Job** fills the repo-derived
fields (`snapshot_count`, `source_count`, `logical_bytes`,
`physical_bytes`), while a **separate S3-metrics task** fills
`bucket_bytes` from CloudWatch `BucketSizeBytes` (it's the real billing
basis and runs *ahead* of `physical_bytes` — versioning + the lock keep
expired-but-not-yet-deletable data around, so the gap is the visible cost
of the lock). `bucket_bytes` is **best-effort / nullable** — it may lag or
be absent independently of the kopia stats. The two are split because they
need different permissions: kopia inspection uses read-only S3 creds, but
the CloudWatch read needs `cloudwatch:GetMetricStatistics` (and the metric
lives in the *deployment* account, so it's a cross-account read) — granted
via a **dedicated least-privilege IRSA role** for the S3-metrics task or
folded into a canopy-wide IRSA (implementation choice). Keeping it off the
inspection creds avoids CloudWatch creep on the read-only inspector.

## Endpoint shape

Three endpoints, all `ServerDevice`-authenticated and all resolving the
device → root → `server_group_backup_config` the same way:

### `POST /backup-credentials` — short-lived creds

```
POST /backup-credentials
  Authorization: mTLS via ServerDevice
  Body: { "purpose": "backup" | "restore" }   -- default "backup"
  Response 200: {
    "Version": 1,
    "AccessKeyId": "...",
    "SecretAccessKey": "...",
    "SessionToken": "...",
    "Expiration": "..."
  }
  Response 412: device not bound to a live server (DeviceHasNoServer)
  Response 409: server ungrouped, or no backup config for the group
  Response 502: STS call failed
```

### `GET /backup-target` — where to back up to

```
GET /backup-target
  Authorization: mTLS via ServerDevice
  Response 200: {
    "storage": "s3",
    "bucket": "...",
    "prefix": "",             -- normally empty (repo at bucket root); non-empty only for a sub-path
    "region": "...",
    "repo_password": "..."    -- the kopia repo passphrase (read from the k8s Secret)
  }
  Response 412: device not bound to a live server (DeviceHasNoServer)
  Response 409: server ungrouped, or no backup config for the group
```

(`repo_password` is here because bestool needs it to `kopia repository
connect`. **Consequence:** `public-server` — the *internet-facing* pod —
must itself read the group's k8s Secret, so the net-new k8s machinery
isn't only on the jobs side: `public-server` needs a kube client +
ServiceAccount with Secret-read RBAC. This widens its blast radius, which
the stage-1 accepted-risk note and the blind-relay stub both cover.)

This is the piece that keeps the bucket out of device provisioning. The
`credential_process` output format (above) is **fixed by the AWS SDK** and
carries only the four credential fields — it cannot carry the bucket,
prefix, or region. Yet the device must know all of those to
address S3 at all. So rather than baking them into each device's kopia
config at provision time (which would make "rotating bucket / changing
prefix" a per-device reconfiguration, not the server-side-only change this
plan promises), bestool fetches them from Canopy at runtime via this
endpoint and reconstructs the kopia repository connection from the result.

The only thing a device is ever provisioned with is its Canopy URL, its
mTLS identity, and "run bestool" — never a bucket name. Canopy is the sole
owner of backup-target config; changing it is a single-row update with no
device-side coordination.

### `POST /backup-report` — outcome of a run

```
POST /backup-report
  Authorization: mTLS via ServerDevice
  Body: {
    "run_id":         "...",   -- the run-uuid bestool minted at run start (becomes backup_runs.id)
    "purpose": "backup" | "restore",
    "outcome": "success" | "failure",
    "error":          "...",   -- optional, on failure
    "bytes_uploaded": 12345,   -- optional
    "snapshot_id":    "..."    -- optional, kopia snapshot/manifest id
  }
  Response 204
```

bestool calls this after the kopia run finishes. It is what turns the
control plane's signal from "creds were issued" into "a backup actually
completed", and it is the input to staleness detection below. A device
that fails to even reach this endpoint shows up as staleness (no recent
success), so a crashed run is not silent.

(`region` is a per-group column on `server_group_backup_config` — each
group's bucket can live in its own AWS region. It's read from config, not
snapshotted; the issuance audit log snapshots only `bucket`/`prefix` — see
`backup_credential_issuances`.)

`purpose` is a real capability gate, not just audit metadata:

- `"backup"` (default): read + write + multipart, **no `DeleteObject`**,
  scoped to the group's bucket (prefix normally empty). Snapshot expiry
  and blob GC are *not* done by the device — Canopy owns maintenance (see
  "Canopy-owned maintenance" below), so the device never needs delete. A
  compromised server therefore cannot delete backups.
- `"restore"`: read-only (Get + ListBucket), scoped to the group's
  bucket (prefix normally empty). A device with these creds physically
  cannot mutate the bucket, so an accidental `kopia repository create` or
  similar can't damage backups.

No device purpose grants `DeleteObject`. The only identity that can
delete from the bucket is the Canopy maintenance role.

The choice is the caller's; both purposes are available to every
`Server`-role device within the group. There's no privilege gradient
("only some devices can restore") — that was considered and rejected
because cross-device restore within a group is meant to be cheap.

This is what makes **pre-prod restoring from prod backups** work without
a special path. The requesting device's own account is irrelevant: Canopy
resolves device → group → the group's single target (the prod-account
bucket/role) and assumes that role. The creds it returns are *prod-account
creds for the prod bucket*, so from S3's point of view the pre-prod device
is making ordinary same-account read calls — the only cross-account hop is
Canopy's `AssumeRole`, which it already does. A pre-prod server in another
account just asks for `purpose=restore` and reads the group's backups;
nothing about the spanning-account topology leaks into the device.

Handler flow:
1. `ServerDevice` extractor authenticates the caller. It yields only a
   `Device` (`device.0.0`) — server/group resolution is the handler's job
   (same as `statuses.rs`).
2. Resolve the device's server via `Server::live_by_device_id` (devices
   have no `server_id`; servers reference devices — and the
   `servers_device_id_unique` partial unique index guarantees at most one
   server per device, so this is a single server, not a set). No live
   server → **`412`** via the existing `AppError::DeviceHasNoServer`
   (which maps to `PRECONDITION_FAILED`, `commons-errors/src/lib.rs:193`;
   used by `events.rs`).
3. Read that server's `group_id: Option<Uuid>`; `None` (ungrouped) →
   `409`.
4. Read `server_group_backup_config` for that `group_id`; `409` if absent.
   This yields the group's `target_role_arn` (the per-bucket role to
   assume).
5. Only for `purpose=restore`: build the read-only session policy
   (template below). The `backup` purpose needs none — the per-bucket
   role's own policy is already correctly scoped.
6. Cross-account `sts:AssumeRole` on the group's `target_role_arn` (with
   the restore session policy if applicable), session name
   `canopy-<purpose>-<device-id>`.
7. Insert into `backup_credential_issuances` (recording the assumed role
   and the issued `access_key_id`).
8. Return the `credential_process` JSON.

### Permission templates

These are the permission *sets*. Under the decided IAM model, the
**backup** set is the per-bucket role's own policy (authored in the
group's Pulumi stack); the **restore** set is the read-only **session
policy** Canopy passes when assuming that same role for `purpose=restore`
(it ANDs down to read-only).

**`purpose = "backup"`** — read + write, no delete (the role policy):

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject", "s3:PutObject",
        "s3:AbortMultipartUpload", "s3:ListBucketMultipartUploads",
        "s3:ListMultipartUploadParts"
      ],
      "Resource": "arn:aws:s3:::<bucket>/<prefix>*"
    },
    {
      "Effect": "Allow",
      "Action": ["s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::<bucket>"
    },
    {
      "Effect": "Allow",
      "Action": ["s3:ListBucket"],
      "Resource": "arn:aws:s3:::<bucket>",
      "Condition": {
        "StringLike": { "s3:prefix": ["<prefix>*"] }
      }
    }
  ]
}
```

**`purpose = "restore"`** — read-only (the session policy):

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": ["s3:GetObject"],
      "Resource": "arn:aws:s3:::<bucket>/<prefix>*"
    },
    {
      "Effect": "Allow",
      "Action": ["s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::<bucket>"
    },
    {
      "Effect": "Allow",
      "Action": ["s3:ListBucket"],
      "Resource": "arn:aws:s3:::<bucket>",
      "Condition": {
        "StringLike": { "s3:prefix": ["<prefix>*"] }
      }
    }
  ]
}
```

`s3:GetBucketLocation` is a bucket-level op with no prefix notion — the
`s3:prefix` context key isn't populated for it, so it must be its own
**unconditioned** statement; folding it under the `s3:prefix` condition
(as an earlier draft did) would silently deny it. With one bucket per
group, `<prefix>` is normally empty, so the `ListBucket` condition
matches everything and the device lists the whole bucket — correct, since
the group owns it outright; the condition only does work in the rare
sub-path case. The restore variant omits all mutation actions — even an
explicit attempt to `kopia repository create` is rejected by S3, not just
by kopia.

`AbortMultipartUpload` is retained on the backup purpose: it can only
discard a device's *own in-flight* multipart upload, never a committed
object, so it doesn't weaken the "can't delete backups" property — it
just lets a failed upload clean up its own parts instead of leaving
billable orphans.

Dropping `DeleteObject` is defence-in-depth on top of the real backstop:
**every group's bucket has versioning + a 30-day S3 Object Lock** (both
required, set at creation — see "Per-group buckets"). A device cred has
neither `DeleteObject` nor `s3:BypassGovernanceRetention`, so it cannot
*delete* anything. But it *does* keep `PutObject` on existing keys, so the
one residual attack is **overwrite-poisoning**: a compromised device
overwrites live blobs, burying the good versions as *noncurrent*. This is
survivable, not destruction:

- Versioning preserves the good (noncurrent) versions; the Object Lock
  keeps them undeletable, and lifecycle can't reclaim them until the
  lock/lifecycle window elapses (see "Interaction with the 30-day Object
  Lock").
- kopia content blobs are **content-addressed and hash-verified**, so
  poisoning surfaces as *detectable corruption* (signal 2 catches it —
  see below), not silently-served bad data.
- Recovery is a version-level rollback driven by the maintenance role
  (see "Recovery from poisoning"), within the window.

So the precise guarantee: **a compromised device cannot delete backups,
and cannot *permanently* destroy them** — overwrite-poisoning is a
bounded, detectable DoS recoverable by version rollback within the
lock/lifecycle window. (Not "full stop": the current restore would be
broken until recovery.)

**The threat boundary is a compromised device.** The lock is `GOVERNANCE`,
not `COMPLIANCE` (per the `backups` Pulumi stack: `mode: 'GOVERNANCE',
days: 30`) — and we're keeping it that way; AWS-level compromise stays out
of scope (see non-goals).

### Recovery from poisoning

If signal 2 reports corruption / unreadability for a group's repo (the
overwrite-poisoning case above), recovery is a **version-level rollback,
driven by the maintenance role** — which already has `s3:*`, hence
`GetObjectVersion` / `ListBucketVersions`, in the deployment account. The
device `restore` creds stay read-only-*current* (no version access);
recovery is a deliberate operator action, not a device capability.

Runbook (to be written out at implementation):
1. Stop issuing to the group (so the attacker can't keep overwriting).
2. Identify the poisoning time from `backup_runs` / `backup_repo_snapshots`
   / version timestamps.
3. `kopia repository connect … --point-in-time <before-poisoning>` and
   restore/roll the repo back past the bad versions (kopia supports PIT
   reconnect against a versioned, object-locked bucket).
4. Re-verify, then resume issuance.

The deadline is the **lock/lifecycle window** (~30–35 days from the
overwrite — see "Interaction with the 30-day Object Lock"); after that
lifecycle reclaims the good noncurrent versions. Detection latency
(signal 2, ~daily) is far inside that window, so the window is ample —
but the runbook and the deadline must be known before an incident, not
discovered during one.

What GOVERNANCE deliberately leaves open is *AWS-level* compromise: a
principal holding `s3:BypassGovernanceRetention` — e.g. the full-access /
maintenance role's AWS credentials — can override the lock. Protecting
against that (COMPLIANCE mode, or denying bypass on the maintenance role)
is **explicitly out of scope** for now: defending the AWS account itself
is a separate problem from defending the fleet of devices. So the
guarantee is precisely "no device can destroy backups", not "nobody can".
See "Canopy-owned maintenance" for how the lock interacts with kopia GC.

**Accepted stage-1 risk — the issuer's blast radius.** `public-server`
(internet-facing) holds, fleet-wide: STS-assume on every group's
per-bucket role (read + **write**, i.e. the poisoning vector) and — since
it serves the repo password on `/backup-target` — Secret-read for every
group's repo password (which decrypts everything). So a `public-server`
RCE yields fleet-wide backup read + poison in one hop. For stage 1 we
**accept** this (same trust as canopy's other internet surface), relying
on poisoning detection + recovery to bound the damage. The hardening that
removes it (a blind-relay issuer) is captured separately in
[`backup-credentials-blind-relay.md`](./backup-credentials-blind-relay.md).
Two requirements that hold regardless: **the maintenance role's trust must
never include the `public-server` principal** (delete/bypass stays
Job-only, so a `public-server` compromise can't reach the *delete*
capability), and the issuer rights must not extend beyond per-bucket
assume.

## AWS setup (provisioned by the Pulumi `backups` stack)

This already mostly exists. The `backups` stack in `ops/pulumi/backups`
creates, per Pulumi stack, exactly the one-bucket-per-repo shape this plan
wants: an S3 bucket with `objectLockEnabled`, versioning, a 30-day
GOVERNANCE Object Lock, a `DenyInsecureTransport` bucket policy, plus two
IAM roles — a full-access role (`s3:*`) and a kopia role
(`AWS_S3_BACKUP_ACTIONS`). What it does *not* yet have is the
Canopy-mediated path; today those roles are assumed by **EC2** instance
profiles (`Principal: { Service: ec2.amazonaws.com }`), i.e. the
AWS-resident model where a server assumes its role directly.

The canopy fleet is the reason the Canopy path exists: those servers are
remote, authenticated by mTLS, and generally *not* EC2 instances in our
account, so they can't assume an AWS role directly — Canopy issues
short-lived creds for them instead. The IaC changes are therefore:

- **Trust Canopy's IRSA**, not (only) EC2: the roles Canopy assumes from
  need `Principal` set to the Canopy pod's IRSA role.
- **A reduced device-backup action set.** `AWS_S3_BACKUP_ACTIONS` today is
  `[...AWS_S3_MULTIPART_ACTIONS, s3:DeleteObject, s3:PutObjectRetention]` —
  it includes delete because, in the EC2 model, the server ran maintenance.
  Under this plan the device must *not* delete, so device-backup creds use
  the existing **`AWS_S3_MULTIPART_ACTIONS`** constant directly (already
  Get/Put + multipart + ListBucket/GetBucketLocation, **no** `DeleteObject`,
  **no** `PutObjectRetention`) rather than a new list. The full set stays on
  the maintenance role only. When adding IRSA trust to these roles, preserve
  the stack's existing optional IAM-Users path and the `.storageconfig`
  object so they aren't clobbered.

  *Why dropping `PutObjectRetention` is safe (H3):* the lock is applied by
  the bucket's **default retention** (the stack's `objectLockConfiguration`
  GOVERNANCE 30d auto-locks every PUT), so the device needs no permission to
  set retention itself — and we deliberately do **not** run the repo
  client-lock-aware (kopia setting/extending per-object locks). Client lock
  *extension* only buys protection against an AWS-level (delete-capable)
  attacker, which is out of scope; under device-compromise, versioning +
  default-retention + lifecycle + the recovery path already cover it.
  **Verify before finalizing:** confirm kopia's S3 backend writes/maintains
  fine against a default-retention bucket without client-side
  `PutObjectRetention` (it deletes by key → markers, doesn't set per-object
  retention). Fallback if kopia insists: re-grant `PutObjectRetention` to
  devices — safe, because in GOVERNANCE-without-bypass it can only *lengthen*
  a lock, never shorten it (minor cost-DoS vector, recoverable via the
  maintenance role's bypass).
- **Maintenance role** = the existing full-access role (or a sibling),
  assumed by the maintenance Job's own IRSA (not chained — see
  "Canopy-owned maintenance"). Only this identity can delete. It keeps
  `s3:*` (incl. the ability to bypass GOVERNANCE) — that's the accepted
  AWS-level trust boundary, see the threat-boundary note above; the
  protection target here is device compromise, not this first-party role.

### IAM model: per-bucket roles, assumed cross-account

**Decided: one role per bucket (= per group), assumed by Canopy
cross-account.** The topology forces this and the existing stack already
fits it:

- Most deployments have their **own AWS account**, so the bucket and its
  role live in the *deployment's* account while Canopy's pod (IRSA) lives
  in the central account. Issuing creds is a **cross-account `AssumeRole`**:
  the deployment-account role trusts Canopy's central principal, Canopy
  assumes it across the boundary. The group's config stores the **role
  ARN** (which encodes the account), so same-account, cross-account, and
  shared-account are one code path.
- A single central role over a bucket *pattern* (the discarded
  alternative) can't reach buckets in other accounts without each bucket's
  policy also granting it — i.e. per-account IaC regardless — so it buys
  nothing here.
- Granularity must be the **bucket, not the account**: small deployments
  now share an account (multiple groups co-tenant), so an account-scoped
  role would let co-tenant groups reach each other's buckets. A per-bucket
  role keeps them isolated *structurally* — the role's policy names only
  its own bucket, so a Canopy bug degrades to "wrong/again no creds",
  never "another group's bucket".

Scoping is therefore structural; no session policy is needed for
correctness. The one place a session policy still earns its keep is the
read-only **restore** downscope on the same per-bucket role (it can only
narrow). The permission *sets* in the templates below are the role's own
policy (backup role) plus that restore session policy.

### Per-group buckets

One Pulumi `backups` stack per group → one bucket per group, named by the
stack's convention (`bes-kopia-backups-<stack>`; README: begin with
`bes-`, contain `kopia-backups`). The role IAM keys off that convention.
The hard constraint, already satisfied by the stack but worth stating:
**S3 Object Lock must be enabled at bucket creation** — it cannot be
retrofitted. The stack does this (`objectLockEnabled: true` +
`objectLockConfiguration`), so a group's bucket is correct by
construction; getting it wrong would mean recreating the bucket.

The bucket also needs **lifecycle rules** — `noncurrentVersionExpiration`
~35 days (≥ the 30-day lock) + a 7-day `abortIncompleteMultipartUpload` —
since on a versioned bucket they're what actually reclaims space (see
"Interaction with versioning + the 30-day Object Lock"). The
`tamanu/on-linux` stack already has these (`kopia.ts`); the standalone
`backups` stack does **not** and must gain them as part of this work.

**IaC (Pulumi) provisions the buckets.** Onboarding a group means standing
up its `backups` stack (bucket + lock + roles) and then inserting the
`server_group_backup_config` row. Canopy only ever *uses* the bucket; it
is never granted `CreateBucket` or bucket-config powers, keeping those out
of the public-server's blast radius. Canopy's role is to *verify*, not
create: the per-group preflight checks the bucket exists, is reachable,
and has the expected Object Lock — so a missing or misconfigured bucket
(e.g. the stack not yet applied for a new group, or the lock weakened)
surfaces as an alert naming that group, rather than as silent backup
failure.

The ordering matters: insert the row before the stack is applied and the
group's devices get clean preflight/issuance failures (not corruption),
which the alert makes obvious. Worth noting for the onboarding runbook.

**Clones don't break the 1:1 invariant.** The `ops/pulumi` `cloneOf`
mechanism (in `tamanu/on-linux`) lets a clone stack *re-export* its
parent's bucket/role instead of provisioning its own — but a prod
deployment and its clones are **already one `server_group`** in canopy.
So that shared bucket maps to the *single* group's one config row; it's
the same within-group, cross-account picture as "pre-prod restores from
prod backups", not N groups → 1 bucket. The 1:1 *group*→bucket invariant
holds. (The standalone `backups` stack is strictly 1:1 per stack; the
`bucketName` config override only renames the bucket, it doesn't share
one.)

## `bestool` changes

These hang off bestool's existing `canopy` subcommand, since they're all
Canopy-mediated. A subcommand for the credential refresh, plumbed as
kopia's `credential_process`:

```
bestool canopy backup-credentials [--purpose backup|restore]   # default: backup
```

- Reads the device's mTLS identity from its existing location.
- POSTs to `/backup-credentials` with the optional purpose.
- Writes the response JSON to stdout verbatim.
- Exits 0 on success, non-zero on any failure (the AWS SDK treats any
  non-zero exit as "creds unavailable").

And a driver subcommand that owns the kopia invocation so the device
holds no hardcoded bucket:

```
bestool canopy backup [--purpose backup|restore]
```

- `GET /backup-target` to learn `{bucket, prefix, region}`
  **on every run** — never cached to persistent device config.
- Reconciles the kopia repository connection against that target (so a
  changed bucket/prefix is picked up here), with
  `credential_process = bestool canopy backup-credentials` for the creds.
- Runs the backup (or restore).
- Reports the run outcome back to Canopy (see "Backup reporting" below),
  so the control plane learns "backup completed", not just "creds
  issued".

The device is provisioned only with its Canopy URL and mTLS identity.
The bucket, prefix and region are never written to the device's
persistent config — bestool re-derives the repository connection from
Canopy on *every* run. This is the crux of the "no per-host action"
property: a server-side config change propagates to the whole fleet
automatically on each host's next run. There is no operator command to
run per host and nothing to "forget to re-run".

### Backup cadence and triggering

**Canopy is authoritative for *when* a device backs up, and the signal
rides the existing ~1-minute device↔canopy healthcheck** rather than a
separate device-side timer. On each tick Canopy answers "back up now /
nothing to do," and the device launches `bestool canopy backup` as its
own process when told. This drops responsiveness from a coarse local timer
to ~1 minute essentially for free (the tick already happens), and a
held-open bestool connection makes it cheaper still — near-instant if we
want push. The device holds no schedule of its own; the existing tick *is*
the trigger.

Canopy computes "back up now?" as **"the schedule is due OR an operator
requested a one-off":**

- **Scheduled** (`expected_interval` set): due when the last successful
  `backup_run` is older than `expected_interval`. One value drives both
  this and staleness, so they can't drift.
- **Manual-only** (`expected_interval` NULL): never due on a schedule, no
  staleness alerting — but an operator one-off still fires (see below).
- **Operator one-off** — a first-class capability: an operator (via Canopy)
  requests a best-effort-immediate backup; Canopy sets a pending-backup
  flag for that device/group and emits "back up now" on the next tick
  (within a minute, or instantly over a held connection), **bypassing the
  cadence debounce**. Works for *both* manual-only and scheduled groups
  (out-of-band "back up now"). Cleared when the run is reported.

This gives **provision-then-authorize**: the device image ships with the
backup wiring unconditionally and simply gets "nothing to do" (a benign,
non-failure state — `bestool` treats it as dormant, not an error) until an
operator authorizes/configures the group; backups then begin on the next
tick with no per-host action.

Changing a group's cadence is a single-row update — no per-host action.
The transport specifics (how the signal rides today's minute-cadence
healthcheck — tailnet poll, device poll, or a held-open connection) are an
implementation detail for the repo-alignment pass; the *model* (Canopy
authoritative, minute-cadence, schedule-or-manual) is fixed here.

bestool work is in a separate repo; this plan covers the Canopy side
and the bestool side will be a sibling change there.

## Backup reporting and staleness detection

The thing that actually protects against *silently* broken backups is
not the credential or target delivery mechanism — a backup that simply
doesn't run is silent regardless of how config reaches the device. The
protection is Canopy *knowing* each device's backup state and alerting
when one goes quiet. That detection catches a stale config, a dead
timer, a crashed run, or a network outage — all the ways a backup can
fail, not just config drift.

There are two independent ways for Canopy to know, and we use both:

### Signal 1 — device reports (timely, self-reported)

This is the cheap, immediate signal: bestool tells Canopy how each run
went, and a job scans those reports in the database.

1. bestool reports each run via `POST /backup-report`, written to
   `backup_runs`.
2. A periodic Canopy job (a tokio loop in the `jobs` crate, like
   `reachability`) scans the **servers expected to be backed up** — those
   in a group whose `server_group_backup_config` has a non-NULL
   `expected_interval` — joins each against its most recent `backup_runs`
   row, and classifies it. (Detection is **server-centric**: the subject
   is the server being protected; the device is the actor recorded in
   `backup_runs`/snapshot tags. A manual-only or unconfigured group is
   simply not in the scanned set, so unauthorized devices never alert.)
   - **Stale**: previously reported success but no `outcome = 'success'`
     newer than `expected_interval × 2` → alert. (`×2` = the anti-flap
     factor from the decisions; not per-group configurable yet. Given the
     ~1-min trigger retries quickly, two missed intervals signals genuine
     breakage, not a blip.)
   - **Never backed up**: present longer than `expected_interval × 2` with
     *no* successful `backup_runs` row → alert. "Present since" is
     `max(server-present, group-authorized)` where *group-authorized* is
     `server_group_backup_config.created_at` and *server-present* is taken
     from `device_server_associations` — but note `first_seen` there is per
     `(device_id, server_id)` **pair**, so for a server-centric anchor use
     `MIN(first_seen)` over that server's rows (earliest any device saw it)
     rather than treating it as a per-server scalar. This way neither a
     freshly-present server nor a freshly-authorized group false-alarms.
   - **Recovered**: a previously-stale server reporting success again
     clears the alert.
3. Alerts go through the existing **issues/events** model, exactly as
   `reachability` does — *not* a direct "open incident" call (there is no
   `Incident::open_for`). Construct a `NewEvent { source: "canopy", ref:
   "backup-staleness", severity, message, active }` and call
   `NewEvent::save(conn, server_id, device_id)` (mirroring reachability's
   `source="canopy"`/`ref="reachability"`). Downstream,
   `re_evaluate_incident_membership → find_or_open_incident →
   enqueue_slack_open → SlackOutbox::enqueue` opens the incident and queues
   Slack automatically — but only for a **monitored** server in a group and
   only when the issue severity satisfies `opens_incident()` (≥ `Error`),
   so staleness events must be raised at `Error`+. **Recovery** is the same
   `(source, ref)` event with `active: false` (lower severity), which lets
   the issue leave the incident and auto-close. The `slacker_outbox` binary
   drains the queue to Slack. (`Incident::resolve(incident_id, by, reason)`
   exists but is the operator-driven, UUID-keyed human resolution — not a
   group recovery call.) Distinct from the `backup-monitoring` Pulumi stack
   (AWS Backup service), so this detection is genuinely new.

   **`is_monitored` is absolute — by design, no backup exception.** Per-
   server staleness events are always *recorded* (so they show on the
   server's detail page / status indicator) but open an incident → Slack
   *only* for monitored servers — exactly the existing gate. We don't
   override it: some prods are intentionally intermittently-alive, and
   per-server backup checks on them would be noise. An unmonitored server
   in a backup-configured group thus has visible-but-non-paging backup
   staleness. **But group-level checks must still page** (next).

**Group-level checks alert regardless of `is_monitored`.** Whether Canopy
can mint creds for a group, whether its bucket/Object-Lock is intact
(preflight), whether maintenance is running, and whether the repo is
corrupt (poisoning detection) are *group / control-plane* concerns, not
any one server's — so they must **not** pass through the per-server
`is_monitored` gate. **Mechanism wrinkle to settle:** the incident model
is *server-keyed* (membership gates on a server's `is_monitored`), and
there's no obvious "group-level issue with no server". So group-level
alerts need a server-independent path — e.g. a group-scoped incident, or
raising against the group rather than a member. Flagged for
implementation; do not route these through a per-server `NewEvent`, which
would inherit the monitored gate.

The limit of signal 1 is that it's the *device's word*: it scans the
Canopy database for what devices reported, not for what's actually in the
bucket. A device that crashes before reporting looks (safely) stale; a
compromised or buggy device could report success it didn't achieve; a
snapshot that the device believes it wrote but that never persisted reads
as healthy. Reports are timely but not authoritative.

### Signal 2 — repository inspection (authoritative, periodic)

Canopy reads the **ground truth** — the snapshots that actually exist — via
a **dedicated, read-only inspection Job**, decoupled from maintenance at
both the job and schedule level. It connects to each group's repo with
**read-only (restore-level) creds** (never write/delete — smaller blast
radius than the maintenance Job), runs `kopia snapshot list`, and writes
`backup_repo_snapshots` (latest snapshot per source) plus the repo-derived
fields of `backup_repo_stats` (snapshot/source counts, logical & physical
size). The S3 `bucket_bytes` billing figure is filled by a **separate
S3-metrics task** with its own CloudWatch permissions — *not* on these
read-only creds (see `backup_repo_stats`). It runs on its **own cadence**
(defaulting to roughly `expected_interval`, tunable), so signal-2 freshness
isn't gated by the slow maintenance interval.

- Attribution: the kopia **source encodes the server** — `bestool` overrides
  the kopia hostname to the **server id** (`canopy@<server-id>:<path>`), so
  the source is the backup subject and survives device replacement
  (continuous history, no fragmented chain). The *device + run* that
  produced a snapshot live in its **tags** (`canopy-device=<uuid>`,
  `canopy-run=<run-uuid>`, where the run-uuid is **`backup_runs.id`** —
  minted by bestool at run start, stamped on the snapshot, then reported),
  closing the loop snapshot → run → issuance → CloudTrail.
- Reconciliation against signal 1 is where the value is:
  - report says success **but** no recent snapshot in the repo → the
    report is wrong or the upload didn't persist → **alert** (the case
    signal 1 alone cannot catch).
  - recent snapshot **but** no report → backups are fine, the reporting
    path is broken → lower-severity notice.
  - neither → genuinely stale (agrees with signal 1).
- It is *not* a replacement for signal 1's timeliness; reports stay the
  day-to-day signal, repo inspection is the periodic trust anchor.
- Trust property: signal 2 depends only on the bucket (Object-Lock
  protected), not on any device, so it's the signal a compromised server
  cannot fake into showing a healthy backup.
- **Poisoning / corruption detection → critical incident.** Inspection
  also *verifies* the repo (kopia detects content-blob hash mismatches /
  unreadable index — the overwrite-poisoning signature, see "Recovery from
  poisoning"). On detected corruption, raise a **`Critical`-severity**
  event so it opens an incident for immediate investigation (Critical
  satisfies `opens_incident()`), pointing at the recovery runbook. This is
  a **group-level** alert — it fires independent of any server's
  `is_monitored` (see the group-level-checks note under detection), since
  a corrupt repo endangers the whole group's backups regardless of which
  servers are monitored.

Both signals feed the same `Incident` alerting path.

This is the half of "did device X back up today" that the audit log
alone can't give: `backup_credential_issuances` says creds were handed
out; `backup_runs` says what the device *reported*; repository inspection
says what *actually landed* — and the three together shout when they
disagree.

A bucket change therefore can't silently break the fleet: if a host
fails to pick up the new target or fails to upload, its next expected
window lapses and Canopy alerts. The propagation itself is automatic
(every-run target fetch); detection is the backstop for when automatic
propagation doesn't take.

## Canopy-owned maintenance

A kopia repository needs periodic maintenance: index compaction, blob
garbage collection, and snapshot retention/expiry. In the default kopia
deployment, one client "owns" the repo and runs this. We don't want that
here, for two reasons:

- **Load.** Full maintenance walks the whole repo and rewrites packs —
  it takes a while and is I/O heavy. Putting it on a client server steals
  resources from the thing that server actually exists to do.
- **Least privilege.** Maintenance is the *only* operation that needs
  `DeleteObject`. If a client ran it, every client would need delete, and
  a compromised server could wipe the group's backups. By moving
  maintenance to Canopy, no device cred ever needs delete (see the
  permission templates above), so a compromised server cannot delete
  backups.

So Canopy owns maintenance. Concretely, a scheduler loop (a new
`crates/jobs/src/bin/<name>.rs` following the existing `reachability` /
`pingtask` `spawn()` + `loop { sleep(60); pool.get; … }` template, with its
own single-replica Deployment in `ops/pulumi/tamanu/meta/src/jobs.ts`)
**spawns a Kubernetes Job per group** that runs the maintenance cycle
against that group's bucket, then exits. Spawning a Job keeps the heavy,
long-running work off the loop pod and lets it use the kopia image.

**Greenfield infra (net-new to canopy):** the loop pattern matches
`reachability`, but everything about *spawning k8s Jobs* is new — canopy
has **no Kubernetes API client** (no `kube`/`k8s-openapi` in the workspace)
and its pods today carry **no ServiceAccount / IRSA** (the shared
`spec.ts` injects only env/affinity/tolerations/resources). So this needs:
a `kube` client dependency, a ServiceAccount plumbed through `spec.ts`
with RBAC to create Jobs and `get` Secrets, and IRSA wiring (the
`common/eksServiceAccount.ts` helper exists but isn't used by canopy yet).
Don't read "like reachability" as "already have the machinery" — that loop
only does a DB sweep.

The maintenance cycle is **three steps**, not just "run maintenance":
1. **assert** the group's retention policy into the repo (so the declared
   policy, not a drifted in-repo one, governs);
2. **`kopia snapshot expire`** — apply that policy, dropping snapshots
   beyond `keep-daily/weekly/monthly`. This step is *required* and easy to
   miss: because device creds have no delete, **clients can't self-expire
   at snapshot time**, so without Canopy running expire the retention
   policy never actually fires and the repo grows unbounded;
3. **`kopia maintenance run`** (quick or full) — index compaction + content
   GC of the now-unreferenced blobs.

Cadence: **quick daily, full weekly** (deployment-wide defaults, per-group
override later). Full more often is largely wasted — the 30-day Object Lock
means GC can't reclaim younger blobs anyway (the ~30-day reclamation lag),
and expired manifests are themselves locked, so expire/GC *mark* but
physical deletion lags. Client-side maintenance *and* expiry stay disabled
(clients lack delete), with the repo's maintenance owner set to the Canopy
identity.

**Scheduling is hash-jittered** so the fleet doesn't stampede: each group's
slot within its cadence window is `hash(group) mod window`, stable per
group, spreading Job-creation / STS / S3 / compute load evenly (applies to
inspection and per-group preflight too).

Spawned Jobs carry the org's three **billing tags** as pod labels —
`billing.product` / `billing.stage` / `billing.deployment` — set from the
group (its `billing.*` tags if present, else `product=tamanu`,
`deployment=` the group name, and `stage=` derived from the group's
highest member rank via the existing `ServerGroup::highest_member_ranks`
/ `rank_priority`). One gotcha to encode: the `ServerRank` Display strings
(`production`/`clone`/`demo`/`test`/`dev`) **don't all match the stage
values ops already emits** — ops's `typeGuess` yields `prod` (not
`production`). So map explicitly (`Production → "prod"`, `Demo → "demo"`,
…) to match existing CUR tags, and pick a fallback (`prod`, or omit
`billing.stage`) for groups whose members are all unranked — which
`highest_member_ranks` omits. So AWS split cost allocation attributes the
compute to the right deployment.

### Credentials for the maintenance Job

The Job assumes `canopy-backup-maintenance` (full S3 incl. delete on the
bucket). Crucially it gets this via **its own IRSA service account**, not
chained creds minted by Canopy. The reason is the 1-hour cap on chained
AssumeRole sessions noted up top: device backups tolerate it because
`credential_process` refreshes on demand, but a full-maintenance run can
exceed an hour, and we don't want it dying mid-rewrite. A direct IRSA
identity on the Job refreshes transparently with no cap.

Like the device path, maintenance uses the **per-bucket** role — the
group's full-access role (`s3:*` on that one bucket, incl. delete),
assumed directly by the Job's IRSA. Because the role is already scoped to
the single bucket, it needs no session-policy downscope (which is what
would re-introduce the 1-hour cap). It's broad *on its own bucket*, not
across a pattern — so a maintenance bug or compromise is still confined to
that one group, the same structural isolation the device path gets.

### Interaction with versioning + the 30-day Object Lock

Object Lock requires **versioning**, so every bucket here is versioned —
which changes how deletion actually works, and the plan must account for
it:

- On a versioned bucket, kopia's `DeleteObject` is **never refused**; it
  writes a *delete marker* and the prior version becomes *noncurrent*. So
  maintenance doesn't "throw errors on locked deletes" (an earlier draft
  said that) — it appears to succeed while **reclaiming nothing**.
- Physical reclamation therefore happens via **S3 lifecycle**, not kopia's
  delete: a `noncurrentVersionExpiration` rule expires noncurrent versions
  (and delete markers), gated by the Object Lock (a version still under
  retention is not expired until its lock lapses). This is also how kopia's
  *own* GC reclaims space (kopia "delete" → marker → lifecycle), so the
  lifecycle rule is **load-bearing for normal operation**, not optional.

**So the lifecycle rules are in scope** for the per-group buckets (see
"Per-group buckets"): `noncurrentVersionExpiration` ~35 days (≥ the 30-day
lock, matching the existing `tamanu/on-linux` stack) + a 7-day
`abortIncompleteMultipartUpload`. The standalone `backups` stack has no
lifecycle rules today — they must be added.

Consequences:
- Reclamation lags ~35 days (a version becomes deletable only after both
  the ~35-day rule *and* its lock have elapsed) — budget for up to ~35
  days of not-yet-collectable garbage on top of live data.
- That ~35-day window is also the **recovery deadline** for
  overwrite-poisoning (see "Recovery from poisoning"): the good noncurrent
  versions survive exactly until lifecycle reclaims them.
- We do **not** rely on kopia client-side lock-awareness (extending
  per-object locks) — that only matters against an AWS-level attacker,
  which is out of scope; see "AWS setup" for why devices need no
  `PutObjectRetention`.

### Retention policy ownership

"How long backups are kept" is a kopia **policy** (`keep-latest`,
`keep-daily`, `keep-weekly`, …) stored *in the repository* and applied at
snapshot + maintenance time. It is distinct from the 30-day Object Lock:
the lock is an immutability floor on physical deletion; the kopia policy
is the logical keep/expire schedule. Note the floor means the **effective
minimum retention is 30 days** no matter what the policy says, since
nothing can be deleted sooner.

Because Canopy already owns repo creation, the password, and maintenance,
it also owns the retention policy:

- The intended values live declaratively on `server_group_backup_config`
  (a `retention` field), not only inside the repo.
- Canopy sets the kopia policy from that field at repo creation, and
  **re-asserts it at the start of each maintenance run**, so the
  declared policy is the source of truth and an in-repo policy that has
  drifted (or been tampered with by a writer) is corrected rather than
  silently honoured.
- This keeps retention answerable from Canopy ("what's group X's policy?"
  is a DB read) instead of requiring a repo connect to inspect.

The *values themselves* are a product decision and remain deferred (see
non-goals); what's fixed here is that Canopy declares and enforces them,
with a sane default applied until tuned.

### Repository password ownership

This is the new dependency maintenance forces into the open: kopia
repositories are encrypted, and **connecting to one — to back up, to
restore, or to maintain — requires the repository password.** Today
nothing in the plan says where that password lives. Maintenance settles
it: Canopy owns the per-group repository password, because Canopy is the
one party that must always be able to connect (it runs maintenance) and
is already the source of truth for backup config.

Ownership splits cleanly: **IaC owns the bucket** (container), **Canopy
owns the repo** — including its encryption key, since key + retention +
maintenance are all the repo's lifecycle.

- **Storage: a k8s Secret** in Canopy's namespace (canopy is k8s-native;
  the password is a cluster-side encryption key, not an AWS resource, so
  it needn't live in the deployment account). `repo_password_ref` is the
  Secret name. Canopy reads it via the k8s API on demand (service-account
  RBAC: `get` secrets — handles dynamically-added groups without a pod
  restart). Maintenance/inspection Jobs **mount it via `secretKeyRef`**
  (never through env/logs).
- **Provenance, two modes:** *from-birth* — Canopy generates the password
  when it sets up a new repo; *import* — for adopting a pre-existing kopia
  repo, the operator supplies the existing passphrase (or points
  `repo_password_ref` at an already-created Secret). (Importing an
  existing repo also brings a pre-existing bucket whose lock/account may
  not match the from-birth assumptions — broader than the passphrase, and
  related to the migration concern.)
- **DR escrow (from-birth only):** the backups survive a Canopy
  catastrophe (object-locked in the deployment account) but are *useless
  without the passphrase*, which would die with Canopy's Secrets/DB. So at
  generation, surface it **once** in the Tailscale-gated admin UI —
  "copy this into Bitwarden now" — ideally gated on operator
  acknowledgment before the repo is marked ready. The k8s Secret is the
  operational copy; Bitwarden is the break-glass copy. (Imported repos
  skip this; the operator already has the passphrase.)
- Serving to devices is *not* a new exposure: any client writing to an
  encrypted kopia repo inherently holds the password (intrinsic to
  direct-to-S3 backup). Canopy serves it over mTLS on `/backup-target`.
- The repo's maintenance *owner* is set to the Canopy maintenance
  identity, and client-side maintenance/expiry is disabled, so clients
  never attempt operations they lack delete rights for.

### Audit

Maintenance runs are logged like device runs — a `backup_maintenance_runs`
table (group_id, kind quick|full, started/finished, outcome, error,
bytes reclaimed if available). Same value as `backup_runs`: "is
maintenance actually happening for this group, and did it succeed." A
group whose maintenance has silently stopped is a slow-motion problem
(repo bloat, retention not enforced), so it feeds the same staleness
alerting.

### Where it lives

The device-facing endpoints are on `public-server` (mTLS). Maintenance
scheduling is internal control-plane work, not device-facing, and needs
Kubernetes API access (a service account with RBAC to create Jobs). The
exact home (private-server, a dedicated worker, or a CronJob that itself
fans out per-group Jobs) is an implementation detail to settle then;
what's fixed is that it's Canopy's responsibility, not a client's.

## Operator workflows & repo provisioning (private-server UI)

Onboarding a group is a real workflow with operator UI shipping **in this
plan** (private-server React, `TailscaleAdmin`-gated, fitting the existing
`private-web` SPA/admin pattern), each screen with Playwright e2e coverage
per AGENTS.md — *not* deferred to SQL bootstrap.

**Repo creation.** "Canopy owns repo creation" is concrete: a one-shot
Canopy-spawned **init Job** (kopia image) runs `kopia repository create`
+ asserts the initial retention policy, using the **maintenance role's**
IRSA (creating the format blob needs more than the no-delete device set).
It is triggered from the onboarding UI, not implicitly on first backup.

**Lifecycle state** (`server_group_backup_config.status`):
`provisioning` (init Job running) → `escrow_pending` (repo created,
awaiting escrow ack) → `ready`. Backups stay **dormant (412/409)** until
`ready`, so "authorized" = config set + repo created + escrow
acknowledged.

Screens:
- **Onboarding / config** — set `bucket` / `target_role_arn` / `region` /
  `expected_interval` / `retention`, and kick off repo creation.
- **Escrow** — after creation, reveal the generated passphrase **once**
  with a "saved to Bitwarden" acknowledgment that flips `escrow_pending →
  ready` (from-birth repos only; imports skip — operator already has it).
- **One-off "backup now"** — an operator button that enqueues a pending
  request (see below); the device picks it up on its next ~1-minute tick.
- **Stats panel** — read-only view of `backup_repo_stats` + recent
  `backup_runs` per group.

**One-off request state** (so the pending flag has a home):

```sql
CREATE TABLE backup_requests (
    server_id    UUID NOT NULL REFERENCES servers(id),
    purpose      TEXT NOT NULL,            -- "backup" | "restore"
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    requested_by TEXT,                     -- operator identity
    PRIMARY KEY (server_id, purpose)
);
```

A row makes Canopy answer "back up now" on the server's next tick
(bypassing the cadence debounce); cleared when the run is reported.

## Upstream access preflight

The device-staleness signals above watch the *devices*. This watches
**Canopy's own upstream access**, which is a different and higher-stakes
failure class: if the pod's IRSA annotation is dropped (shared), or a
group's per-bucket role trust is edited / the role deleted / that group's
bucket Object Lock removed (per-group), devices can't get creds and
maintenance Jobs fail — and the backups stop without any per-device fault
to point at. We should learn that from Canopy checking itself, not from
devices starting to fail.

**It runs per server-group, not fleet-wide.** Each group has its own
bucket (its own account, region, role, and Object Lock config), so
bucket-level access can differ per group. A failure is therefore normally
*scoped to one group*; it's only fleet-wide when the genuinely shared
piece breaks. The preflight reflects that split:

Shared, checked once:
- **Identity resolves** — `sts:GetCallerIdentity` confirms the pod's IRSA
  web-identity is mounted and valid. This is the only genuinely shared
  piece; everything else is per-bucket and therefore per-group.

Per configured group (deep checks — shallow "did AssumeRole return" isn't
enough):
- **Both purposes issue valid creds** — assume *that group's*
  `target_role_arn` (cross-account) **both ways**: plain (the `backup`
  path) *and* with the read-only restore session policy (the `restore`
  path), each followed by a **read-only no-op** against the bucket. This
  is the issuance path itself, so it proves we can mint *working* creds
  for *every* purpose — catching a broken restore session policy (the
  `GetBucketLocation` class of bug) proactively, not at restore time,
  while plain backup issuance looks fine.
- **Object Lock is in place** — verify *that group's* bucket still has the
  expected (≥30-day) Object Lock configuration. The whole "a compromised
  server can't destroy backups" guarantee rests on this; if someone
  removes or weakens the lock, the protection silently erodes with no
  other symptom, so it must be actively checked — per bucket, since the
  config is per bucket.
- **Maintenance path** — no separate maintenance preflight Job needed: the
  read-only inspection Job already connects to each group's repo on its
  cadence (proving reachability + password), and maintenance-specific
  failures surface via `backup_maintenance_runs`.

**Cadence (hash-jittered per group, like maintenance):** the shared
`GetCallerIdentity` rides the ~1-minute loop (free); the per-group deep
checks run hourly (cheap at fleet scale, and a broken bucket/lock isn't a
sub-hour emergency). Alerts name the affected group(s) at control-plane
severity (distinct from per-device staleness); a check failing for *every*
group points at the shared IRSA identity rather than any one bucket.

Prefer **behavioural** checks (try to assume; try a harmless S3 op) over
IAM/policy *introspection*: behavioural checks test the real path and
need no extra `iam:Get*` permissions, except the Object Lock check, which
does need `s3:GetBucketObjectLockConfiguration` — an acceptable read to
add given how load-bearing the lock is. Consistent with the session-
policy reasoning earlier: we care that the effect is right, not that a
described config *looks* right.

Reactively, the live paths are signals too: `/backup-credentials` already
502s on STS failure and maintenance failures land in
`backup_maintenance_runs` — track their rates (per group) so a regression
surfaces between preflights, not only at the next scheduled probe.

This should **alert, not gate readiness**: a failing upstream check
pulling the pod out of rotation would only make the problem worse.
Surface it loudly; don't take Canopy down over it.

## Operational story (what we gain, day-one)

- **"Did device X back up today?"** — three corroborating answers:
  `backup_runs` (what the device *reported*), `backup_credential_issuances`
  (the cheaper "creds were issued" proxy), and `backup_repo_snapshots`
  (what *actually landed* in the repo). Disagreement is itself a signal.
- **"How big / how much is this costing?"** — `backup_repo_stats` caches
  repo size (logical & physical) and the S3 `bucket_bytes` billing basis
  per group for display, refreshed by the inspection cycle.
- **One-off / manual backups** — an operator can trigger a best-effort
  immediate backup for any group (scheduled or manual-only) via Canopy;
  it fires on the next ~1-minute tick. Useful before risky changes, and
  the only backup path for manual-only groups.
- **Decommissioning a device** — revoke its mTLS cert (existing
  mechanism); it can no longer call the endpoint, so it can no longer
  get fresh creds. Already-issued creds expire within an hour.
- **Decommissioning a group** — delete its `server_group_backup_config`
  row; devices in that group start getting `409`. The group's bucket and
  its Object-Lock'd objects persist independently — they can't be deleted
  until their locks expire (~30 days), so bucket teardown is a deliberate,
  delayed step, not a side effect of removing the config row.
- **Onboarding a group** — IaC provisions its bucket (with Object Lock,
  see "Per-group buckets"), then the `server_group_backup_config` row is
  inserted. Devices in the group start succeeding on their next run; if
  the row lands before the bucket, the preflight alerts and issuance
  fails cleanly rather than corrupting anything.
- **Changing region** — update the
  `server_group_backup_config` row. Each device picks it up on its next
  scheduled `bestool canopy backup` (every-run target fetch); no per-host
  command, no coordinated cutover. Staleness detection flags any host
  that fails to roll over. (This means pointing at a *different* bucket;
  since a kopia repo lives in its bucket, that's a repo migration or a
  start-fresh the operator owns — not a free flip — even though the config
  change itself propagates automatically.)
- **A compromised server can't *destroy* backups** — no device cred grants
  `DeleteObject`, so deletion is impossible; and overwrite-poisoning is
  survivable (versioning + Object Lock preserve the good versions;
  signal-2 detects the corruption; version-rollback recovers within the
  lock/lifecycle window). The current restore is broken until recovery —
  so it's a detectable, recoverable DoS, not data loss. (See "Recovery
  from poisoning".)
- **Maintenance just happens** — clients don't run it and don't have the
  rights to; Canopy spawns the Jobs. Repo bloat / unenforced retention
  from a stuck client owner is no longer a failure mode, and
  `backup_maintenance_runs` + staleness alerting catch a stuck *Canopy*
  maintenance instead.
- **A broken control plane is caught at the source** — the per-group
  upstream preflight alerts if Canopy loses a group's bucket access, or a
  bucket's Object Lock is removed, *before* a single device fails, and
  names the affected group rather than waiting for the fleet to discover
  it the hard way.

## Decisions (resolved in review)

The original open questions were worked through one by one; outcomes
(detail in the body sections):

1. **Cross-account hardening** — **no `ExternalId`** (first-party within
   one Org; the per-bucket role trust already names Canopy's principal).
   The `backups`-stack changes (IRSA trust, reduced action set, export role
   ARN) are implementation tasks, not open.
2. **Session naming + correlation** — `RoleSessionName` =
   **`canopy-<purpose>-<uuid>`** (the `canopy-` prefix makes provenance
   unambiguous in CloudTrail; purpose + device inline). Maintenance/
   inspection sessions: `canopy-maint-<group>`; per-bucket roles get a
   `canopy-` name too. **Also store the issued `AccessKeyId`** on
   `backup_credential_issuances` — the durable join from a CloudTrail S3
   event back to the issuance (purpose/device/bucket/time).
3. **`sts_request_id`** — **keep** (nullable, best-effort via the SDK
   `RequestId` trait); belt-and-suspenders with `access_key_id`.
4. **Device→server/group resolution** — dormant-when-unconfigured is
   locked: **no live server → `412`** (`DeviceHasNoServer`, the existing
   code maps it to `PRECONDITION_FAILED`), **ungrouped or no config →
   `409`**. (Corrected from "single 409" — `DeviceHasNoServer` is 412 in
   the codebase.) Gives the **provision-then-authorize** property; bestool
   treats *both* 412 and 409 as benign "dormant, nothing to do". Lookup
   mechanics resolved in the repo-alignment commit.
5. **Staleness "present since"** —
   `max(device_server_associations.first_seen, server_group_backup_config.created_at)`.
6. **Grace factor** — **`×2`**, not per-group configurable yet.
7. **Default retention** — `keep-daily 7, keep-weekly 4, keep-monthly 6`
   (org minimum, **floor-enforced in code**), `keep-latest 1` default (not
   floored), `keep-annual 0`.
8. **Cadence / trigger** — **Canopy-authoritative**, signal on the
   ~1-minute healthcheck; three `expected_interval` states; **operator
   one-off backup** is a first-class capability; `409` = dormant. Transport
   deferred.
9. **Inspection + source mapping** — **dedicated read-only inspection Job
   on its own schedule** (decoupled from maintenance); kopia source host =
   **server id**; snapshot **tags** carry device + a client-minted run-uuid
   (echoed in `backup_runs`); writes `backup_repo_snapshots` +
   `backup_repo_stats`.
10. **Preflight** — **deep** checks (S3 no-op + `GetBucketObjectLockConfiguration`),
    **both purposes** (backup plain + restore with session policy);
    shared-identity ~every minute, per-group hourly, hash-jittered.
11. **Repo password** — **k8s Secret** in Canopy's namespace; Canopy
    generates for from-birth / operator supplies for import; read via k8s
    API, mounted into Jobs via `secretKeyRef`, served to devices over mTLS;
    **one-off Bitwarden escrow** for from-birth repos.
12. **Maintenance** — scheduler loops in the **`jobs` crate**; cycle =
    **assert-retention → `kopia snapshot expire` → `kopia maintenance run`**;
    quick-daily / full-weekly; **hash-jittered** per-group scheduling;
    spawned Jobs carry the three `billing.*` tags.
13. **Maintenance Job IRSA** — **direct cross-account web-identity** (not
    chained, so no 1-hour cap); OIDC-provider-per-account wiring is ops/IaC.

New capabilities captured along the way: operator one-off backup (8),
repo-import onboarding mode + Bitwarden escrow (11), repo/bucket stats for
display (9 + the cost non-goal), `billing.*` tags on Jobs (12),
hash-jittered scheduling (12).

## Repo-alignment outcomes (applied) + remaining prerequisites

A repo-alignment review reconciled the plan against the real `canopy` +
`ops/pulumi` source. **Applied throughout:** rekeyed everything to
`group_id` → `server_groups` (flat table; no `root_server_id`/hierarchy);
device→server via `Server::live_by_device_id` (not `device.server_id`);
alerting via `NewEvent::save(source="canopy", ref="backup-staleness")` at
severity ≥ `Error` (not the non-existent `Incident::open_for`); schedulers
as new `jobs`-crate bins; the `ServerRank::Production`→`"prod"` mapping;
`first_seen` is per device-server pair (`MIN` over the server); reuse
`AWS_S3_MULTIPART_ACTIONS` for the device set.

**Net-new infrastructure this requires (none exists in canopy today):**
- The **AWS SDK** — `aws-config` + `aws-sdk-{sts,s3,…}`, a provider, and an
  `AppState` client (first AWS SDK usage in the workspace).
- A **Kubernetes API client** (`kube`/`k8s-openapi`) — for spawning Jobs
  (jobs side) **and** for `public-server` to read the repo-password Secret
  it serves on `/backup-target`.
- A **ServiceAccount + IRSA** on canopy's pods — **including
  `public-server`**, which today carries none (the shared `spec.ts`
  injects no ServiceAccount). RBAC: the jobs side needs create-Jobs +
  `get` Secrets; `public-server` needs `get` Secrets for repo passwords.
  Plus IRSA wiring (the `common/eksServiceAccount.ts` helper exists but is
  unused) and the OIDC-provider-per-account wiring for cross-account Job
  web-identity.

**Still open (genuine choices, not gaps):**
- The **transport** for the cadence signal (how it rides today's
  ~1-minute healthcheck — tailnet poll / device poll / held-open).
- Tunable cadence defaults (heartbeat/inspection/maintenance/preflight) —
  chosen above, adjustable later.

(The operator UI — onboarding, repo creation, escrow, one-off backup,
stats — is **decided: ships in-plan**; see "Operator workflows & repo
provisioning".)

## Out of scope (do not silently fold in)

- Cross-group restore mechanism (explicitly disallowed by product
  decision).
- **AWS-level compromise protection.** The threat boundary is a
  compromised *device*; defending against a compromised AWS principal that
  holds `s3:BypassGovernanceRetention` (e.g. the maintenance role's creds)
  is a separate problem. We stay on GOVERNANCE Object Lock and accept that
  the first-party maintenance role can bypass it. (COMPLIANCE mode,
  bypass-denial, and kopia client-side **lock extension** — which keeps
  live blobs' locks fresh against a delete-capable attacker — all remain
  available later if the threat model widens.)
- Data-plane proxy / Canopy-served S3/WebDAV (explicitly rejected).
- **Device-local S3 proxy** (kopia → `localhost`, a bestool daemon
  re-signs and forwards to the real bucket). Considered as an
  alternative to the every-run target fetch: it maximises decoupling
  (kopia never knows the bucket) and could be a future interposition
  point, but it relocates rather than removes the large S3 protocol
  surface this plan avoids (SigV4 re-signing, streaming multipart, a
  supervised long-lived daemon), and — crucially — it does **not** solve
  silent breakage either (a dead proxy is just as silent). Detection
  does that. Rejected for the first cut; revisit only if we want the
  interposition point for its own sake.
- Migrating *existing* shared-bucket data into per-group buckets (if any
  group is on a shared bucket today). The design is one-bucket-per-group;
  moving legacy data across is a separate operational task.
- bestool subcommand implementation (separate repo).
- **Tuning** retention policy values and encryption-at-rest beyond default
  SSE-S3. (Retention *execution* is in scope — Canopy maintenance runs it —
  but the kopia retention values start at a default and per-group tuning is
  later. Note: the *reclamation* lifecycle rules — `noncurrentVersionExpiration`
  + `abortIncompleteMultipartUpload` — are now **in scope**, since they're
  load-bearing for GC on a versioned bucket; see "Interaction with
  versioning + the 30-day Object Lock".)
- (No longer deferred: the operator UI for onboarding, repo creation,
  escrow, one-off backup, and the stats panel **ships in this plan** — see
  "Operator workflows & repo provisioning". What's still out is *bulk* /
  fleet-wide config editing beyond the per-group onboarding screen.)
