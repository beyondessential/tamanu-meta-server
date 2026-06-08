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
- Within-group cross-device restore: every device in a group can
  request read-only creds for the group's scope (`purpose=restore`), so
  restoring onto a freshly-rebuilt sibling is trivial and the restoring
  device can't accidentally damage the source backups.
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
- Cost dashboards and byte-level reconciliation against S3
  inventory / CloudWatch. (Missed-backup alerting *is* in scope now —
  see staleness detection — and maintenance/expiry execution is owned by
  Canopy; what's deferred is cost/usage analytics.)
- Tuning the kopia *retention policy* values (keep N daily / M weekly).
  Canopy now *owns, declares, and enforces* the policy (see "Retention
  policy ownership") — the mechanism is in scope — but the actual values
  start at a sane default and per-group tuning is later.
- WORM immutability is a **required property of every group's bucket**
  (30-day S3 Object Lock), not a follow-up. Existing buckets already have
  it; new per-group buckets must enable it at creation (it can't be
  retrofitted — see "Per-group buckets"). The plan also has to be
  compatible with it (object-lock-aware kopia repos; GC reclaims on a
  ~30-day lag).
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
┌──────────┐  AssumeRole + session policy        ┌─────────┐
│  device  │                                     │   STS   │
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
2. From that session, Canopy calls `AssumeRole` on a dedicated
   `canopy-backup-issuer` role, passing a session policy that narrows
   S3 access to the requesting device's group bucket.

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
- **Session policies are ANDed with the role policy.** The role's
  policy grants broad S3 access across the bucket pattern; the per-call
  session policy narrows that to the one group's bucket. A bug in the
  session policy can only ever *over*-restrict, never expand — good
  failure mode.

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
- `public-server` already has the right state (DB, AWS clients if we
  add one), the right auth extractors, and the right deployment shape.

## Database changes

### New table: `server_group_backup_config`

```sql
CREATE TABLE server_group_backup_config (
    root_server_id    UUID PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
    bucket            TEXT NOT NULL,           -- this group's own bucket (one bucket per group)
    prefix            TEXT NOT NULL DEFAULT '', -- usually empty: the repo lives at the bucket root
    region            TEXT,                    -- NULL → deployment default
    endpoint          TEXT,                    -- NULL → AWS; set for non-AWS S3
    expected_interval INTERVAL,                -- declared cadence: paces devices AND drives staleness; NULL → neither
    retention         JSONB NOT NULL,          -- kopia keep-* policy; Canopy asserts it into the repo
    repo_password_ref TEXT NOT NULL,           -- reference to the secret, NOT the secret
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

`region`/`endpoint` are what `GET /backup-target` serves to devices.
`expected_interval` is the group's **declared backup cadence** and the
single source for it: it both paces the devices (see "Backup cadence")
and drives staleness detection. Set it to `1 day` and devices aim to
back up daily *and* Canopy alerts when one hasn't. Because it's one
value, the schedule and the alert can't drift apart.

`retention` holds the kopia keep-policy (e.g. `{"keep_daily": 7,
"keep_weekly": 4, ...}`); Canopy asserts it into the repo at creation and
each maintenance run — see "Retention policy ownership". The values start
at a default; the schedule is owned here, not left to drift inside the
repo.

`repo_password_ref` points at the group's kopia repository password
(held in a k8s secret / Secrets Manager, not stored here in plaintext —
see "Repository password ownership"). It's the secret *reference*; the
column never holds the password itself.

A row is keyed by the root server id (the parentless server that heads
the group). Devices without a configured root row → `409 Conflict` from
the credential endpoint with an instructive message.

Keeping this in a separate table rather than columns on `servers`
because:
- The columns are meaningful only on roots — adding them to `servers`
  pollutes every row with mostly-NULL fields.
- More backup-related fields are likely to land here (retention,
  encryption-key id, monitoring thresholds) and they all naturally
  co-locate.

### New table: `backup_credential_issuances`

```sql
CREATE TABLE backup_credential_issuances (
    id                  BIGSERIAL PRIMARY KEY,
    device_id           UUID NOT NULL REFERENCES devices(id),
    root_server_id      UUID NOT NULL REFERENCES servers(id),
    issued_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at          TIMESTAMPTZ NOT NULL,
    purpose             TEXT NOT NULL,       -- "backup" | "restore"
    sts_assumed_role    TEXT NOT NULL,
    sts_request_id      TEXT,                -- from AssumeRole response, for cross-ref with CloudTrail
    bucket              TEXT NOT NULL,       -- snapshot of config at issuance time
    prefix              TEXT NOT NULL
);

CREATE INDEX ON backup_credential_issuances (device_id, issued_at DESC);
CREATE INDEX ON backup_credential_issuances (root_server_id, issued_at DESC);
```

This is the audit log of credential *issuance*. It answers "was a device
handed creds" but not "did the backup succeed" — see `backup_runs` for
that. We snapshot bucket/prefix at issuance time so the log stays correct
even if the group config is later changed.

### New table: `backup_runs`

```sql
CREATE TABLE backup_runs (
    id              BIGSERIAL PRIMARY KEY,
    device_id       UUID NOT NULL REFERENCES devices(id),
    root_server_id  UUID NOT NULL REFERENCES servers(id),
    purpose         TEXT NOT NULL,            -- "backup" | "restore"
    outcome         TEXT NOT NULL,            -- "success" | "failure"
    error           TEXT,                     -- populated on failure
    bytes_uploaded  BIGINT,
    snapshot_id     TEXT,                     -- kopia snapshot/manifest id
    reported_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ON backup_runs (root_server_id, reported_at DESC);
CREATE INDEX ON backup_runs (device_id, reported_at DESC);
```

Written by `POST /backup-report`. This is the "a backup actually
completed" signal that staleness detection reads. Issuance alone is not
enough: a device can get creds and then crash before uploading anything,
and that must not read as a healthy backup.

### New table: `backup_maintenance_runs`

```sql
CREATE TABLE backup_maintenance_runs (
    id              BIGSERIAL PRIMARY KEY,
    root_server_id  UUID NOT NULL REFERENCES servers(id),
    kind            TEXT NOT NULL,            -- "quick" | "full"
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ,
    outcome         TEXT,                     -- "success" | "failure"; NULL while running
    error           TEXT,
    bytes_reclaimed BIGINT                    -- if kopia surfaces it
);

CREATE INDEX ON backup_maintenance_runs (root_server_id, started_at DESC);
```

Written by the Canopy maintenance Jobs (see "Canopy-owned maintenance").
A group whose maintenance silently stops is a slow-motion failure (repo
bloat, retention not enforced), so this feeds the same staleness
alerting as `backup_runs`.

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
  Response 409: no backup config for this device's group
  Response 502: STS call failed
```

### `GET /backup-target` — where to back up to

```
GET /backup-target
  Authorization: mTLS via ServerDevice
  Response 200: {
    "storage": "s3",
    "bucket": "...",
    "prefix": "groups/<root-id>/",
    "region": "...",
    "endpoint": "..."         -- optional; for non-AWS S3
  }
  Response 409: no backup config for this device's group
```

This is the piece that keeps the bucket out of device provisioning. The
`credential_process` output format (above) is **fixed by the AWS SDK** and
carries only the four credential fields — it cannot carry the bucket,
prefix, region, or endpoint. Yet the device must know all of those to
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

(`region`/`endpoint` are per-group columns on `server_group_backup_config`
— each group's bucket can live in its own region, or behind a non-AWS S3
endpoint. The audit-log snapshot should capture whatever the device was
actually told.)

`purpose` is a real capability gate, not just audit metadata:

- `"backup"` (default): read + write + multipart, **no `DeleteObject`**,
  scoped to the group's prefix. Snapshot expiry and blob GC are *not*
  done by the device — Canopy owns maintenance (see "Canopy-owned
  maintenance" below), so the device never needs delete. A compromised
  server therefore cannot delete backups.
- `"restore"`: read-only (Get + ListBucket), scoped to the group's
  prefix. A device with these creds physically cannot mutate the
  bucket, so an accidental `kopia repository create` or similar can't
  damage backups.

No device purpose grants `DeleteObject`. The only identity that can
delete from the bucket is the Canopy maintenance role.

The choice is the caller's; both purposes are available to every
`Server`-role device within the group. There's no privilege gradient
("only some devices can restore") — that was considered and rejected
because cross-device restore within a group is meant to be cheap.

Handler flow:
1. `ServerDevice` extractor authenticates the caller (existing).
2. Look up the device's server via `device.server_id` (assumed to
   exist — verify when implementing; if a device isn't yet associated
   with a server, return `409`).
3. `Server::root_id` walks up to the group root.
4. Read `server_group_backup_config` for that root; `409` if absent.
5. Build the session policy (template below).
6. Call `sts:AssumeRole` on `canopy-backup-issuer` with the session
   policy and a session name like `device-<device-id>`.
7. Insert into `backup_credential_issuances`.
8. Return the `credential_process` JSON.

### Session policy templates

**`purpose = "backup"`** — read + write, no delete:

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
      "Action": ["s3:ListBucket", "s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::<bucket>",
      "Condition": {
        "StringLike": { "s3:prefix": ["<prefix>*"] }
      }
    }
  ]
}
```

**`purpose = "restore"`** — read-only:

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
      "Action": ["s3:ListBucket", "s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::<bucket>",
      "Condition": {
        "StringLike": { "s3:prefix": ["<prefix>*"] }
      }
    }
  ]
}
```

With one bucket per group, `<prefix>` is normally empty, so these
resolve to the whole bucket (`<bucket>/*`) and an unconditioned
`ListBucket` — which is correct, since the group owns the bucket
outright. The `s3:prefix` condition only does work in the rare
sub-path case; it's kept in the template so a non-empty prefix still
scopes listings. The restore variant omits all mutation actions — even
an explicit attempt to `kopia repository create` is rejected by S3, not
just by kopia.

`AbortMultipartUpload` is retained on the backup purpose: it can only
discard a device's *own in-flight* multipart upload, never a committed
object, so it doesn't weaken the "can't delete backups" property — it
just lets a failed upload clean up its own parts instead of leaving
billable orphans.

Dropping `DeleteObject` is defence-in-depth on top of the real backstop:
**every group's bucket has a 30-day S3 Object Lock** (a required property,
set at creation — see "Per-group buckets"). So even a cred that *could*
delete or overwrite can't destroy a backup object younger than 30 days —
a locked version is retained regardless. The no-delete device
policy still earns its place (it removes the most destructive action
outright, and avoids accidental client-side expiry), but the guarantee
"a compromised server cannot destroy recent backups" rests on Object
Lock, not on IAM alone. See "Canopy-owned maintenance" for how the lock
interacts with kopia GC.

