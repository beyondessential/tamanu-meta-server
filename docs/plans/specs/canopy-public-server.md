# Spec: canopy-public-server — device backup endpoints (backup-credentials, backup-target, backup-report)

Implementation spec for the `public-server` slice of the
[backup-credentials](../backup-credentials.md) system. This is **canopy's
first AWS SDK usage** and its first Kubernetes API client on the
internet-facing pod. Read the parent plan for the why; this file is the
how, grounded in the real `crates/public-server` code.

Authoritative design: [`backup-credentials.md`](../backup-credentials.md)
(esp. "Endpoint shape", "Permission templates", "IAM model", "Repository
password ownership", and the "Accepted stage-1 risk" note). The stage-2
hardening that removes this component's blast radius is
[`backup-credentials-blind-relay.md`](../backup-credentials-blind-relay.md)
— **out of scope here**; we build the stage-1 on-demand minting path.

## Purpose

Add three `ServerDevice`-authenticated device endpoints to `public-server`:

- `POST /backup-credentials` — mint short-lived per-group S3 creds via a
  cross-account `sts:AssumeRole` and return them in `credential_process`
  JSON. `restore` purpose adds a read-only session policy.
- `GET /backup-target` — return `{storage, bucket, prefix, region,
  repo_password}` so bestool can reconstruct the kopia repo connection on
  every run. `repo_password` is read from a k8s Secret.
- `POST /backup-report` — record a run outcome into `backup_runs` (the
  "a backup actually completed" signal staleness detection reads).

All three resolve `device → server (live_by_device_id) → group_id →
server_group_backup_config` identically, returning **412** when the device
is bound to no live server and **409** when the server is ungrouped or the
group has no `ready` backup config.

This component **owns the AWS STS client and the kube client on
`AppState`**, plus the net-new deps to support them. It **consumes** the
DB models (new tables defined by the canopy-database component) and the
IAM trust / bucket config provisioned by the ops `backups` stack.

## Scope boundary

In scope (this component):
- The three handlers + their module(s) under `crates/public-server/src/`.
- Net-new workspace deps: `aws-config`, `aws-sdk-sts`, `aws-sdk-s3` (s3
  only if a behavioural no-op lands here; see open questions), `kube`,
  `k8s-openapi`.
- `AppState` AWS-client + kube-client fields and their `FromRef` impls;
  binary `init()` wiring; test-harness wiring.
- The restore session-policy JSON builder.
- Inserting `backup_credential_issuances` rows (capturing `AccessKeyId` +
  best-effort `sts_request_id`).
- A new `AppError` variant for STS/upstream failure → **502**.

Out of scope (other components — depend on, don't build):
- The table schemas/migrations and diesel models (`canopy-database`).
- Staleness scan, maintenance/inspection/preflight Jobs, schedulers
  (`canopy-jobs`).
- Operator onboarding UI, escrow, one-off "backup now" (`private-server` /
  `private-web`).
- The ops `backups`-stack IRSA-trust + action-set + lifecycle changes, the
  ServiceAccount/IRSA Pulumi wiring (`ops`). This spec states the contract
  it needs from ops but does not author the Pulumi.
- bestool's `canopy backup` / `backup-credentials` subcommands (separate
  repo).

## Where it lives in the repo

`public-server` mounts feature routers in `crate::routes()`
(`crates/public-server/src/lib.rs:24`), each module exposing
`pub fn routes() -> OpenApiRouter<AppState>`. Existing device modules:
`events.rs`, `tags.rs`, `statuses.rs`, `servers.rs`, `versions.rs`.

Add one module, `crates/public-server/src/backup.rs`, exposing all three
endpoints, mounted **at the root** (not nested) so the paths are exactly
`/backup-credentials`, `/backup-target`, `/backup-report` (matching the
plan's endpoint shape). `events.rs` is already merged at the root via
`.merge(events::routes())` and is the pattern to copy:

```rust
// crates/public-server/src/lib.rs, in routes()
let mut router = OpenApiRouter::new()
    .merge(events::routes())
    .merge(backup::routes())   // NEW — root-mounted, like events
    .nest("/artifacts", artifacts::routes())
    // ...
```

`backup::routes()` registers all three handlers:

```rust
pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(credentials)) // POST /backup-credentials
        .routes(routes!(target))      // GET  /backup-target
        .routes(routes!(report))      // POST /backup-report
}
```

Add `pub mod backup;` to `lib.rs`. The module is **not** behind the `ui`
feature (it's a device API, like `events`/`statuses`).

## Net-new dependencies

No AWS SDK and no kube client exist anywhere in the workspace today (the
only `aws-*` presence is the `aws-lc-rs` crypto backend, unrelated). Add
to the workspace `Cargo.toml` `[workspace.dependencies]` and reference
from `crates/public-server/Cargo.toml`:

- `aws-config` — default credential/region provider chain. In-cluster this
  resolves the pod's IRSA web-identity automatically (`AWS_ROLE_ARN` +
  `AWS_WEB_IDENTITY_TOKEN_FILE`, injected by EKS once the ServiceAccount is
  IRSA-annotated). No explicit credential wiring in canopy code.
- `aws-sdk-sts` — for `assume_role`.
- `aws-sdk-s3` — **only if** a behavioural no-op (e.g. `GetBucketLocation`)
  is performed at issuance time. The plan puts deep S3 checks in the
  preflight Job, not the hot issuance path, so the public-server may not
  need `aws-sdk-s3` at all. Decide per the open question below; do not add
  it speculatively.
- `kube` (client + `Api<Secret>`) and `k8s-openapi` (with a pinned
  Kubernetes feature, e.g. `v1_30` — match the cluster; verify, don't
  guess). Used by `GET /backup-target` to read the repo-password Secret.

Do **not** pin AWS/kube crate versions from memory — check the registry
(`cargo add --dry-run` / crates.io) and use the workspace's
`[workspace.dependencies]` convention. The repo rule is "never guess
versions; verify or ask."

Feature-gating: these are core to the backup endpoints, which are always
compiled (not `ui`-gated). Add them as unconditional `public-server` deps.

## AppState changes

`AppState` (`crates/public-server/src/state.rs`) is `Clone + Debug` and is
constructed in three places: `init()` (binary), `from_db*()` (helpers),
and the test harness (`commons-tests/src/server.rs:92` builds the struct
literal directly). All three must stay compiling.

Add two fields, both `Option<…>` so the test harness and the
private-server's nested `/public/...` mount (which also build `AppState`)
can leave them `None` and so a missing AWS/kube environment degrades to a
clean error rather than a panic at startup:

```rust
pub struct AppState {
    pub db: Db,
    // ... existing fields ...

    /// STS client built from the pod's IRSA web-identity. `None` when no
    /// AWS environment is configured (tests, the nested private mount).
    /// Backup-credentials issuance requires it; absent ⇒ 502 with a
    /// clear "issuer not configured" message.
    pub sts: Option<aws_sdk_sts::Client>,

    /// Kube client for reading repo-password Secrets in canopy's
    /// namespace. `None` in tests / non-cluster runs ⇒ `/backup-target`
    /// 502s. The namespace to read from is fixed at construction.
    pub kube: Option<BackupSecrets>,
}
```

`BackupSecrets` is a small wrapper holding the `kube::Client` + the
namespace (read from `POD_NAMESPACE` / downward-API env, default
`canopy`), exposing `async fn read_password(&self, secret_name: &str,
key: &str) -> Result<String>`. Keep the kube surface this narrow — the
handler only ever does `get` on one Secret and pulls one key out.

`Debug` derive: `aws_sdk_sts::Client` and `kube::Client` are `Debug`;
`BackupSecrets` derives `Debug`. If any field isn't `Debug`, switch
`AppState` to a manual `Debug` impl rather than dropping the derive
elsewhere.

### FromRef impls

Add `FromRef<AppState>` for the two new client types so handlers can take
them as `State<…>` extractors, mirroring the existing `Db` / `RateLimiter`
impls (`state.rs:77-93`):

```rust
impl FromRef<AppState> for Option<aws_sdk_sts::Client> { /* clone */ }
impl FromRef<AppState> for Option<BackupSecrets> { /* clone */ }
```

(Both AWS SDK clients and `kube::Client` are cheap to clone — they're
`Arc`-backed handles.)

### Binary init wiring

In `AppState::init()` (and a new async constructor, since `aws_config::
load_defaults` is async — `init()` is currently sync), build the clients:

```rust
let aws = aws_config::load_defaults(BehaviorVersion::latest()).await;
let sts = Some(aws_sdk_sts::Client::new(&aws));
let kube = match kube::Client::try_default().await {
    Ok(c) => Some(BackupSecrets::new(c, namespace_from_env())),
    Err(_) => None, // log; backup-target will 502 until fixed
};
```

`init()` is called from `main.rs:49` (`AppState::init()?`). Make the
backup wiring an **async** init path; `main` is already `#[tokio::main]`
so awaiting is fine. Keep a sync/`None`-clients fallback constructor for
the private-server nested mount and any non-AWS deployment.

Region: per-request region comes from the group config row
(`server_group_backup_config.region`, nullable → deployment default). The
STS client itself uses the provider-chain region; only the eventual S3
addressing cares about the bucket's region, and that is handed to the
device in `GET /backup-target`. STS `AssumeRole` is global-ish; set the
STS client region from the default provider.

## Handler 1 — `POST /backup-credentials`

### Request / response

```rust
#[derive(Deserialize, ToSchema)]
pub struct CredentialsArgs {
    #[serde(default)] pub purpose: Purpose, // default Backup
}

#[derive(Deserialize, Serialize, ToSchema, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Purpose { Backup, Restore }
impl Default for Purpose { fn default() -> Self { Self::Backup } }

// credential_process output — field names fixed by the AWS SDK.
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "PascalCase")]
pub struct CredentialProcessOutput {
    pub version: u8,            // serialized as "Version": 1
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub expiration: String,     // RFC3339 / ISO8601 Z
}
```

`Version` is the literal `1`; the rename must produce exactly
`Version/AccessKeyId/SecretAccessKey/SessionToken/Expiration` (the AWS SDK
treats this format as fixed — see the plan's "AWS quirks"). Verify the
`PascalCase` rename yields `AccessKeyId` not `AccessKeyId` casing drift;
if `serde(rename_all)` doesn't produce the exact AWS casing, rename each
field explicitly.

### Handler flow (mirrors plan step list)

Signature follows the bare-handler pattern (events.rs is the closest
twin):

```rust
async fn credentials(
    State(db): State<Db>,
    State(sts): State<Option<aws_sdk_sts::Client>>,
    device: ServerDevice,
    Json(args): Json<CredentialsArgs>,
) -> Result<Json<CredentialProcessOutput>>
```

1. `ServerDevice` authenticates (it yields only a `Device`;
   `device.0.0.id` is the device id — same access as `events.rs:44`,
   `statuses.rs`).
2. Resolve server: `Server::live_by_device_id(&mut conn, device_id)`
   (`crates/database/src/servers.rs:352`). It returns `Vec<Server>`; the
   `servers_device_id_unique` partial index guarantees ≤1, so
   `.into_iter().next()`. Empty ⇒ `AppError::DeviceHasNoServer` (**412**,
   maps at `commons-errors/src/lib.rs:193`). Use `live_by_device_id` (not
   `get_by_device_id`) so archived servers don't issue creds.
3. Read `server.group_id: Option<Uuid>` (`servers.rs:58`). `None` ⇒
   **409** via `AppError::Conflict("server is not in a group")`.
4. Load `ServerGroupBackupConfig::by_group_id(&mut conn, group_id)` (model
   provided by canopy-database). Absent ⇒ **409**
   `AppError::Conflict("group has no backup config")`. Also gate on
   `status == 'ready'`: a `provisioning`/`escrow_pending` row is **409**
   (dormant). This yields `target_role_arn`, `bucket`, `prefix`, `region`.
5. For `purpose == Restore` only: build the read-only **session policy**
   JSON (template below). `Backup` needs none (the per-bucket role's own
   policy is the scoping).
6. Require `sts` is `Some`; else **502** (`AppError::Upstream(...)`,
   "issuer not configured"). Call cross-account assume:
   ```rust
   sts.assume_role()
      .role_arn(&cfg.target_role_arn)
      .role_session_name(format!("canopy-{}-{}", purpose_str, device_id))
      .set_policy(restore_policy_json)   // None for backup
      .duration_seconds(3600)            // chained sessions cap at 1h anyway
      .send().await
   ```
   Any SDK error ⇒ **502** (`AppError::Upstream`). Capture
   `request_id()` (via `aws_sdk_sts::error::ProvideErrorMetadata` /
   `RequestId` trait) best-effort for `sts_request_id`.
7. Pull `credentials` from the response: `access_key_id`,
   `secret_access_key`, `session_token`, `expiration`. A response missing
   credentials ⇒ **502**.
8. Insert `backup_credential_issuances` (canopy-database model
   `NewBackupCredentialIssuance`): `device_id`, `group_id`,
   `expires_at` (from STS `Expiration`), `purpose`, `sts_assumed_role`
   (= `target_role_arn`), `sts_request_id` (nullable), `access_key_id`,
   `bucket`/`prefix` (snapshot of config). A failed audit insert should
   fail the request (don't hand out creds we didn't record).
9. Return `Json(CredentialProcessOutput { version: 1, .. })`, **200**.

`RoleSessionName`: `canopy-backup-<device-id>` / `canopy-restore-<device-id>`
(decision #2; the `canopy-` prefix makes CloudTrail provenance
unambiguous). Note `RoleSessionName` is capped at 64 chars — `canopy-` (7)
+ `restore-` (8) + a 36-char UUID = 51, within budget.

### Restore session policy (normative)

Authored at assume-time; ANDs down to read-only against the per-bucket
role. `<prefix>` is normally empty (repo at bucket root). `GetBucketLocation`
**must be its own unconditioned statement** — the `s3:prefix` context key
isn't populated for it, so folding it under the prefix condition would
silently deny it:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    { "Effect": "Allow", "Action": ["s3:GetObject"],
      "Resource": "arn:aws:s3:::<bucket>/<prefix>*" },
    { "Effect": "Allow", "Action": ["s3:GetBucketLocation"],
      "Resource": "arn:aws:s3:::<bucket>" },
    { "Effect": "Allow", "Action": ["s3:ListBucket"],
      "Resource": "arn:aws:s3:::<bucket>",
      "Condition": { "StringLike": { "s3:prefix": ["<prefix>*"] } } }
  ]
}
```

Build this with `serde_json` (a typed struct or `json!` macro) from the
config's `bucket`/`prefix`; pass the serialized string to `.policy(...)`.
The **backup** permission set is **not** authored here — it's the
per-bucket role's own policy in ops (`AWS_S3_MULTIPART_ACTIONS` is the
source of truth). public-server only authors the restore downscope.

### OpenAPI annotation

`#[utoipa::path(post, path = "/backup-credentials", tag = "backup",
security(("server-device" = [])), request_body = CredentialsArgs,
responses((status=200, body=CredentialProcessOutput), (status=409,
body=ProblemDetailsSchema), (status=412, body=ProblemDetailsSchema),
(status=502, body=ProblemDetailsSchema)))]`. Run `just gen-openapi` is a
private-web concern; the **public** server has its own `openapi.rs`
(`ApiDoc`) — register the new tag/handlers there.

## Handler 2 — `GET /backup-target`

```rust
#[derive(Serialize, ToSchema)]
pub struct BackupTarget {
    pub storage: String,       // "s3"
    pub bucket: String,
    pub prefix: String,        // normally ""
    pub region: String,        // config.region or deployment default
    pub repo_password: String, // read from the k8s Secret
}

