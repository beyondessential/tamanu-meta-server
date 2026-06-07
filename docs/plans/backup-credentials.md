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
    root_server_id  UUID PRIMARY KEY REFERENCES servers(id) ON DELETE CASCADE,
    bucket          TEXT NOT NULL,
    prefix          TEXT NOT NULL,           -- e.g. "groups/<root-id>/"
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

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

This is the audit log that "did device X back up today" queries against.
We snapshot bucket/prefix at issuance time so the log stays correct
even if the group config is later changed.

## Endpoint shape

Two endpoints, both `ServerDevice`-authenticated and both resolving the
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

- `GET /backup-target` to learn `{bucket, prefix, region, endpoint}`.
- Connects/creates the kopia repository against that target, with
  `credential_process = bestool backup-credentials` for the creds.
- Runs the backup (or restore).

The device is provisioned only with its Canopy URL and mTLS identity.
The bucket, prefix and region are never written to the device's
persistent config — bestool re-derives the repository connection from
Canopy on each run, so a server-side config change takes effect on the
next backup with zero device reconfiguration.

bestool work is in a separate repo; this plan covers the Canopy side
and the bestool side will be a sibling change there.

## Operational story (what we gain, day-one)

- **"Did device X back up today?"** — query
  `backup_credential_issuances` for the device, with a recent
  `issued_at`. Not a perfect proxy (creds issued ≠ bytes uploaded) but
  it's the cheap version. Real "bytes uploaded" comes from S3
  inventory / CloudWatch later.
- **Decommissioning a device** — revoke its mTLS cert (existing
  mechanism); it can no longer call the endpoint, so it can no longer
  get fresh creds. Already-issued creds expire within an hour.
- **Decommissioning a group** — delete its `server_group_backup_config`
  row; devices in that group start getting `409`.
- **Rotating bucket / changing prefix** — update the
  `server_group_backup_config` row. Existing creds keep working until
  expiry, then refresh with the new config. No coordinated cutover.

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

## Out of scope (do not silently fold in)

- Cross-group restore mechanism (explicitly disallowed by product
  decision).
- Data-plane proxy / Canopy-served S3/WebDAV (explicitly rejected).
- Per-group bucket migration (the schema supports it; the operational
  work is separate).
- bestool subcommand implementation (separate repo).
- Backup retention, encryption-at-rest beyond default SSE-S3, lifecycle
  policies (separate concerns).
- Operator UI in `private-server` / React for editing
  `server_group_backup_config` (will want it eventually; not in the
  first cut — bootstrap via SQL).