## AWS setup (out-of-band of this plan, but documented here for completeness)

Because there's one bucket per group, the roles grant against a **bucket
naming convention** rather than a single named bucket, and the per-call
session policy still narrows to the one bucket the call is for.

- `canopy-backup-issuer` role with trust policy allowing the Canopy pod's
  IRSA role to assume it.
- Role policy grants broad `s3:*` on the bucket pattern (e.g.
  `arn:aws:s3:::canopy-backup-*`) — broad because the session policy is
  what actually constrains each call to the one group's bucket. Note: the
  *device* session policies never grant delete, so even though the role
  could, the creds handed to devices can't.
- `canopy-backup-maintenance` role (or service account) for the
  maintenance Jobs, with full S3 incl. `DeleteObject` on the same bucket
  pattern. This is the only identity in the system that can delete backup
  objects. It is first-party (Canopy-controlled), never handed to a
  device. See "Canopy-owned maintenance" for why it gets its own IRSA
  identity rather than chained creds.

### Per-group buckets

Each group gets its own bucket, named by convention (e.g.
`canopy-backup-<root-id>`) so the role patterns above cover it without
per-bucket IAM edits. The hard constraint: **S3 Object Lock must be
enabled at bucket creation** — it cannot be retrofitted onto an existing
bucket. So provisioning a group's bucket must, atomically at creation,
enable versioning + Object Lock with the default ≥30-day retention.
Getting this wrong isn't fixable in place; it means recreating the
bucket.

