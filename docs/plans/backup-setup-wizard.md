# Backup setup wizard + Canopy-owned passphrases + chained-AssumeRole cred model

Status: **PLAN / for review** (2026-06-20). Supersedes parts of the cred model
in `backup-credentials.md` (the direct-web-identity scheme) — see §1.

## Why

Two things landed together:

1. **Operator feedback (feature):** the backup setup form should become an
   interactive wizard. The operator enters bucket/prefix/role(s)/region; Canopy
   *immediately* uses those creds to probe the bucket — verifying the creds work
   and reporting whether the prefix is empty, looks like an existing kopia repo,
   holds other (forgotten) content, or is already configured in Canopy — and
   offers next steps based on that. Only once the passphrase situation is
   settled do we collect schedule/retention. Rework repo modes so **Canopy owns
   every passphrase**: `from_birth` (generate + escrow) or `passphrase` (operator
   types it); drop the old import-an-existing-Secret mode.

2. **Ops-driven cred-model change:** there is **no deployment-account OIDC
   provider**, so the previous "override `AWS_ROLE_ARN` + reuse the projected
   web-identity token → direct `AssumeRoleWithWebIdentity`" scheme is gone.
   Everything cross-account is now **explicit chained `sts:AssumeRole`** from the
   pod's own IRSA creds, and there are now **two roles per group**.

Both touch the same credential plumbing, so they're planned together.

---

## 1. Credential model change (ops-driven)

### 1.1 Two roles per group

`server_group_backup_config` carries **both**:

- `target_role_arn` — **device role** (`deviceRoleArn`). No delete. public-server
  mints device creds from it. **Unchanged.**
- `maintenance_role_arn` — **maintenance role** (`maintenanceRoleArn`). `s3:*` +
  delete + CloudWatch. The backups pod assumes this for
  maintenance / inspection / s3-metrics. **New column.**

