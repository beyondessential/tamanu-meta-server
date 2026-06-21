# Ops/pulumi handoff — backup setup wizard + chained-AssumeRole cred model

Companion to `backup-setup-wizard.md`. This lists **only the ops/pulumi-side
changes** canopy needs. Canopy owns the Rust/UI/DB work; this is what the ops
agent must adjust. (canopy ticket TAM-6877; ops ticket TAM-6878.)

## Context

Canopy is moving the backup credential flow to **explicit chained
`sts:AssumeRole`** (there is no deployment-account OIDC provider, so the old
`AWS_ROLE_ARN`-override / direct-web-identity trick is gone), and adding an
interactive setup wizard that **probes the target bucket from private-server**
before a config is saved. private-server has no AWS identity today — that's the
main new ask.

## 1. New `canopy-private` ServiceAccount + IRSA role

private-server currently has no AWS identity. Add a **dedicated**
`canopy-private` SA (not a reuse of `canopy-jobs`/`canopy-issuer` — we want room
to grow private-server AWS features later):

- IRSA role annotated on the `canopy-private` SA.
- The role carries `sts:AssumeRole`.
- k8s RBAC for the SA: **`get` + `create` secrets** in the namespace (today
  private-server only needs `get`; the wizard now creates the passphrase Secret).

## 2. Trust-policy matrix (chained AssumeRole) — the main change

Verified current state in `pulumi/tamanu/on-linux/src/backup/kopia.ts`:

- device role: `assumeRolePolicyDocument: deviceAssumeRolePolicy(canopyIssuerRoleArn)`
  → trusts **canopy-issuer**.
- maintenance role: `maintenanceAssumeRolePolicy(canopyJobsRoleArn)` → trusts
  **canopy-jobs**.

Target trust per group:

| Per-group role | Trusted by (can `sts:AssumeRole` it) | Used for |
|---|---|---|
| **device role** (`deviceRoleArn` / `target_role_arn`) | `canopy-issuer` (existing) **+ `canopy-private` (NEW)** | mint device creds; wizard validation (`sts:get-caller-identity`) |
| **maintenance role** (`maintenanceRoleArn` / `maintenance_role_arn`) | `canopy-jobs` (existing) **+ `canopy-private` (NEW)** | maintenance/inspection/s3-metrics; wizard content-probe + connect-verify |

**Concrete change:** `deviceAssumeRolePolicy` / `maintenanceAssumeRolePolicy`
currently take a single trusted role ARN. Extend them to also trust a new
`canopyPrivateRoleArn` (add a `canopy.get('canopyPrivateRoleArn')` alongside the
existing `canopyIssuerRoleArn` / `canopyJobsRoleArn` config reads), and have the
canopy deployment stack export that ARN (next to `canopyIssuerRoleArn` /
`canopyJobsRoleArn`).

## 3. Maintenance role permissions — already correct, just confirm

Verified: the maintenance role already grants `s3:*` on the bucket **and**
`CLOUDWATCH_METRIC_ACTIONS` (the s3-metrics CloudWatch reads). No change needed —
this section is just to confirm canopy will now assume **this** role for
maintenance/inspection/s3-metrics.