async fn target(
    State(db): State<Db>,
    State(kube): State<Option<BackupSecrets>>,
    device: ServerDevice,
) -> Result<Json<BackupTarget>>
```

Flow: steps 1–4 identical to credentials (same 412/409, same `ready`
gate). Then:

5. Require `kube` is `Some` else **502**.
6. `kube.read_password(&cfg.repo_password_ref, KEY).await?` — reads the
   Secret named by `repo_password_ref` from canopy's namespace and pulls
   the password key out (decide the key name — e.g. `password`; fix it as
   a constant and document it for the operator-UI/escrow component that
   creates the Secret). A missing Secret or key ⇒ **502** (upstream
   misconfig), with the group named in the (server-side) log, not the
   body.
7. `region` = `cfg.region` or the deployment default (an env/config
   constant; the plan calls it "deployment default (AWS region)").
8. Return `Json(BackupTarget { storage: "s3".into(), .. })`, **200**.

**Blast-radius note (carry into the PR description, accepted stage-1
risk):** serving `repo_password` makes the internet-facing pod hold
Secret-read for every group's repo password. The stage-1 plan
**accepts** this; the blind-relay stub removes it later. Two invariants
this component must not violate: it only ever does `get` on the one named
Secret (never `list`), and it holds no delete/bypass capability.

## Handler 3 — `POST /backup-report`

```rust
#[derive(Deserialize, ToSchema)]
pub struct BackupReport {
    pub run_id: Uuid,          // becomes backup_runs.id (client-minted)
    pub purpose: Purpose,
    pub outcome: Outcome,      // success | failure
    pub error: Option<String>,
    pub bytes_uploaded: Option<i64>,
    pub snapshot_id: Option<String>,
}