Who provisions it is a decision (flagged in open questions), and it
trades off against privilege:

- **Out-of-band (IaC / admin step):** a Terraform module or admin tool
  creates the bucket with lock when a group is onboarded; Canopy only
  *uses* it (and the preflight verifies it). Keeps `CreateBucket` and
  bucket-config powers out of the public-server's blast radius.
- **Canopy-driven:** Canopy creates the bucket on group-config setup.
  More "behind the scenes" but hands the control plane bucket-creation
  authority, a meaningful privilege increase.

Recommend out-of-band provisioning with preflight verification for the
first cut: bucket creation is rare (once per group) and heavyweight, and
the preflight already checks the lock is correct — so we get the
"managed" feel without granting `CreateBucket` to the request path.

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

- `GET /backup-target` to learn `{bucket, prefix, region, endpoint}`
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

### Backup cadence

Who decides *when* `bestool canopy backup` runs is a real question,
because Canopy is **pull-based** — it has no inbound path to devices (the
push/proxy direction is explicitly rejected), so it physically cannot
trigger a backup. A local trigger must exist. The design choice is only
whether the cadence is hardcoded per host (drifts from Canopy's
`expected_interval`, and changing the fleet cadence means touching every
host) or owned by Canopy like everything else.

We own it in Canopy:

- A local systemd timer fires `bestool canopy backup` as a *heartbeat* —
  frequent enough (say hourly) that it never gates the real cadence, and
  needing no per-host tuning.
- Each tick, bestool already fetches the target/creds; it also reads the
  group's `expected_interval` and backs up only if the last successful
  run is older than that. So the *effective* schedule is Canopy's
  declared cadence; the local timer is just "wake up and ask".
- Staleness alerting reads the same `expected_interval`, so the cadence
  the fleet targets and the cadence Canopy polices are one number and
  cannot diverge.

Changing a group's cadence is then the same single-row update as any
other config — no per-host action.

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
2. A periodic Canopy job (alongside the existing health/alerting
   machinery — the same path that surfaces and recovers other device
   health signals) scans the **devices expected to be backing up** —
   i.e. every `Server`-role device whose group root has a
   `server_group_backup_config` with a non-NULL `expected_interval` —
   and, joining each against its most recent `backup_runs` row,
   classifies it:
   - **Stale**: a device that previously reported successful backups but
     has no `outcome = 'success'` row newer than `expected_interval`
     (times a small grace factor) → alert.
   - **Never backed up**: a device in the scanned set that has been
     present longer than `expected_interval` and has *no* successful
     `backup_runs` row → alert. (Catches a host that was never wired up,
     which a "last success" check alone would miss.)
   - **Recovered**: a previously-stale device reporting success again
     clears the alert.

   The scanned set — "which devices ought to back up" — is the part to
   pin down in implementation: the natural definition is `Server`-role
   devices associated with a configured group, but it leans on the same
   `devices.server_id` / "device present" questions flagged below.
3. Alerts route through the existing operator notification path
   (Slack/email via `PRIVATE_URL`).

The limit of signal 1 is that it's the *device's word*: it scans the
Canopy database for what devices reported, not for what's actually in the
bucket. A device that crashes before reporting looks (safely) stale; a
compromised or buggy device could report success it didn't achieve; a
snapshot that the device believes it wrote but that never persisted reads
as healthy. Reports are timely but not authoritative.

### Signal 2 — repository inspection (authoritative, periodic)

Now that maintenance jobs already connect to each group's repository with
the password, Canopy can read the **ground truth**: the snapshots that
actually exist. A check — piggybacked on the maintenance job, or a
dedicated read-only job on the same footing — lists the repo's snapshots
and records, per source, the latest snapshot time. That's what *actually
landed in the bucket*, independent of whether any device reported, was
honest, or is even reachable.

- For this to attribute snapshots to devices, the kopia **source must
  encode the device** (e.g. fix the snapshot host to the device id) so
  Canopy can map `user@host:path` back to a device. The backup setup must
  pin this deterministically — flagged below.
