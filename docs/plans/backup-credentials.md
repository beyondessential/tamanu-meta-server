# Backup credentials

Issue short-lived S3 credentials to devices on demand, so backups can run
without each server holding its own long-lived bucket credentials. Canopy
becomes the control plane; the actual backup bytes flow device → S3
directly (no proxy through Canopy).

## Goal

- One credential to manage per device — the existing mTLS identity.
- No long-lived AWS access keys on servers or in operator hands.
- Per-group isolation: a device only ever gets access to its own
  server-group's backup scope.
- Within-group cross-device restore: every device in a group can
  request read-only creds for the group's scope (`purpose=restore`), so
  restoring onto a freshly-rebuilt sibling is trivial and the restoring
  device can't accidentally damage the source backups.
- Cross-*group* restore is explicitly **not** supported.
- Every credential issuance is recorded so we can answer "did device X
  back up today, and when."

## Non-goals (for this plan)

- Serving S3/WebDAV/kopia-repository traffic through Canopy. Considered
  and rejected: bandwidth on the Canopy data path, availability coupling
  of every backup to Canopy uptime, and a much larger protocol surface.
- Cross-group restore. The endpoint signature can stay minimal because
  this is genuinely out of scope, not "deferred."
- Retention policy enforcement, cost dashboards, alerting on missed
  backups. The audit log this plan introduces is the foundation for
  those, but the features themselves come later.
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
```

Two STS calls per issuance:
1. The pod's IRSA session is implicit — kubernetes injects it.
2. From that session, Canopy calls `AssumeRole` on a dedicated
   `canopy-backup-issuer` role, passing a session policy that narrows
   S3 access to the requesting device's group prefix.

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
  policy grants broad S3 access to the backups bucket; the per-call
  session policy narrows that to the specific prefix. A bug in the
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
    bucket            TEXT NOT NULL,
    prefix            TEXT NOT NULL,           -- e.g. "groups/<root-id>/"
    region            TEXT,                    -- NULL → deployment default
    endpoint          TEXT,                    -- NULL → AWS; set for non-AWS S3
    expected_interval INTERVAL,                -- NULL → no staleness alerting
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

`region`/`endpoint` are what `GET /backup-target` serves to devices.
`expected_interval` drives staleness detection (below): if a group is
expected to back up daily, set it to `1 day` and Canopy alerts when a
device in the group has no recent successful run.

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

(`region`/`endpoint` can be added as columns to `server_group_backup_config`
when implementing, or derived from a single deployment-wide default if the
backups bucket always lives in one region. Decide during implementation;
the audit-log snapshot should capture whatever the device was actually
told.)

`purpose` is a real capability gate, not just audit metadata:

- `"backup"` (default): read + write + delete + multipart, scoped to
  the group's prefix. Kopia needs delete for snapshot expiry.
- `"restore"`: read-only (Get + ListBucket), scoped to the group's
  prefix. A device with these creds physically cannot mutate the
  bucket, so an accidental `kopia repository create` or similar can't
  damage backups.

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

**`purpose = "backup"`** — read + write + delete:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Action": [
        "s3:GetObject", "s3:PutObject", "s3:DeleteObject",
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

The `s3:ListBucket` condition prefix is what kopia needs to enumerate
its own snapshots; without it the device sees the entire bucket
listing. The restore variant omits all mutation actions — even an
explicit attempt to `kopia repository create` over the prefix is
rejected by S3, not just by kopia.

## AWS setup (out-of-band of this plan, but documented here for completeness)

- `canopy-backup-issuer` role with trust policy allowing the Canopy pod's
  IRSA role to assume it.
- Role policy grants broad `s3:*` on the backups bucket (broad because
  the session policy is what actually constrains each call).
- The backups bucket is created separately. Single bucket to start;
  the `bucket` column in `server_group_backup_config` means moving to
  one-bucket-per-group later is a data migration, not a code change.

## `bestool` changes

A new subcommand for the credential refresh, plumbed as kopia's
`credential_process`:

```
bestool backup-credentials [--purpose backup|restore]   # default: backup
```

- Reads the device's mTLS identity from its existing location.
- POSTs to `/backup-credentials` with the optional purpose.
- Writes the response JSON to stdout verbatim.
- Exits 0 on success, non-zero on any failure (the AWS SDK treats any
  non-zero exit as "creds unavailable").

And a driver subcommand that owns the kopia invocation so the device
holds no hardcoded bucket:

```
bestool backup [--purpose backup|restore]
```

- `GET /backup-target` to learn `{bucket, prefix, region, endpoint}`
  **on every run** — never cached to persistent device config.
- Reconciles the kopia repository connection against that target (so a
  changed bucket/prefix is picked up here), with
  `credential_process = bestool backup-credentials` for the creds.
- Runs the backup (or restore).
- Reports the run outcome back to Canopy (see "Backup reporting" below),
  so the control plane learns "backup completed", not just "creds
  issued".

The device is provisioned only with its Canopy URL and mTLS identity.
The bucket, prefix and region are never written to the device's
persistent config — bestool re-derives the repository connection from
Canopy on *every* run. This is the crux of the "no per-host action"
property: `bestool backup` is the scheduled job (systemd timer / cron)
that already runs on each host on its backup cadence, so a server-side
config change propagates to the whole fleet automatically on each host's
next scheduled backup. There is no operator command to run per host and
nothing to "forget to re-run".

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

Mechanism:
1. bestool reports each run via `POST /backup-report`, written to
   `backup_runs`.
2. A periodic Canopy job (alongside the existing health/alerting
   machinery — the same path that surfaces and recovers other device
   health signals) scans, per group with a non-NULL `expected_interval`:
   - **Stale**: a device that previously reported successful backups but
     has no `outcome = 'success'` row newer than `expected_interval`
     (times a small grace factor) → alert.
   - **Never backed up**: a device in a configured group that has been
     present longer than `expected_interval` and has *no* successful
     `backup_runs` row → alert. (Catches a host that was never wired up,
     which a "last success" check alone would miss.)
   - **Recovered**: a previously-stale device reporting success again
     clears the alert.
3. Alerts route through the existing operator notification path
   (Slack/email via `PRIVATE_URL`).

This is the half of "did device X back up today" that the audit log
alone can't give: `backup_credential_issuances` says creds were handed
out; `backup_runs` + this job says whether the backup landed and shouts
when it didn't.

A bucket change therefore can't silently break the fleet: if a host
fails to pick up the new target or fails to upload, its next expected
window lapses and Canopy alerts. The propagation itself is automatic
(every-run target fetch); detection is the backstop for when automatic
propagation doesn't take.

## Operational story (what we gain, day-one)

- **"Did device X back up today?"** — `backup_runs` gives the real
  answer (a successful run landed), and `backup_credential_issuances`
  the cheaper "creds were issued" proxy. Byte-level reconciliation
  against S3 inventory / CloudWatch still comes later.
- **Decommissioning a device** — revoke its mTLS cert (existing
  mechanism); it can no longer call the endpoint, so it can no longer
  get fresh creds. Already-issued creds expire within an hour.
- **Decommissioning a group** — delete its `server_group_backup_config`
  row; devices in that group start getting `409`.
- **Changing prefix, region, or endpoint** — update the
  `server_group_backup_config` row. Each device picks it up on its next
  scheduled `bestool backup` (every-run target fetch); no per-host
  command, no coordinated cutover. Staleness detection flags any host
  that fails to roll over.
- **Rotating the *bucket*** — note this is not a free config flip: a
  kopia repository lives *in* its bucket, so a new bucket is either a
  repo data migration or a deliberate start-fresh. The mechanism makes
  the *config* change propagate automatically, but the operator still
  owns the migration/cutover decision. (Listed under out-of-scope as
  per-group bucket migration.)

## Open questions

None blocking, but flag-and-decide-during-implementation:

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
- Per-group bucket migration (the schema supports it; the operational
  work is separate).
- bestool subcommand implementation (separate repo).
- Backup retention, encryption-at-rest beyond default SSE-S3, lifecycle
  policies (separate concerns).
- Operator UI in `private-server` / React for editing
  `server_group_backup_config` (will want it eventually; not in the
  first cut — bootstrap via SQL).
