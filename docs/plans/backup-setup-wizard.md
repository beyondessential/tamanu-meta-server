# Backup setup wizard + third repo mode + chained-AssumeRole cred model

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
   settled do we collect schedule/retention. Add a **third repo mode**: type the
   passphrase directly (vs `from_birth` generate, vs `import` an existing
   Secret).

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
chain) and, per group op, calls `sts:AssumeRole(maintenance_role_arn)`, then:

- passes the temp creds to the **kopia subprocess** via
  `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` / `AWS_SESSION_TOKEN` (+`AWS_REGION`)
  — **not** `AWS_ROLE_ARN` + web-identity-token-file;
- uses the same assumed creds for the **CloudWatch SDK client** (s3-metrics).

Drop every direct-web-identity-against-deployment-account assumption.

### 1.3 1-hour session cap

Chaining caps the assumed session at **1 hour** regardless of the role's
`MaxSessionDuration`. Maintenance / inspection runs that can exceed an hour must
re-assume before expiry. **Recommendation:** assume fresh creds immediately
before each kopia invocation (most ops are well under 1h); for a single op that
could run long, write the creds to a file kopia re-reads (kopia S3 storage reads
standard AWS creds; a refreshable credentials file is the least-invasive path).
Track as an implementation detail; start with per-op fresh assume.

### 1.4 Device path: unchanged

public-server / canopy-issuer already chain-assumes `target_role_arn` and hands
creds to the device over mTLS. No change.

### 1.5 What ops provides (so canopy just uses the default chain → AssumeRole)

- `canopy-issuer` + `canopy-jobs` SAs annotated with IRSA role ARNs, both
  carrying `sts:AssumeRole` + `get secrets`.
- Per-bucket roles trust the matching SA role ARN.
- Image still bundles kopia.

> **Ops coordination (TAM-6878):** the wizard's synchronous probe runs in
> **private-server**, which today has no AWS identity. private-server must run
> under an IRSA SA whose role ARN the per-bucket roles trust (and which carries
> `sts:AssumeRole`), plus `create secrets` RBAC (§3). Confirm which SA
> private-server uses — reuse `canopy-jobs`/`canopy-issuer`, or a dedicated
> `canopy-private` role added to the per-bucket trust policies.

---

## 2. Interactive setup wizard (probe)

### 2.1 Flow

Step 1 — **Identity & target.** Operator enters: bucket, prefix, region (default
**`ap-southeast-2`** — most buckets live there), `target_role_arn` (device),
`maintenance_role_arn`. → **Probe.**

Step 2 — **Probe result & passphrase.** Canopy assumes the role and inspects the
prefix. Based on the result (§2.3) it presents the right passphrase choice
(from_birth / type-passphrase / import). Probe also reports if this bucket+prefix
is already configured in Canopy (DB check, §2.4).

Step 3 — **Schedule & retention.** Only reached once the passphrase situation is
settled. Same fields as today (interval + per-type retention with the org
floors). Then create + provision.

### 2.2 Probe endpoint

New private-server endpoint, e.g. `POST /api/backups/probe`:

```
ProbeArgs { bucket, prefix, region, target_role_arn, maintenance_role_arn }
ProbeResult {
  creds_ok: bool,
  error: Option<String>,          // assume/list failure surfaced verbatim-ish
  state: "empty" | "kopia_repo" | "other_content" | "inaccessible",
  object_sample: Vec<String>,     // a few keys, for "other content" context
  already_configured_in_canopy: Option<Uuid>,  // group id if bucket+prefix taken
}
```

Implementation: add `aws-sdk-sts` + `aws-sdk-s3` + `aws-config` to private-server
(mirrors `public-server/src/backup.rs` and `jobs/src/backup/preflight.rs`).
Assume the **`maintenance_role_arn`** (full read; it's the path that does the
heavy lifting, so validating it is the most useful signal), then:

- `ListObjectsV2(bucket, prefix, max-keys=small)` → empty vs non-empty.
- Probe for a kopia repo marker: `HeadObject`/`GetObject` on
  `<prefix>kopia.repository` (kopia's format blob). Present ⇒ `kopia_repo`.
- Non-empty but no marker ⇒ `other_content` (return a sample of keys).
- Assume/list failure ⇒ `creds_ok=false`, `inaccessible`, surface the error.

(Optionally also cheaply validate `target_role_arn` with `sts:get-caller-identity`
under the assumed session so a bad device role is caught at setup, not first
device mint. Recommend: yes, it's cheap.)

> kopia repo-marker key needs a quick verification against the real 0.23.1
> layout (is it exactly `kopia.repository` at the prefix root?). Confirm before
> coding the detector.

### 2.3 State → offered options

| Probe state | What we show |
|---|---|
| `empty` | Proceed normally. Passphrase mode: from_birth (recommended) or type-your-own; import is N/A (nothing to import). |
| `kopia_repo` | An existing kopia repo is here. Offer **import** (supply/type the passphrase; we verify by connecting) — *not* from_birth (would refuse to create over an existing repo). Verify the passphrase by attempting a `kopia repository connect` before committing. |
| `other_content` | **Warn**: the prefix holds non-kopia objects (show sample). Require explicit confirmation to proceed (don't clobber); recommend a different prefix. |
| `inaccessible` | Block step 1; show the assume/list error so the operator can fix the role/bucket/region. |

### 2.4 Already-configured-in-Canopy check

Before/with the probe, query whether `(bucket, prefix)` (or the group) already
has a `server_group_backup_config`. If so, surface it (link to the existing
config) and block creating a duplicate. Pure DB check, no creds needed.

---

## 3. Third repo mode + private-server-owned Secret creation

### 3.1 New mode

`BackupRepoMode` gains a third variant (commons-types `text_enum!`), e.g.
`Passphrase = "passphrase"` (operator types the passphrase directly). DB
migration must extend the `CHECK (mode IN (...))` on
`server_group_backup_config.mode` to include it.

Status machine: like `from_birth` it produces a Canopy-held Secret, but it
**skips escrow** (the operator already knows the passphrase) → goes
`provisioning → ready` on successful init (same as import), not via
`escrow_pending`. (Confirm: do we still want a one-time "we stored it"
confirmation, or straight to ready? Recommend straight to ready.)

### 3.2 Secret creation — currently missing

**Gap found:** nothing in the codebase *creates* the passphrase Secret today.
`from_birth` init only ever *reads* it (`worker.read_repo_password`), and there
is no passphrase generation. So `from_birth` is not actually wired end-to-end,
and the new mode needs creation too.

**Decision (confirmed):** private-server owns Secret creation for **both**:

- `from_birth`: generate a strong passphrase, create the k8s Secret
  (`backup-repo-{group_id}`, key `password`), record the ref. Escrow flow
  unchanged (reveal-once + ack).
- `passphrase` (new): create the Secret from the operator-typed value, ref
  recorded, no escrow.
- `import`: unchanged (operator supplies an existing Secret name).

This gives private-server `create secrets` RBAC (today it has `get` only). The
`backups` init loop keeps only *reading* the Secret — no change there. Creating
the Secret at config-create time (private-server) also means the probe's
existing-repo "verify by connect" can use the just-supplied passphrase.

> Scope note (rule_no_self_scoping): finishing `from_birth` generate+create is
> pulled in here because the new mode shares the same missing machinery; calling
> it out rather than silently bundling or dropping it.

---

## 4. Work breakdown

### DB (database crate, migration via `just migration`)
- Add `maintenance_role_arn TEXT` to `server_group_backup_config` (+ model field,
  re-exports). Backfill/repurpose: existing rows have only `target_role_arn`;
  decide default (likely NOT NULL with no default → set in a follow-up deploy, or
  nullable first per the migration-meaning rule — see open Q).
- Extend `mode` CHECK to include the new variant.

### commons-types
- `BackupRepoMode` third variant.

### private-server (`fns/backups.rs`, `state.rs`)
- AWS deps; `probe` endpoint (§2.2); `already_configured` DB check.
- Secret creation on `create` for from_birth + passphrase (§3.2); extend
  `BackupSecrets` (or a sibling) with a create op; `create secrets` RBAC.
- `CreateBackupConfigArgs`: add `maintenance_role_arn`, accept typed passphrase
  for the new mode, keep `repo_password_ref` for import.
- openapi regen (`just gen-openapi`) for new/changed request+response bodies.

### jobs (`backup/{kopia,worker,maintenance,inspection,s3_metrics}.rs`)
- Switch maintenance/inspection/s3-metrics to **chain-assume
  `maintenance_role_arn`** via `aws-sdk-sts` and pass temp creds to kopia env
  (`AWS_ACCESS_KEY_ID/SECRET/SESSION_TOKEN`+`AWS_REGION`) and the CloudWatch
  client (§1.2). Remove the `AWS_ROLE_ARN`-override path in `kopia.rs`.
- 1h-cap handling (§1.3): assume fresh per op.
- Fix the device-role-for-maintenance bug (use `maintenance_role_arn`).

### frontend (`private-web/`)
- `BackupConfig.tsx` → multi-step wizard (step 1 identity+probe, step 2
  passphrase/probe-result, step 3 schedule/retention). New role-ARN field;
  region defaults to `ap-southeast-2`. Wire the probe endpoint; render the
  state→options matrix (§2.3); show already-configured.
- Generated api-types (`just gen-openapi`).

### tests
- Rust: probe endpoint (mock/seed; assume + list behaviour where feasible),
  secret-creation path, new-mode status machine, migration.
- Playwright e2e: wizard steps, region default, probe states (stub the probe
  response where AWS isn't reachable in e2e — the e2e kube/AWS clients are
  None today, so probe needs a test seam), new-mode flow.

### cross-repo / ops (TAM-6878 pulumi)
- private-server IRSA SA + per-bucket trust + `create secrets` RBAC.
- Confirm `maintenanceRoleArn` is what pulumi emits per group; CloudWatch grant
  on the maintenance role.

---

## 5. Open decisions (for review)

1. **private-server's SA / trust** (§1.5): reuse `canopy-jobs`/`canopy-issuer`,
   or a dedicated `canopy-private` role? Drives the pulumi trust-policy change.
2. **Which role the probe assumes**: recommend `maintenance_role_arn` (full
   read), + cheap `target_role_arn` validation. OK?
3. **`maintenance_role_arn` migration shape**: nullable-first then backfill, or
   NOT NULL with a value supplied at create? (existing rows + the
   reinterpret-column rule.)
4. **New-mode status**: straight to `ready` (no escrow), or a one-time "stored"
   confirmation?
5. **`other_content` handling**: hard-block vs warn-and-confirm. Recommend
   warn-and-confirm + suggest a different prefix.
6. **kopia repo marker key**: confirm `kopia.repository` at prefix root for
   0.23.1 before building the detector.