#[derive(Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Outcome { Success, Failure }

async fn report(
    State(db): State<Db>,
    device: ServerDevice,
    Json(rep): Json<BackupReport>,
) -> Result<StatusCode>  // 204
```

Flow: steps 1–3 (resolve device → live server → `group_id`; 412/409). The
config row need **not** be `ready` to accept a report (a report is just an
observation), but the server **must** be grouped — `device_id`/`group_id`
come from the authenticated context, never the client's claim (per plan:
forgery-proof attribution).

Insert `backup_runs` via `NewBackupRun` (canopy-database model) with
`id = rep.run_id` (client-supplied PK — safe: `device_id`/`group_id` are
server-derived, and a duplicate `id` fails its **own** insert with a PK
violation, can't overwrite another row). On a PK-conflict, return a clean
**409** (duplicate run_id) rather than a 500 — map the diesel unique
violation. Return **204 No Content** on success.

This component **does not** run staleness detection or clear the one-off
`backup_requests` flag — those are `canopy-jobs` / scheduler concerns that
read the `backup_runs` rows this writes. (The plan says the one-off flag
is "cleared when the run is reported"; whether that clear happens in this
handler or the scheduler is an open question below.)

## Error handling — new variant

Add `AppError::Upstream(String)` (or `AppError::StsFailed` /
`AppError::BadGateway`) to `crates/commons-errors/src/lib.rs`:

- `IntoResponse` status map (currently `commons-errors/src/lib.rs:180+`):
  `Self::Upstream(_) => StatusCode::BAD_GATEWAY`.
- Problem-type slug (the `match` near `:243`): `Self::Upstream(_) =>
  "upstream"`.
- Add an entry to `ERRORS.md` with heading matching the slug (`upstream`)
  — AGENTS.md requires this.

Reuse existing variants where they fit: `DeviceHasNoServer` (412) and
`Conflict(String)` (409) already exist and map correctly — do not add new
409/412 variants. Only the 502 path is new.

Keep STS/kube error **detail out of the response body** (it can name
roles/buckets); log it server-side and return a generic upstream message.

## Interfaces / contracts

### Consumed (must exist first)

- **canopy-database** models + migrations:
  - `ServerGroupBackupConfig` with `by_group_id(conn, group_id) ->
    Result<Option<Self>>`; fields `bucket`, `prefix`, `target_role_arn`,
    `region: Option<String>`, `repo_password_ref`, `status`
    (`provisioning|escrow_pending|ready`).
  - `NewBackupCredentialIssuance` insert (fields per plan's
    `backup_credential_issuances`).
  - `NewBackupRun` insert with caller-supplied `id` (UUID PK), exposing a
    way to distinguish a unique-violation for the 409 mapping.
  - Re-exports in `database/src/lib.rs`.
- **`Server::live_by_device_id`** — already exists
  (`database/src/servers.rs:352`); `Server.group_id` already exists.
- **ops `backups` stack** (contract, provisioned elsewhere): the
  per-bucket `target_role_arn` **trusts canopy's IRSA principal** for
  cross-account `AssumeRole`; the role's own policy grants the device
  backup action set (`AWS_S3_MULTIPART_ACTIONS`, no delete) and
  `s3:GetBucketObjectLockConfiguration`. The pod's ServiceAccount is
  IRSA-annotated and has RBAC `get` on Secrets in canopy's namespace.
  public-server is **never** granted `CreateBucket` / delete / bypass.
- **Repo-password Secret**: a k8s Secret named by `repo_password_ref`,
  with the password under an agreed key, created by the
  onboarding/escrow component before a group reaches `ready`.

### Provided (to others)

- **`POST /backup-credentials`** → `credential_process` JSON
  (`Version/AccessKeyId/SecretAccessKey/SessionToken/Expiration`); consumed
  by bestool's `credential_process` hook. 412/409/502 contract above.
- **`GET /backup-target`** → `{storage, bucket, prefix, region,
  repo_password}`; consumed by bestool to build the kopia connection.
- **`POST /backup-report`** → 204; writes `backup_runs` rows that
  `canopy-jobs` staleness/reconciliation reads.
- **`backup_credential_issuances` rows** — the audit log + CloudTrail join
  key (`access_key_id`) other components/operators query.
- **`AppError::Upstream` (502)** — reusable upstream-failure variant.
- **`AppState.sts` / `AppState.kube`** — the AWS/kube clients now available
  on public-server's state for any future device-facing AWS use.

## Data shapes (wire)

- credentials request: `{ "purpose": "backup" | "restore" }` (default
  backup).
- credentials response: the four-field `credential_process` JSON +
  `Version: 1`.
- target response: `{ "storage": "s3", "bucket": "...", "prefix": "",
  "region": "...", "repo_password": "..." }`.
- report request: `{ "run_id": uuid, "purpose": ..., "outcome": ...,
  "error"?: str, "bytes_uploaded"?: int, "snapshot_id"?: str }` → 204.

## Testing approach

Per AGENTS.md, HTTP endpoint tests use
`commons_tests::server::run_with_device_auth("server", |conn, cert,
device_id, public, private| async move { ... })`, adding the
`mtls-certificate` header on each request:
`.add_header("mtls-certificate", &cert)`. Use
`#[tokio::test(flavor = "multi_thread")]`. Put tests in
`crates/public-server/tests/backup.rs` (no `_test` suffix) or an inline
`#[cfg(test)] mod tests` as `bestool.rs` does.

