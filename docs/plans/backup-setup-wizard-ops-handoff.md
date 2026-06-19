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

## 4. Session duration

Chained AssumeRole caps each session at **1 hour** regardless of the role's
`MaxSessionDuration` — so **don't bother raising `MaxSessionDuration`** (e.g. to
43200); it's a no-op for this path. Canopy handles long maintenance runs by
re-fetching creds just-in-time (kopia `credential_process`), so no ops action
needed here — informational.

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

## 7. Not changing

- Device credential path (public-server / `canopy-issuer` chain-assuming the
  device role, handing creds to the device over mTLS) — unchanged.
- The image still bundles kopia.

## Summary of ops action items

1. New `canopy-private` SA + IRSA role (`sts:AssumeRole`); SA RBAC `get`+`create`
   secrets; export its role ARN as `canopyPrivateRoleArn`.
2. Add `canopyPrivateRoleArn` to **both** `deviceAssumeRolePolicy` and
   `maintenanceAssumeRolePolicy` (they take a single ARN today).
3. Maintenance role perms — already `s3:*` + CloudWatch; nothing to change, just
   confirmed.
4. (No `MaxSessionDuration` change; no `.storageconfig` change.)
5. Plan to call canopy's config-as-a-resource API over the tailnet
   (`TailscaleAdmin`) feeding `deviceRoleArn`+`maintenanceRoleArn`+bucket/prefix/
   region; delete cascades to the Secret. Schema TBD from canopy.