- Reconciliation against signal 1 is where the value is:
  - report says success **but** no recent snapshot in the repo → the
    report is wrong or the upload didn't persist → **alert** (the case
    signal 1 alone cannot catch).
  - recent snapshot **but** no report → backups are fine, the reporting
    path is broken → lower-severity notice.
  - neither → genuinely stale (agrees with signal 1).
- It is *not* a replacement for signal 1's timeliness: it only sees the
  repo as often as the job runs (maintenance cadence), so an hourly-miss
  won't surface until the next inspection. Reports stay the day-to-day
  signal; repo inspection is the periodic trust anchor.
- Trust property: signal 2 depends only on the bucket (Object-Lock
  protected), not on any device, so it's the signal a compromised server
  cannot fake into showing a healthy backup.

Both signals feed the same alerting path.

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
  session policies above), so a compromised server cannot delete backups.

So Canopy owns maintenance. Concretely, a scheduled control-plane task
spawns a **Kubernetes Job per group** that runs `kopia maintenance run`
(quick on a short cadence, full less often) against that group's prefix,
then exits. Spawning a Job rather than running kopia in-process keeps the
heavy, long-running work off the Canopy pod and lets it use the kopia
image directly.

### Credentials for the maintenance Job

The Job assumes `canopy-backup-maintenance` (full S3 incl. delete on the
bucket). Crucially it gets this via **its own IRSA service account**, not
chained creds minted by Canopy. The reason is the 1-hour cap on chained
AssumeRole sessions noted up top: device backups tolerate it because
`credential_process` refreshes on demand, but a full-maintenance run can
exceed an hour, and we don't want it dying mid-rewrite. A direct IRSA
identity on the Job refreshes transparently with no cap.

Maintenance is first-party Canopy-controlled code, so granting it access
to the whole bucket pattern (rather than narrowing to the one group's
bucket via a session policy, which would re-introduce the 1-hour cap) is
an acceptable trade-off. Per-bucket narrowing is what protects against
*untrusted* device creds; it's not load-bearing for our own maintenance
job. (If we later want per-group blast-radius limits on maintenance too,
one role per bucket is the escape hatch — flagged, not built.)

### Interaction with the 30-day Object Lock

The buckets have a 30-day S3 Object Lock, which constrains maintenance:
kopia GC cannot actually delete a pack blob until its lock expires, so
storage from expired snapshots is reclaimed on a ~30-day lag, not
immediately. This is expected and is the price of ransomware-proof
backups — it must not be read as maintenance failing. Two implications
for implementation:

- The kopia repository must be created **object-lock-aware** (kopia
  supports this explicitly: it tracks retention and avoids trying to
  delete still-locked blobs). A repo created oblivious to the lock will
  throw errors when maintenance attempts deletes that S3 refuses.
- Budget for the lag: at any time the bucket holds up to ~30 days of
  not-yet-collectable garbage on top of live data. Fine, but worth
  noting for capacity/cost.

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

- Canopy generates the password when a group's backup config is first
  set up, and serves it to devices (alongside the target) so bestool can
  `kopia repository connect`, and injects it into maintenance Jobs.
- This is *not* a new exposure on the device side: any client writing to
  an encrypted kopia repo inherently holds the password — that's
  intrinsic to direct-to-S3 backup, not something maintenance adds.
- Storage of the password on the Canopy side is sensitive. Plaintext in
  the DB is the easy option but probably wrong; a k8s secret or AWS
  Secrets Manager reference held by `server_group_backup_config` is more
  appropriate. **Open question — decide during implementation.**
- The repo's maintenance *owner* is set to the Canopy maintenance
  identity, and client-side maintenance is disabled, so clients never
  attempt the maintenance they no longer have delete rights for.

### Audit