**The AWS/kube clients are `None` in the test harness** (the harness
builds `AppState` directly without AWS env — `commons-tests/src/server.rs:92`).
That cleanly tests the **resolution + error** matrix without a live AWS:

- `POST /backup-credentials` with a device bound to **no live server** ⇒
  **412** (seed a device with no server row).
- device → **ungrouped** server ⇒ **409** (seed a server with
  `group_id = NULL`).
- device → grouped server but **no config row** ⇒ **409**.
- device → grouped, config row in `provisioning`/`escrow_pending` ⇒ **409**
  (dormant gate).
- device → grouped, config `ready`, but `sts == None` ⇒ **502** ("issuer
  not configured") — this is the harness default and proves the 502 path.
- `GET /backup-target`: same 412/409 matrix; with config `ready` and
  `kube == None` ⇒ **502**.
- `POST /backup-report`: grouped server ⇒ row written, **204**; assert the
  `backup_runs` row via the model (`device_id`/`group_id` from context,
  not from body — try sending a bogus group and confirm it's ignored);
  duplicate `run_id` ⇒ **409**; ungrouped ⇒ **409**; no live server ⇒
  **412**.

**Successful issuance (200)** needs the STS call mocked or stubbed —
options to weigh (open question): (a) inject a stub STS behind a small
trait so the harness can return canned creds; (b) `aws-smithy-mocks` /
`StaticReplayClient` to replay an `AssumeRole` HTTP response with the
`sts` client present; (c) leave the 200 path to a manual/integration test
against a real role. Prefer (a) or (b) so the happy path + the
issuance-audit insert + the restore-vs-backup `policy` difference are
covered in CI. The restore session-policy JSON builder should also have a
**pure unit test** (assert the three statements, the unconditioned
`GetBucketLocation`, the `<prefix>*` substitution) — that's the
correctness-critical, AWS-free piece.

Seeding helpers: `run_with_device_auth` creates the device + key; insert
the `servers` / `server_groups` / `server_group_backup_config` rows via
the database models (add a seed helper if one doesn't exist). Use direct
model calls for DB state, HTTP for the endpoint behaviour (AGENTS.md).

No frontend/Playwright work in this component (device API, no UI).

## Open questions / decisions to make

1. **STS happy-path test strategy** — stub trait vs `aws-smithy-mocks`
   replay vs manual-only. Recommend a stub trait or smithy mock so CI
   covers 200 + audit insert + the restore/backup policy split. (Pick
   before implementing the happy path.)
2. **`aws-sdk-s3` on public-server at all?** The plan keeps deep S3
   behavioural checks in the preflight Job, not the issuance hot path. If
   issuance does **no** S3 no-op, public-server needs only `aws-sdk-sts`.
   Confirm we are not adding a per-issuance `GetBucketLocation` here
   (latency + an extra permission on the issuer). Default: STS only.
3. **`AppState::init()` becomes async** — it's currently sync and called
   `?`-style from `main`. Confirm making the AWS/kube-aware constructor
   async (and keeping a sync `None`-clients path for the private nested
   mount + tests) is acceptable, vs. lazily building the clients on first
   use.
4. **Repo-password Secret key name** — fix the data key inside the Secret
   (e.g. `password`) and align it with the onboarding/escrow component
   that creates the Secret. Single source of truth needed.
5. **Deployment-default region** — where does the fallback region live
   (env var `AWS_REGION` / a canopy config constant)? `GET /backup-target`
   must always return a concrete region string even when
   `config.region IS NULL`.
6. **Who clears the one-off `backup_requests` flag** — the plan says
   "cleared when the run is reported." Decide whether `POST /backup-report`
   clears it (this component) or the scheduler does (`canopy-jobs`).
   Leaning: the scheduler owns `backup_requests`; this handler just writes
   `backup_runs`. Confirm so the flag-clear isn't dropped between
   components.
7. **Namespace discovery** — read `POD_NAMESPACE` via the downward API, or
   `kube`'s in-cluster namespace inference? Pick one and set it at
   `BackupSecrets` construction.
8. **`Purpose`/`Outcome` shared types** — these enums recur across
   `backup_credential_issuances`, `backup_runs`, the endpoints, and the
   jobs crate. Decide whether they live in `commons-types` (shared) or are
   defined per-crate. Recommend `commons-types` to avoid drift.

---

## Backup types addendum

Per the plan's "Backup types": requests carry a `type`, and there's a new
registration endpoint.

- **New `POST /backup-capabilities`** (ServerDevice): body
  `{ "types": [...] }`; resolve device→server; upsert
  `server_backup_capabilities`, seeding `enabled` from each type's
  `backup_type_defaults.auto_enable` for newly-seen types (don't clobber an
  operator-set `enabled`). 204.
- **`POST /backup-credentials`** body gains `"type"`; **`POST
  /backup-report`** body gains `"type"`; both record it
  (`backup_credential_issuances.type`, `backup_runs.type`/`server_id`).
- Issuance/credentials gating is per `(server, type)`: the capability must
  be `enabled` and the group `ready`.
- Add a shared **effective-config resolver** (override ?? type-default,
  retention floor) — also consumed by the jobs + UI components.