> Bug being fixed canopy-side: maintenance/s3-metrics currently assume the
> *device* role (can't delete). They'll switch to `maintenance_role_arn`. The
> maintenance role is already complete, so no ops grant change — the device role
> stays minimal.

## 4. Session duration — **NO ops change** (MaxSessionDuration ask withdrawn)

Disregard the earlier "raise `canopy-jobs` `MaxSessionDuration` to 12h" note —
**withdrawn.** No `MaxSessionDuration` change is needed on any role.

Why it's moot: we verified (kopia v0.23.1 + minio-go v7.2.0 source) that kopia
cannot use `credential_process`/a creds file, **and** the `--role` approach
(which would have needed a long base session) is superseded. The chosen
mechanism is kopia's **IAM container-credentials endpoint**: canopy runs a tiny
localhost endpoint that mints a fresh (1h chained) maintenance-role session per
poll; kopia's minio-go re-polls at ~80% of lifetime, and the Rust SDK keeps the
pod's IRSA base fresh on its own. So a 90-min (or longer) run just re-polls — no
session ceiling, no role-duration tuning. Per-bucket roles stay 1h-capped
(fine); `canopy-jobs`/`canopy-private` need no duration change.

## 5. `.storageconfig` — informational, likely no change

Canopy will **create `<prefix>.storageconfig` as a fallback** during repo init
*only if absent*, and **never overwrites** an existing one, using the same
schema ops already writes (`blobOptions: p→INTELLIGENT_TIERING, else STANDARD`,
from `pulumi/tamanu/on-linux/src/backup/kopia.ts` and `pulumi/backups/index.ts`).
Since pulumi writes it at bucket creation, pulumi's object normally wins and
canopy's fallback is a no-op. No ops change required — just be aware canopy may
write it for buckets/prefixes pulumi didn't seed.

## 6. Config-as-a-resource API (so pulumi stops hand-copying ARNs)

Canopy will expose an API for pulumi to register a backup config as a managed
resource after it creates the bucket+roles — create/update/delete/get,
idempotent upsert. Ops side:

- **Auth: `TailscaleAdmin`** for now (pulumi already has tailnet access — call
  the private-server admin API over the tailnet). A proper non-interactive
  machine-auth path over Tailscale (tagged/ACL-grant) is wanted **later, not in
  this plan** — flag if ops wants to start designing it, but canopy isn't
  building it yet.
- **Inputs pulumi must supply per group:** `server_group_id`, `bucket`,
  `prefix`, `region`, `target_role_arn` (= `deviceRoleArn`), `maintenance_role_arn`
  (= `maintenanceRoleArn`), `mode` (machine flows: `from_birth` — canopy
  generates the passphrase; the human still escrows via the UI).
- **Delete** removes the config **and the canopy-owned passphrase Secret**.
- The create/update path runs the **same server-side access-check (the probe)**
  before persisting, so a misconfigured role/bucket fails fast.
- Exact request/response schema is canopy's to finalize; canopy will share the
  generated openapi. Ops only needs to confirm the **inputs above** are
  available as pulumi outputs (they are: `deviceRoleArn` + `maintenanceRoleArn`
  are already exported).

## 6a. Device path note (informational — bestool repo, not pulumi)

Heads-up that the device backup path is changing on the bestool side (TAM-6879),
not here: bestool will serve a localhost **container-credentials** endpoint that
kopia polls (fed by public-server creds), because we verified `credential_process`
doesn't work and ~90-min snapshot jobs make any <1h static-cred path non-viable.
This may prompt revisiting public-server's device-cred response shape. No pulumi
action — flagged only so the ops/bestool picture is consistent.

## 7. Not changing

- Device credential path *trust/roles* (public-server / `canopy-issuer`
  chain-assuming the device role) — unchanged (only the bestool-side cred
  *delivery* changes, §6a).
- The image still bundles kopia.

## Summary of ops action items

1. New `canopy-private` SA + IRSA role (`sts:AssumeRole`); SA RBAC `get`+`create`
   secrets; export its role ARN as `canopyPrivateRoleArn`.
2. Add `canopyPrivateRoleArn` to **both** `deviceAssumeRolePolicy` and
   `maintenanceAssumeRolePolicy` (they take a single ARN today).
3. Maintenance role perms — already `s3:*` + CloudWatch; nothing to change, just
   confirmed.
4. **No `MaxSessionDuration` change (ask withdrawn, §4)** — canopy uses kopia's
   container-credentials endpoint, which refreshes with no session ceiling. No
   `.storageconfig` change either.
5. Plan to call canopy's config-as-a-resource API over the tailnet
   (`TailscaleAdmin`) feeding `deviceRoleArn`+`maintenanceRoleArn`+bucket/prefix/
   region; delete cascades to the Secret. Schema TBD from canopy.

---

## Changelog (append-only — do NOT edit the body above after handoff)

**v1 — the version ops actioned** (everything above as of the first handoff). The
action items §1–§3, §5 are the source of truth; treat them as done.

**2026-06-20 — delta since v1 (nothing here needs new ops IAM/pulumi work):**
- **§4 reworded, net zero for ops.** v1 already said "no `MaxSessionDuration`
  change," and that's still true. The *reason* changed (canopy-internal): kopia
  now gets creds via a localhost **container-credentials endpoint** (verified
  against kopia 0.23.1 + minio-go 7.2.0), not `credential_process` and not
  `--role`+long-session. No role-duration tuning on any role.
  - ⚠️ **If anyone verbally relayed a "raise `canopy-jobs` `MaxSessionDuration`
    to 12h" ask (it was never in this doc), it is WITHDRAWN — ignore/revert it.**
- **§6a added — informational only, no pulumi action.** The device backup path
  moves to a bestool-served container-credentials endpoint (TAM-6879); may prompt
  revisiting public-server's device-cred response shape. Flagged for picture
  consistency only.
- **§1–§3, §5 unchanged** (byte-identical to v1).

**2026-06-20 — NEW ops action (passphrase rotation):**
- ⚠️ **`canopy-jobs` SA now needs WRITE on secrets** (`create`/`update`/`patch`,
  on top of the existing `get`). Why: the backups pod rotates each repo's
  passphrase regularly (forward protection) — after `kopia change-password` it
  writes the new passphrase back to the group's k8s Secret (dual-key
  `password`/`password_next`, server-side apply, field-manager `canopy-backups`).
  Read-only `get secrets` no longer covers the rotation path.