The current code assuming `target_role_arn` for maintenance / s3-metrics is a
**bug** (the device role deliberately can't delete). Fixing it is in scope here.

### 1.2 Chained AssumeRole everywhere cross-account

The backups pod keeps its own `canopy-jobs` IRSA creds (default credential
chain). For each group op it needs `sts:AssumeRole(maintenance_role_arn)`. Two
consumers:

- **CloudWatch SDK client (s3-metrics):** in-process Rust `aws-sdk-sts` assume →
  hand the temp creds to the CloudWatch client (refreshable provider).
- **kopia subprocess:** instead of baking static temp creds into the env (which
  expire mid-run, see §1.3), kopia fetches creds **just-in-time via
  `credential_process`** (§1.3).

Drop every direct-web-identity-against-deployment-account assumption, and drop
the old `AWS_ROLE_ARN`-override path in `kopia.rs`.

### 1.3 Just-in-time creds for kopia (`credential_process`) — solves the 1h cap

Chaining caps each assumed session at **1 hour** regardless of the role's
`MaxSessionDuration`, so static env creds would break long maintenance runs.
kopia uses the AWS SDK, which supports `credential_process`: a profile entry
naming a helper that prints `{Version:1, AccessKeyId, SecretAccessKey,
SessionToken, Expiration}` and is **re-invoked on demand** when creds near
expiry.

**Plan:** ship a creds helper as a subcommand of the backups bin (e.g.
`backups creds --role <maintenance_role_arn> [--region]`) that does
`sts:AssumeRole` via the pod's default credential chain and prints the
credential-process JSON. Point each kopia subprocess at it via a generated AWS
profile (`AWS_CONFIG_FILE`/`AWS_PROFILE` or `AWS_SHARED_CREDENTIALS_FILE` with
`credential_process = backups creds --role …`). kopia transparently re-invokes
it past the 1h mark — no static-cred expiry, no re-exec dance. This mirrors the
just-in-time pattern bestool will use device-side (there it calls canopy because
the device has no AWS identity; here the helper assumes directly since the pod
*does* have IRSA). A localhost-endpoint variant is possible but the direct
helper binary is simpler and needs no server.

### 1.4 Device path: unchanged

public-server / canopy-issuer already chain-assumes `target_role_arn` and hands
creds to the device over mTLS. No change.

### 1.5 What ops provides (so canopy just uses the default chain → AssumeRole)

- `canopy-issuer` + `canopy-jobs` SAs annotated with IRSA role ARNs, both
  carrying `sts:AssumeRole` + `get secrets`.
- Per-bucket roles trust the matching SA role ARN.
- Image still bundles kopia.

> **Ops coordination (TAM-6878):** the wizard's synchronous probe runs in
> **private-server**, which today has no AWS identity. **Decided:** private-server
> gets a **dedicated `canopy-private` SA + IRSA role** (room to grow more
> private-server AWS features later), carrying `sts:AssumeRole`; the per-bucket
> roles' trust policies must include this role ARN, and the SA needs `create
> secrets` (§3) on top of the existing `get secrets`.

---

## 2. Interactive setup wizard (probe)

### 2.1 Flow

Step 1 — **Identity & target.** Operator enters: bucket, prefix, region (default
**`ap-southeast-2`** — most buckets live there), `target_role_arn` (device),
`maintenance_role_arn`. → **Probe.**

Step 2 — **Probe result & passphrase.** Canopy assumes the role and inspects the
prefix. Based on the result (§2.3) it presents the right passphrase choice
(from_birth generate vs operator-typed passphrase). Probe also reports if this
bucket+prefix is already configured in Canopy (DB check, §2.4). For an existing
repo, once the operator types the passphrase Canopy runs a **second
(verify) probe** that attempts `kopia repository connect` to confirm the
passphrase before committing (§2.3).

Step 3 — **Schedule & retention.** Only reached once the passphrase situation is
settled. Same fields as today (interval + per-type retention with the org
floors). Then create + provision.

### 2.2 Probe endpoint

New private-server endpoint, e.g. `POST /api/backups/probe`. Two phases share it:
an **inspect** phase (no passphrase) and a **verify** phase (with passphrase, for
existing repos):

```
ProbeArgs {
  bucket, prefix, region, target_role_arn, maintenance_role_arn,
  passphrase: Option<String>,     // present ⇒ also run the connect-verify
}
ProbeResult {
  creds_ok: bool,
  error: Option<String>,          // assume/list failure surfaced verbatim-ish
  state: "empty" | "kopia_repo" | "other_content" | "inaccessible",
  object_sample: Vec<String>,     // a few keys, for "other content" context
  already_configured_in_canopy: Option<Uuid>,  // group id if bucket+prefix taken
  passphrase_ok: Option<bool>,    // set only when a passphrase was supplied
}
```

Implementation: add `aws-sdk-sts` + `aws-sdk-s3` + `aws-config` to private-server
(mirrors `public-server/src/backup.rs` and `jobs/src/backup/preflight.rs`).
Assume the **`maintenance_role_arn`** (full read; it's the path that does the
heavy lifting, so validating it is the most useful signal), then:

- `ListObjectsV2(bucket, prefix, max-keys=small)`.
- Probe for the kopia repo marker `HeadObject`/`GetObject` on
  `<prefix>kopia.repository` (confirmed: kopia 0.23.1 writes its format blob
  there). Present ⇒ `kopia_repo`.
- **`.storageconfig`-only counts as empty:** if the only object(s) under the
  prefix are `.storageconfig` (and no `kopia.repository`), treat as `empty`.
- Non-empty (beyond `.storageconfig`) with no marker ⇒ `other_content` (return a
  sample of keys).
- Assume/list failure ⇒ `creds_ok=false`, `inaccessible`, surface the error.
- If `passphrase` supplied and state is `kopia_repo`: attempt `kopia repository
  connect` with it (using the credential-process helper for S3 creds) →
  `passphrase_ok`. (Connect leaves no writes.)

Also cheaply validate `target_role_arn` with `sts:get-caller-identity` under the
assumed session so a bad device role is caught at setup, not at first device
mint.

### 2.3 State → offered options

| Probe state | What we show |
|---|---|
| `empty` | Proceed. Mode: **from_birth** (generate + escrow, recommended) or **passphrase** (type your own). |
| `kopia_repo` | An existing kopia repo. Only **passphrase** mode (operator provides the existing passphrase) — *not* from_birth (won't create over an existing repo). The verify-probe must return `passphrase_ok` before the operator can continue. |
| `other_content` | **Block** with a warning + sample of keys. Canopy does **not** delete anything — the operator clears the contents themselves (or chooses a different prefix/bucket) and hits **Retry** to re-probe. No "proceed anyway", no Canopy-side delete. |
| `inaccessible` | Block step 1; show the assume/list error so the operator can fix the role/bucket/region. |

### 2.4 Already-configured-in-Canopy check

Before/with the probe, query whether `(bucket, prefix)` (or the group) already
has a `server_group_backup_config`. If so, surface it (link to the existing
config) and block creating a duplicate. Pure DB check, no creds needed.

### 2.5 `other_content` → Retry only (no Canopy-side delete)

Canopy never deletes bucket contents. On `other_content` the wizard blocks and
shows a **Retry** button (re-runs the probe); the operator must clear the prefix
/ pick another prefix or bucket on their own (AWS console, etc.). No delete
endpoint, no `s3:DeleteObject*` grant needed.

---

## 3. Repo modes + private-server-owned Secret creation

### 3.1 Two modes only — drop import-by-Secret

**Decision:** **Canopy owns all repo passphrases.** Remove the existing
`import` (operator-supplies-a-Secret-name) variant entirely. `BackupRepoMode`
becomes exactly two variants:

- `from_birth` — Canopy generates the passphrase + escrow flow (reveal-once +
  ack). Only valid on an `empty` prefix.
- `passphrase` — operator provides the passphrase; Canopy stores it. **Skips
  escrow** → `provisioning → ready` on successful init. Covers *both* "set my
  own on a fresh repo" (empty prefix → create) and "connect to an existing repo"
  (`kopia_repo` → connect, passphrase pre-verified by the §2.2 verify probe). The
  repo *state*, not the mode, decides create-vs-connect.

DB migration changes the `CHECK` on `server_group_backup_config.mode` from
`IN ('from_birth','import')` to `IN ('from_birth','passphrase')`. No existing
rows (confirmed), so no data migration. Remove `import`-specific handling
(`repo_password_ref` is no longer a user input — Canopy always names/owns the
Secret) and the `BackupRepoMode::Import` match arms.

### 3.2 Secret creation — currently missing

**Gap found:** nothing in the codebase *creates* the passphrase Secret today.
`from_birth` init only ever *reads* it (`worker.read_repo_password`), and there
is no passphrase generation — so `from_birth` is not actually wired end-to-end.

**Decision (confirmed):** private-server owns Secret creation for both modes,
at config-create time:

- `from_birth`: generate a strong passphrase, create the k8s Secret
  (`backup-repo-{group_id}`, key `password`), record the ref. Escrow flow
  unchanged.
- `passphrase`: create the Secret from the operator-typed value, ref recorded,
  no escrow.

This gives the `canopy-private` SA `create secrets` RBAC (today it has `get`
only). The `backups` init loop keeps only *reading* the Secret — no change.

> Scope note (rule_no_self_scoping): finishing `from_birth` generate+create is
> pulled in here because both modes share the same (missing) machinery; calling
> it out rather than silently bundling or dropping it.

### 3.3 `.storageconfig` (Intelligent-Tiering) on init

Normally pulumi writes `.storageconfig` at bucket creation
(`ops/pulumi/tamanu/on-linux/src/backup/kopia.ts` + `ops/pulumi/backups/index.ts`).
Canopy creates it as a **fallback**: on repo init, if `<prefix>.storageconfig`
is **absent**, write it; **never overwrite** an existing one. A prefix
containing only `.storageconfig` is treated as `empty` by the probe (§2.2), so a
pre-seeded tiering config doesn't block from_birth.

Use the exact schema ops uses (kopia `blobOptions` — data blobs under the `p`
prefix → Intelligent-Tiering, everything else → Standard so indexes stay in the
frequent tier), written as pretty JSON, `Content-Type: application/json`:

```json
{
  "blobOptions": [
    { "prefix": "p", "storageClass": "INTELLIGENT_TIERING" },
    { "storageClass": "STANDARD" }
  ]
}
```

The `p` prefix is relative to the kopia storage root, so the same content is
correct whether or not the repo lives under a canopy prefix; place the object at
`<prefix>.storageconfig`.

---

## 3a. Machine-facing config-as-a-resource API (ops/pulumi)

Complements the wizard (does **not** replace it). Pulumi creates the bucket +
device/maintenance roles, then pushes the backup config to Canopy as a managed
resource — so operators don't hand-copy ARNs out of pulumi.

- **Endpoints:** create / update / delete / get a `server_group_backup_config`
  (the wizard's create/update reuse the same handlers). Create/update run the
  **same server-side access-check/probe** (§2.2) before persisting, so a config
  pushed by pulumi is validated identically — bad creds/role/bucket fail fast.
- **Resource semantics:** idempotent upsert keyed by group (or bucket+prefix),
  suitable for a Pulumi dynamic provider / `Command`-style resource. **Delete
  tears down the config *and* deletes the Canopy-owned passphrase Secret** (both
  modes — Canopy owns it now).
- **Auth (decided):** gate on `TailscaleAdmin` for now — pulumi has tailnet
  access. A proper non-interactive machine path (still over Tailscale, via
  tagged/ACL-grant access) is wanted **later but explicitly out of scope here**.

---

## 4. Work breakdown

### DB (database crate, migration via `just migration`)
- Add `maintenance_role_arn TEXT NOT NULL` to `server_group_backup_config` (no
  existing rows, so NOT NULL is clean) + model field + re-exports.
- Change `mode` CHECK from `IN ('from_birth','import')` to
  `IN ('from_birth','passphrase')`.

### commons-types
- `BackupRepoMode`: replace `Import` with `Passphrase` (`"passphrase"`).

### private-server (`fns/backups.rs`, `state.rs`)
- AWS deps (`aws-sdk-sts`/`aws-sdk-s3`/`aws-config`); `probe` endpoint (inspect +
  verify phases, §2.2); `already_configured` DB check. No delete-contents
  endpoint — `other_content` is Retry-only (§2.5).
- Secret creation on `create` for from_birth + passphrase (§3.2); extend the
  kube wrapper with a create op; `create secrets` RBAC.
- `CreateBackupConfigArgs`: add `maintenance_role_arn`; accept the typed
  passphrase for `passphrase` mode; drop `repo_password_ref` as a user input and
  the `Import` arms.
- **Config-as-a-resource API (§3a):** create/update/delete/get usable by
  pulumi, sharing the access-check; resolve the machine-auth path.
- openapi regen (`just gen-openapi`).

### jobs (`backup/{kopia,worker,maintenance,inspection,s3_metrics}.rs`, bin)
- Switch maintenance/inspection/s3-metrics to **chain-assume
  `maintenance_role_arn`** (fix the device-role bug). CloudWatch client uses the
  in-process assumed creds.
- **`credential_process` helper (§1.3):** add a `backups creds --role …`
  subcommand; generate the per-subprocess AWS profile pointing kopia at it;
  remove the `AWS_ROLE_ARN`-override path in `kopia.rs`.
- **`.storageconfig` on init (§3.3):** create if absent, never overwrite.

### frontend (`private-web/`)
- `BackupConfig.tsx` → multi-step wizard (step 1 identity+probe with both role
  ARNs + region default `ap-southeast-2`; step 2 probe result + passphrase +
  verify-probe for existing repos; step 3 schedule/retention). Render the
  state→options matrix (§2.3); already-configured; `other_content` blocking with
  a Retry button (no delete).
- Generated api-types (`just gen-openapi`).

### tests
- Rust: probe endpoint (inspect + verify; mock/seed S3 where feasible), secret
  creation, two-mode status machine, migration, the resource API.
- Playwright e2e: wizard steps, region default, probe states (the e2e kube/AWS
  clients are `None` today → probe needs a test seam to stub responses),
  passphrase-mode flow, from_birth escrow flow.

### cross-repo / ops (TAM-6878 pulumi)
- New `canopy-private` SA + IRSA role; per-bucket trust includes it; `create
  secrets` RBAC.
- `maintenanceRoleArn` per group (CloudWatch + delete grant on it); pulumi calls
  the new resource API to register configs (§3a).

---

## 5. Decisions (all resolved)

Resolved: dedicated `canopy-private` SA; probe assumes
`maintenance_role_arn` (+ cheap `target_role_arn` validate); `maintenance_role_arn`
NOT NULL (no existing rows); passphrase mode straight to `ready` (no escrow);
`other_content` hard-blocks, Retry-only (Canopy never deletes contents);
import-by-Secret dropped;
`credential_process` for kopia; `kopia.repository` marker confirmed.

Also resolved this round: pulumi→private-server auth = `TailscaleAdmin` gate
(pulumi has tailnet access; machine/tagged-grant auth is a later, out-of-scope
follow-up); deleting a config also deletes the Canopy-owned Secret;
`.storageconfig` schema taken verbatim from the ops repo (§3.3); e2e probe is
stubbed/mocked (whatever's simplest — the e2e AWS/kube clients are `None`, so the
probe path gets a test seam returning canned `ProbeResult`s; no real S3).

**Nothing blocking remains** — ready to implement on plan approval.