Maintenance runs are logged like device runs — a `backup_maintenance_runs`
table (root_server_id, kind quick|full, started/finished, outcome, error,
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

## Upstream access preflight

The device-staleness signals above watch the *devices*. This watches
**Canopy's own upstream access**, which is a different and higher-stakes
failure class: if the trust on the issuer role is edited, the pod's IRSA
annotation is dropped, a role is deleted, or a bucket's Object Lock is
removed, devices can't get creds and maintenance Jobs fail — and the
backups stop without any per-device fault to point at. We should learn
that from Canopy checking itself, not from devices starting to fail.

**It runs per server-group, not fleet-wide.** Each group has its own
bucket with its own `region`/`endpoint` and its own Object Lock config,
so bucket-level access can differ per group — and with a non-AWS-S3
`endpoint`, the cred pathway differs too (it isn't reached via AWS STS at
all). So a failure is normally *scoped to one group*; it's only fleet-wide
when the genuinely shared piece breaks. The preflight reflects that split:

Shared, checked once:
- **Identity resolves** — `sts:GetCallerIdentity` confirms the pod's IRSA
  web-identity is mounted and valid.
- **Issuer role admits Canopy** — a dry-run `AssumeRole` on the issuer
  role with a harmless session policy, confirming trust + role still
  exist. (When per-bucket roles arrive, this becomes per-pathway.)

Per configured group:
- **Group creds work** — mint creds scoped to *that group's* bucket/prefix
  and do a **read-only no-op** against *that group's* bucket/endpoint
  (e.g. `HeadBucket` / a scoped `ListBucket` on a probe prefix), so the
  check proves the group's creds actually work, not merely that
  `AssumeRole` returned. This is the check that catches a single group's
  bucket/endpoint/policy being wrong while others are fine.
- **Object Lock is in place** — verify *that group's* bucket still has the
  expected (≥30-day) Object Lock configuration. The whole "a compromised
  server can't destroy backups" guarantee rests on this; if someone
  removes or weakens the lock, the protection silently erodes with no
  other symptom, so it must be actively checked — per bucket, since the
  config is per bucket.
- **Maintenance path** — that group's maintenance Job already exercises
  its own pathway; either add a cheap preflight Job (connect + no-op and
  exit) on its own IRSA, or lean on `backup_maintenance_runs` failures for
  that group. The former is proactive; the latter only catches it when
  maintenance next runs. Flagged below.

Alerts name the affected group(s) and fire at control-plane severity
(distinct from per-device staleness); a check that fails for *every*
group points at the shared identity/issuer rather than any one bucket.

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

- **"Did device X back up today?"** — `backup_runs` gives the real
  answer (a successful run landed), and `backup_credential_issuances`
  the cheaper "creds were issued" proxy. Byte-level reconciliation
  against S3 inventory / CloudWatch still comes later.
- **Decommissioning a device** — revoke its mTLS cert (existing
  mechanism); it can no longer call the endpoint, so it can no longer
  get fresh creds. Already-issued creds expire within an hour.
- **Decommissioning a group** — delete its `server_group_backup_config`
  row; devices in that group start getting `409`. The group's bucket and
  its Object-Lock'd objects persist independently — they can't be deleted
  until their locks expire (~30 days), so bucket teardown is a deliberate,
  delayed step, not a side effect of removing the config row.
- **Onboarding a group** — provision its bucket (with Object Lock, see
  "Per-group buckets"), then insert the `server_group_backup_config` row.
  Devices in the group start succeeding on their next run.
- **Changing region or endpoint** — update the
  `server_group_backup_config` row. Each device picks it up on its next
  scheduled `bestool canopy backup` (every-run target fetch); no per-host
  command, no coordinated cutover. Staleness detection flags any host
  that fails to roll over. (This means pointing at a *different* bucket;
  since a kopia repo lives in its bucket, that's a repo migration or a
  start-fresh the operator owns — not a free flip — even though the config
  change itself propagates automatically.)
- **A compromised server can't destroy recent backups** — no device cred
  grants `DeleteObject`, and the 30-day Object Lock means even a delete-
  or overwrite-capable cred can't damage backup objects younger than 30
  days. IAM removes the action; Object Lock is the hard backstop.
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

## Open questions

None blocking, but flag-and-decide-during-implementation:

- **Bucket provisioning ownership.** Out-of-band (IaC / admin) vs.
  Canopy-driven `CreateBucket` (see "Per-group buckets"). Recommended
  out-of-band for the first cut to keep `CreateBucket` out of the request
  path; decide, and settle the bucket naming convention the IAM patterns
  key off. Whichever, Object Lock must be enabled at creation.
- **Per-device session naming for CloudTrail.** Suggested
  `device-<uuid>`; check max length and allowed chars on
  `RoleSessionName`.
- **`sts_request_id` storage.** AWS returns it; check the Rust SDK
  surface for retrieving it cleanly.
- **`devices.server_id` invariant.** Implementation needs to confirm
  every `Server`-role device has a server association; if not, the
  endpoint returns 409 and we file a separate issue for the
  data-consistency gap.
- **Staleness "device present" definition.** The "never backed up" check
  needs a notion of how long a device has existed in a configured group
  (first-seen / association timestamp) to avoid alerting on a host added
  minutes ago. Pick the existing timestamp to key off when implementing.
- **Grace factor for staleness.** `expected_interval × N` before
  alerting — pick N (and whether it's configurable per group) during
  implementation; start simple.
- **Default retention values.** The keep-* schedule for the `retention`
  field's default — a product decision, not invented here. Note the
  effective floor is 30 days regardless (Object Lock), so anything
  shorter is meaningless; the default should be ≥ that.
- **Heartbeat interval and last-success source.** The local timer's
  fixed interval (must be ≤ the smallest `expected_interval` in use),
  and whether bestool debounces against locally-recorded last-success or
  asks Canopy (which already has `backup_runs`). Asking Canopy is
  drift-proof but adds a round-trip; local state is cheaper but can lie
  after a host rebuild. Decide during implementation.
- **kopia source → device mapping.** Repository inspection (signal 2)
  needs to attribute each snapshot source back to a device, so the
  snapshot source must encode the device deterministically (e.g. host =
  device id). Pin this in the backup setup; settle where the per-source
  latest-snapshot inventory is stored (extend `backup_maintenance_runs`,
  or a dedicated table) and the inspection cadence if it's a job
  separate from maintenance.
- **Preflight depth, cadence, and per-group cost.** How often the
  upstream preflight runs, and whether it does the deeper per-group checks
  (S3 read no-op against a probe prefix; `GetBucketObjectLockConfiguration`)
  or just the shared `GetCallerIdentity` + dry-run `AssumeRole`. Deeper =
  more confidence, slightly more IAM surface. The per-group bucket checks
  scale with group count, so settle a cadence that stays cheap at fleet
  scale (and whether shared vs. per-group checks run on different
  cadences). Also: proactive maintenance preflight Job vs. relying on
  `backup_maintenance_runs` failures.
- **Repo password storage.** Canopy owns the per-group kopia password;
  where does the secret actually live (k8s secret vs. Secrets Manager
  vs. ...) and how is it served to devices and injected into Jobs?
  `repo_password_ref` is a reference, not the secret — settle the
  backing store during implementation.
- **Maintenance cadence.** Quick vs. full intervals — start with
  deployment-wide defaults; per-group override is a later column if
  needed. Decide the scheduler home (CronJob fanning out per-group Jobs,
  a private-server task, or a dedicated worker).
- **Maintenance Job scoping.** Broad-bucket IRSA (simple, no 1-hour cap)
  vs. one role per group (tighter blast radius, but per-group narrowing
  via session policy re-introduces the cap). Recommended: broad for the
  first cut since maintenance is first-party.

## Out of scope (do not silently fold in)

- Cross-group restore mechanism (explicitly disallowed by product
  decision).
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
- **Tuning** retention policy values, encryption-at-rest beyond default
  SSE-S3, S3 lifecycle rules. (Retention *execution* is in scope — Canopy
  maintenance runs it — but the kopia retention values start at a default
  and per-group tuning is later.)
- Operator UI in `private-server` / React for editing
  `server_group_backup_config` (will want it eventually; not in the
  first cut — bootstrap via SQL). This now includes the maintenance
  schedule and the repo-password reference, still bootstrapped via SQL /
  secret tooling for the first cut.