- No other ops change; rotation cadence is a canopy env
  (`CANOPY_BACKUP_ROTATION_DAYS`, default 7).

**2026-06-21 — NEW ops action (recovery vault):**
- ⚠️ **A new object-locked S3 bucket for the recovery vault, in a SEPARATE account**
  from both the canopy cluster account and the per-tenant backup accounts.
  Requirements:
  - **Object Lock = COMPLIANCE** + **versioning** on (so a Canopy compromise
    can't delete history; each daily write is a new immutable version of the
    same key). Pick a retention period (with a lifecycle expiry so it doesn't
    grow forever). SSE on.
  - A **writer role** the `canopy-jobs` SA assumes (chained AssumeRole), granted
    **`s3:PutObject` ONLY** on that bucket — **no delete, no get** (Canopy never
    reads the vault back; the blob is asymmetrically encrypted so it couldn't
    read it anyway).
- ⚠️ **age recipient keypairs (recovery-key custody).** Generate **multiple** age
  keypairs (e.g. one per recovery officer; `bestool crypto keygen`). The
  **public** keys go to Canopy via `CANOPY_RECOVERY_VAULT_KEYS` (space/comma-
  separated `age1…`); the **private** keys are held **offline, out-of-band**
  (any one can recover). Custody is an ops runbook — Canopy never sees a private
  key.
- **Canopy env (backups pod):** `CANOPY_RECOVERY_VAULT_KEYS` (**mandatory** — the
  pod refuses to start without it), `CANOPY_RECOVERY_VAULT_BUCKET` (**mandatory**),
  `CANOPY_RECOVERY_VAULT_REGION`, `CANOPY_RECOVERY_VAULT_ROLE_ARN` (the writer role),
  `CANOPY_RECOVERY_VAULT_SNAPSHOT_HOURS` (default 24). The object key/path within
  the bucket is not configurable (fixed at `canopy-recovery/state.age`). These
  must be provisioned **before** the backups pod is deployed with this build, or
  it will crash-loop on the mandatory check.
- **Verification ceremony (runbook):** operators run a yearly (and on-key-change)
  ceremony in the canopy admin UI (recovery vault page): Canopy issues an age-encrypted
  challenge, the operator decrypts it offline with a held private key
  (`bestool crypto decrypt`) and pastes it back. The vault blob itself is plain
  `age` v1 (decryptable with `bestool crypto decrypt` / `age` / `rage`).
- No k8s RBAC change (the vault is S3, not a Secret).

**2026-06-22 — clarification (private-server also needs the recipients):**
- ⚠️ **Set `CANOPY_RECOVERY_VAULT_KEYS` on private-server too**, not just the
  backups pod. They're **public** keys (non-secret), so use the same value. The
  private-server needs them to run the verification ceremony (issue the
  age-encrypted challenge); without them the recovery-vault page reports the
  ceremony as unavailable. private-server does **not** hard-require them (it
  starts fine without; only the ceremony page is degraded) — unlike the backups
  pod, which won't start without them. Nothing else on private-server needs it.
