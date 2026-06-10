# Spec: canopy-operator-ui

Implementation spec for the **operator UI** of the backup-credentials system:
the `TailscaleAdmin`-gated private-server admin endpoints and the private-web
React/MUI screens that drive group backup onboarding/config, repo-creation
trigger + status, the reveal-once escrow + acknowledgment, the one-off "backup
now" trigger, and the read-only stats panel.

Authoritative design: [`../backup-credentials.md`](../backup-credentials.md)
(see esp. "Operator workflows & repo provisioning (private-server UI)",
"Repository password ownership" → "DR escrow", "Backup cadence and triggering"
→ "Operator one-off", "Operational story"). This component owns **only** the
operator-facing surface; it consumes the data model, AWS/k8s machinery,
schedulers, and detection owned by the other backup-credentials components.

This spec is scoped to repo `canopy`: `crates/private-server` (axum admin fns)
and `private-web` (React SPA + Playwright e2e), following the patterns in
`AGENTS.md` ("Private server architecture", "React frontend").

---

## 1. Purpose

Make group backup onboarding a real, self-serve operator workflow in the
existing Tailscale-gated admin SPA — *not* a SQL bootstrap. Concretely, give an
operator the ability to:

1. **Onboard / configure** a group's backup: set `bucket`, `target_role_arn`,
   `region`, `expected_interval`, `retention`, choose from-birth vs. import
   mode, and kick off repo creation.
2. **See repo-creation status** and the lifecycle state machine
   (`provisioning → escrow_pending → ready`), including init-Job failures.
3. **Reveal the generated passphrase once** (from-birth repos), with a "saved
   to Bitwarden" acknowledgment that flips `escrow_pending → ready`.
4. **Trigger a one-off "backup now"** for any group (scheduled or
   manual-only), writing a `backup_requests` row the device picks up on its
   next ~1-minute tick.
5. **Read a stats panel** — cached `backup_repo_stats` plus recent
   `backup_runs` / `backup_maintenance_runs` per group.

The UI is the human end of the control plane; it never talks to AWS, kopia, or
k8s directly — it only reads/writes Canopy's database via private-server fns
and *triggers* the init Job through a fn that the jobs-side machinery acts on.

---

## 2. Where it lives in the repo

### Backend (private-server)

- New module `crates/private-server/src/fns/backups.rs` exposing
  `pub fn routes() -> OpenApiRouter<AppState>`, mounted under `/api/backups`
  in `crates/private-server/src/fns/mod.rs` (add
  `.nest("/backups", backups::routes())` and `pub mod backups;`).
- Follows the exact handler shape used by `server_groups.rs`: bare axum
  handlers `(State<AppState>, [TailscaleAdmin], Json<Args>) -> Result<Json<T>>`,
  each annotated with `#[utoipa::path(post, path = "/<fn>", operation_id =
  "backups_<fn>", tag = "backups", security(("tailscale-admin" = [])), …)]`.
  Read-only endpoints use `security(("tailscale-user" = []))` (matching how
  `server_groups::list`/`get` are user-gated while mutations are admin-gated).
- DB access via `state.db.get().await?`; all model logic lives in the
  **database crate** (`crates/database/src/`), never inline in private-server
  (per AGENTS.md: no diesel in private-server).

### Backend (database crate)

The UI fns are thin wrappers over model functions. The migrations and models
listed in §4 are **shared with the data-model component** of
backup-credentials; if that component lands them first, this component only
adds the *operator-facing* query/mutation methods. To avoid a silent gap, this
spec lists the full set the UI needs; whoever lands the table owns the
migration, and this component owns the query methods it calls. Coordinate via
the `depends_on` contract in the orchestration metadata.

### Frontend (private-web)

- New route components under `private-web/src/routes/`:
  - `BackupConfig.tsx` — onboarding / edit config form (create + edit modes,
    mirroring `GroupEdit.tsx`'s split).
  - `BackupEscrow.tsx` — reveal-once passphrase + ack (often rendered as a
    section inside the group backup page, gated on `status === 'escrow_pending'`).
  - `BackupPanel.tsx` — the per-group backup overview: status, stats, recent
    runs, "backup now" button, links to config/escrow.
- Surfaced from the existing **group detail page** (`GroupDetail.tsx`): add a
  "Backups" section/card that renders `BackupPanel` (or a "Set up backups"
  CTA when no config row exists). New routes registered in `App.tsx`:
  - `/groups/:id/backups` → `BackupPanel`
  - `/groups/:id/backups/config` → `BackupConfig` (create or edit)
- Wire types come from `private-web/src/api-types.ts` (generated) re-exported
  through `private-web/src/types.ts`. UI-only label/order constants
  (status labels, retention field labels) go in `types.ts` below the
  re-exports, same as `SEVERITY_INTENT` / `SERVER_RANK_ORDER`.
- After any handler request/response change, run **`just gen-openapi`** and
  commit both `private-web/openapi.json` and `private-web/src/api-types.ts`
  alongside the Rust change (per AGENTS.md).

### e2e

- `private-web/e2e/backups.spec.ts` (new), using `./test-fixtures` +
  `./seed.ts`. Extend `seed.ts` with `seedServerGroupBackupConfig`,
  `seedBackupRun`, `seedBackupRepoStats`, `seedBackupRequest` helpers and add
  the new tables to `resetSeededTables`'s `TRUNCATE` list.

---

## 3. Lifecycle state machine (the UI's spine)

`server_group_backup_config.status ∈ { 'provisioning', 'escrow_pending',
'ready' }`. The UI renders one of three top-level states per group, plus the
"no config yet" zero-state and an explicit "init failed" sub-state:

```
(no row)  ──[Set up backups: create config]──►  provisioning
provisioning  ──[init Job creates repo, from-birth]──►  escrow_pending
provisioning  ──[init Job creates repo, import mode]──►  ready
provisioning  ──[init Job fails]──►  provisioning + last_init_error shown (retry available)
escrow_pending  ──[operator acks "saved to Bitwarden"]──►  ready
ready  ──[edit non-structural config]──►  ready
```

- The UI **does not** itself run the init Job; it calls `backups.create_repo`,
  which records intent / sets `status='provisioning'` and lets the jobs-side
  init-Job machinery (owned by the maintenance/jobs component) pick it up. The
  UI polls config status (`useReloadInterval`, like the incidents badge) to
  reflect `provisioning → escrow_pending/ready`.
- **Backups are dormant until `ready`** — this is enforced on the device path
  (412/409), not in this UI; the UI surfaces *why* (status chip + helper text)
  so an operator isn't confused that "configured" ≠ "live".
- **Import mode** skips escrow: `create_repo` with `mode='import'` moves
  `provisioning → ready` once the repo connects (operator already holds the
  passphrase / points `repo_password_ref` at an existing Secret).

How the init Job's outcome reaches `status` and `last_init_error` is the
jobs-side component's contract (see §6 consumed contracts). The UI only reads
those fields; it must not assume an in-process transition.

---

## 4. Data shapes (DB)

These tables come from the backup-credentials data model; the UI reads/writes
the subset below. Migrations are created with `just migration NAME` (never
hand-authored — per AGENTS.md). Two of these (`status`, `last_init_error`,
`mode`, `repo_password_ref`, escrow tracking) are the columns the UI most
depends on, so if the base table is authored elsewhere, confirm these exist.

### `server_group_backup_config` (read + write by UI)

Per the main plan's schema, plus the columns the UI lifecycle needs. If the
base table predates this work, the UI requires at minimum:

```sql
-- (from backup-credentials.md "New table: server_group_backup_config")
group_id          UUID PRIMARY KEY REFERENCES server_groups(id) ON DELETE CASCADE,
bucket            TEXT NOT NULL,
prefix            TEXT NOT NULL DEFAULT '',
target_role_arn   TEXT NOT NULL,
region            TEXT,
expected_interval INTERVAL,            -- NULL = manual-only
retention         JSONB NOT NULL,      -- kopia keep-* policy
repo_password_ref TEXT NOT NULL,
status            TEXT NOT NULL,       -- 'provisioning'|'escrow_pending'|'ready'
created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
-- UI/lifecycle additions this component needs (confirm/author):
mode              TEXT NOT NULL DEFAULT 'from_birth',  -- 'from_birth' | 'import'
last_init_error   TEXT,                -- set when the init Job fails; cleared on success/retry
escrow_acked_at   TIMESTAMPTZ,         -- set when the operator acks the reveal-once (from-birth)
escrow_acked_by   TEXT                 -- operator identity (from TailscaleAdmin)
```

`expected_interval` maps to/from the UI via the same minutes/seconds pattern
`GroupEdit.tsx` uses for `slack_open_delay`, except the API column is
`INTERVAL`. Reuse `database::pg_duration::PgDuration` (already used on
`server_groups.slack_open_delay`) and `#[schema(value_type = Option<i64>)]`
so the wire type is seconds. **NULL `expected_interval` = manual-only** must be
representable distinctly from `0`; the form needs a "Manual only (no schedule)"
toggle, not just an empty number field.

`retention` is a small JSON object; on the wire model it as a typed struct
(not raw `serde_json::Value`) so `openapi-typescript` emits a real shape:

```rust
#[derive(Serialize, Deserialize, ToSchema)]
pub struct RetentionPolicy {
    pub keep_latest:  i32,  // default 1 (not floor-enforced)
    pub keep_daily:   i32,  // floor 7
    pub keep_weekly:  i32,  // floor 4
    pub keep_monthly: i32,  // floor 6
    pub keep_annual:  i32,  // default 0
}
```

The org-minimum **floor** (`keep_daily ≥ 7, keep_weekly ≥ 4, keep_monthly ≥ 6`)
is enforced in the model/handler on create+update; the UI also validates
client-side (helper text + disabled submit) but the server is authoritative
(returns `400 AppError::BadRequest`-style problem-details on violation).

### `backup_requests` (write + read by UI — "backup now")

```sql
CREATE TABLE backup_requests (
    server_id    UUID NOT NULL REFERENCES servers(id),
    purpose      TEXT NOT NULL,            -- "backup" | "restore"
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    requested_by TEXT,                     -- operator identity (TailscaleAdmin login)
    PRIMARY KEY (server_id, purpose)
);
```

The "backup now" button targets a **server** (the request is keyed by
`server_id`), so the UI offers per-server one-off triggers within the group's
backup panel (the group's member servers come from `server_groups.get`). A
group-wide "backup all members" convenience can fan out to one row per member
server (open question §8). Cleared by the device path on report; the UI shows
"requested <TimeAgo>, pending" while a row exists.

### Read-only display tables (read by UI)

- `backup_repo_stats` (PK `group_id`): `snapshot_count`, `source_count`,
  `logical_bytes`, `physical_bytes`, `bucket_bytes` (nullable), `observed_at`.
- `backup_runs` (recent N per group): `device_id`, `purpose`, `outcome`,
  `error`, `bytes_uploaded`, `snapshot_id`, `reported_at`.
- `backup_maintenance_runs` (recent N per group): `kind`, `started_at`,
  `finished_at`, `outcome`, `error`, `bytes_reclaimed`.
- `backup_repo_snapshots` (optional, for a per-source "latest snapshot" list):
  `source`, `server_id`, `latest_snapshot_at`.

### Escrow secret read

The reveal-once passphrase is read from the **k8s Secret** named by
`repo_password_ref`. This requires `public-server`/the relevant pod to have a
kube client + Secret-read RBAC — that machinery is **net-new and owned by the
AWS/k8s-infra component**, not this UI. The escrow reveal endpoint
(`backups.reveal_escrow`) consumes that kube-client capability on `AppState`.
If private-server does not have the kube client at the time this lands, see
§8 open question on where the escrow read executes.

---

## 5. Interfaces this component EXPOSES

All under `/api/backups/<fn>`, POST, `TailscaleAdmin`-gated unless noted.
Argument/response structs live in `backups.rs` with `#[derive(…, ToSchema)]`;
operation ids prefixed `backups_`. Names are the contract for the React layer
and any other consumer.

| fn | gate | args | returns | purpose |
|----|------|------|---------|---------|
| `backups_get` | user | `{ server_group_id }` | `BackupConfigView \| null` | full config + lifecycle for a group (null = no config) |
| `backups_list` | user | `{}` | `Vec<BackupConfigSummary>` | all configured groups (fleet overview) |
| `backups_create` | admin | `CreateBackupConfigArgs` | `BackupConfigView` | insert config row (`status='provisioning'`), validate floor; does **not** create repo |
| `backups_update` | admin | `UpdateBackupConfigArgs` | `BackupConfigView` | edit non-structural config (region, interval, retention) on a `ready` group |
| `backups_create_repo` | admin | `{ server_group_id }` | `BackupConfigView` | record intent for the init Job (sets/keeps `provisioning`, clears `last_init_error`); idempotent retry |
| `backups_reveal_escrow` | admin | `{ server_group_id }` | `RevealEscrowResponse` | reveal-once passphrase (from-birth, `escrow_pending` only); reads the k8s Secret |
| `backups_ack_escrow` | admin | `{ server_group_id }` | `BackupConfigView` | flip `escrow_pending → ready`, stamp `escrow_acked_at/by` |
| `backups_request_now` | admin | `{ server_id, purpose }` | `()` | upsert a `backup_requests` row (one-off "backup now") |
| `backups_cancel_request` | admin | `{ server_id, purpose }` | `()` | delete a pending `backup_requests` row |
| `backups_stats` | user | `{ server_group_id }` | `BackupStatsView` | `backup_repo_stats` + recent `backup_runs` + recent `backup_maintenance_runs` + pending requests |
| `backups_delete` | admin | `{ server_group_id }` | `()` | delete the config row (decommission; see audit-table FK note in main plan) |

### Response/argument shapes (wire)

```rust
pub struct BackupConfigView {
    pub server_group_id: Uuid,
    pub bucket: String,
    pub prefix: String,
    pub target_role_arn: String,
    pub region: Option<String>,
    #[schema(value_type = Option<i64>)]            // seconds; None = manual-only
    pub expected_interval: Option<PgDuration>,
    pub retention: RetentionPolicy,
    pub mode: BackupRepoMode,                       // FromBirth | Import (serde lowercase)
    pub status: BackupConfigStatus,                 // Provisioning | EscrowPending | Ready
    pub last_init_error: Option<String>,
    pub escrow_acked_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    // NOTE: never includes repo_password_ref's *value* — only reveal_escrow does.
}

pub struct CreateBackupConfigArgs {
    pub server_group_id: Uuid,
    pub bucket: String,
    #[serde(default)] pub prefix: String,
    pub target_role_arn: String,
    pub region: Option<String>,
    #[schema(value_type = Option<i64>)]
    pub expected_interval: Option<PgDuration>,
    pub retention: RetentionPolicy,
    pub mode: BackupRepoMode,
    /// Import mode only: name of a pre-existing k8s Secret holding the
    /// passphrase. From-birth leaves this None (Canopy generates + names it).
    pub repo_password_ref: Option<String>,
}

pub struct RevealEscrowResponse {
    pub passphrase: String,        // shown once; UI must not persist
    pub repo_password_ref: String, // the Secret name, for the "saved where" note
}

pub struct BackupStatsView {
    pub stats: Option<BackupRepoStats>,            // None until first inspection
    pub recent_runs: Vec<BackupRunRow>,            // most-recent first, capped (e.g. 20)
    pub recent_maintenance: Vec<BackupMaintenanceRow>,
    pub pending_requests: Vec<PendingRequestRow>,  // server_id, purpose, requested_at, requested_by
}
```

Use `commons_types::Uuid` and `jiff::Timestamp` to match the rest of the
codebase (`server_groups.rs` uses these). Status/mode are string enums with
`#[serde(rename_all = "snake_case")]` so the generated TS unions read
`"provisioning" | "escrow_pending" | "ready"` and `"from_birth" | "import"`.

### Error contract (problem-details)

Reuse existing `AppError` variants; map to the documented statuses in
`#[utoipa::path(responses(...))]`:

- `404` (`AppError`'s not-found path) — group / config not found in `get` when
  the caller expects one; but `backups_get` returns `null` for "no config"
  rather than 404, matching the "zero-state" UI. Use 404 only for a bad
  `server_group_id` (group itself missing).
- `400` — retention floor violation, or `create` for a group that already has
  config. Prefer `AppError::Conflict(String)` (→ 409) for "already configured"
  and a bad-request variant for floor violations; pick per existing
  `commons-errors` variants and **update ERRORS.md** if a new variant is added
  (per AGENTS.md, heading must match the problem type).
- `409` (`AppError::Conflict`) — `reveal_escrow`/`ack_escrow` called when
  `status != 'escrow_pending'`, or `create_repo` on an already-ready group, or
  `update` of structural fields (bucket/role) post-creation (those are a repo
  migration, out of scope — reject).
- `502` — `reveal_escrow` if the k8s Secret read fails (control-plane error).

---

## 6. Interfaces this component CONSUMES

From **other backup-credentials components** (must exist first or be stubbed):

- **Data model component (canopy DB):** the migrations/tables in §4 and the
  base `database::server_group_backup_config` / `backup_requests` /
  `backup_runs` / `backup_maintenance_runs` / `backup_repo_stats` models.
  Contract: model structs with the columns in §4; the UI adds query methods
  (`get_for_group`, `list_configured`, `create`, `update`, `set_status`,
  `ack_escrow`, recent-runs queries) — author these in the database crate.
- **AWS/k8s-infra component:** a **kube client on `AppState`** with Secret-read
  capability, so `backups_reveal_escrow` can read the passphrase Secret named
  by `repo_password_ref`. This is net-new (canopy has no kube client today).
  Contract consumed: `state.kube` (or equivalent) + a helper like
  `read_secret(name, key) -> Result<String>`. The UI does **not** create
  Secrets — from-birth passphrase generation + Secret creation is the init
  Job's job; the UI only reveals.
- **Jobs/maintenance component:** the **init Job** that performs
  `kopia repository create` and drives `status`/`last_init_error`. Contract
  consumed: the UI's `backups_create_repo` records intent (sets
  `status='provisioning'`, clears `last_init_error`); the Job is expected to
  transition the row to `escrow_pending` (from-birth) or `ready` (import), or
  set `last_init_error` on failure. The exact handoff (a flag column, a queue,
  or the Job polling `provisioning` rows) is the jobs component's decision —
  this UI only depends on the *observable* `status`/`last_init_error` fields.
- **Device path / detection components:** none consumed directly; the UI
  surfaces their *output* (runs, staleness via the existing issues/events
  model already shown on the server/group pages — no new wiring here).

From **existing canopy code** (already present):

- `commons_servers::tailscale_auth::TailscaleAdmin` extractor (gate +
  operator identity for `requested_by` / `escrow_acked_by`). Confirm how to
  extract the login string from it (mirror whatever `admins.rs` / audited
  endpoints do).
- `database::server_groups::ServerGroup` (member list for per-server "backup
  now"; group existence checks).
- React: `useApi` / `useApiAction` (`private-web/src/api.ts`),
  `useIsAdmin` (`hooks/useIsAdmin.tsx`), `useReloadInterval`
  (status polling), `TimeAgo`, `usePageTitle`, `TagsEditor` pattern.

---

## 7. Frontend behaviour detail

### `BackupPanel` (`/groups/:id/backups`, and a card on `GroupDetail`)

- `useApi("backups", "get", { server_group_id: id }, [id])`.
  - `null` → zero-state: "Backups not set up" + admin-only "Set up backups"
    button → `/groups/:id/backups/config`.
  - non-null → status chip (`provisioning`/`escrow_pending`/`ready` with the
    same intent-helper-text pattern as `SEVERITY_INTENT`), config summary
    (bucket, region, interval or "Manual only", retention), and:
    - `provisioning` → spinner + "Creating repository…"; if `last_init_error`,
      an error Alert + admin "Retry repo creation" (`create_repo`).
    - `escrow_pending` → prominent warning card → render `BackupEscrow`.
    - `ready` → stats (`backups.stats`), recent runs table, per-server
      "Backup now" buttons.
- Poll status with `useReloadInterval` (e.g. 5s while `provisioning`, slower
  when `ready`) so the operator sees the init Job land without a manual reload.

### `BackupConfig` (`/groups/:id/backups/config`)

- Create vs edit split like `GroupEdit.tsx` (`isCreate = no config row`).
- Fields: bucket, target_role_arn, region (optional), **schedule mode toggle**
  (Manual only ↔ Scheduled every N minutes — `expected_interval`), retention
  (5 number fields with floor validation + helper text), repo mode
  (From-birth ↔ Import; Import reveals a `repo_password_ref` field).
- Structural fields (bucket, target_role_arn, mode) are **create-only**;
  disabled in edit mode with helper text ("changing the bucket is a repo
  migration — out of scope here").
- On create success → if from-birth, `create_repo` is offered as the next step
  (or auto-called) so the operator flows into provisioning → escrow.

### `BackupEscrow`

- Renders only when `status === 'escrow_pending'` and `mode === 'from_birth'`.
- "Reveal passphrase" button → `useApiAction("backups", "reveal_escrow")`;
  shows the passphrase in a monospace, copy-to-clipboard block with a loud
  "Save this to Bitwarden NOW — it cannot be shown again" warning.
- A required checkbox "I have saved this passphrase to Bitwarden" enables the
  "Acknowledge & activate backups" button → `ack_escrow` → flips to `ready`.
- The reveal is deliberately re-callable while `escrow_pending` (operator may
  reload before acking); once `ready`, `reveal_escrow` returns 409.

### Admin gating

- Read views (`get`/`list`/`stats`) render for any Tailscale user (user-gate),
  matching `server_groups::list`/`get`.
- All mutating buttons gate on `useIsAdmin() === true`, mirroring
  `GroupDetail.tsx`'s `admin && (<Button …/>)` pattern.

---

## 8. Testing approach (per AGENTS.md)

### Rust endpoint tests (`crates/private-server/tests/`)

- File `backups.rs` (no `_test` suffix). Use `commons_tests::server::run(|conn,
  _public, private| async move { … })`. Endpoints at
  `/api/backups/<fn>`, params via `.json(&serde_json::json!({...}))`, empty
  body `{}` for no-arg fns.
- Cover, with `use database::…;` imports and direct model seeding via `conn`:
  - `create` happy path (status becomes `provisioning`); retention-floor
    rejection → 400/expected status; duplicate-config → 409.
  - `get` returns `null` for unconfigured group; full view once configured.
  - `create_repo` clears `last_init_error` and is idempotent.
  - `ack_escrow` only from `escrow_pending` (409 otherwise); stamps
    `escrow_acked_at/by`.
  - `request_now` upserts; `cancel_request` deletes; PK `(server_id, purpose)`
    means re-request is a no-op upsert, not an error.
  - `update` rejects structural-field changes (409) and accepts
    region/interval/retention.
  - Auth: confirm admin-gated fns reject non-admin (the test harness's auth
    posture — match how other admin fns are tested).
  - `reveal_escrow`: since the kube client is net-new, test against a stubbed
    secret reader if the harness allows; otherwise gate this test on the infra
    component and assert the 409-when-not-escrow_pending branch (which needs no
    Secret read).

### Playwright e2e (`private-web/e2e/backups.spec.ts`)

Per AGENTS.md, UI features ship with e2e coverage in the same change. Seed
state directly via `seed.ts` helpers (extend it — see §2). Cover the rendered
behaviour Rust tests can't:

- Zero-state: group with no config shows "Set up backups" (admin) /
  hidden (non-admin).
- Config form: create writes a `server_group_backup_config` row with the right
  `expected_interval` (assert via `EXTRACT(EPOCH …)` like the cooldown test),
  retention JSON, and `status='provisioning'`; floor violation blocks submit.
- Manual-only toggle persists `expected_interval IS NULL` (distinct from 0).
- Escrow flow: seed `status='escrow_pending'`, mode from_birth; reveal shows
  the passphrase, ack checkbox gates the button, ack flips DB row to `ready`
  and stamps `escrow_acked_at`. (Stub/seed the Secret value via whatever the
  reveal path reads — coordinate with the infra component; if the kube client
  isn't available in e2e, test the ack transition with a pre-revealed state and
  cover reveal separately or behind a fixture flag.)
- "Backup now": clicking writes a `backup_requests` row for the server; a
  pending row shows "requested <ago>"; cancel deletes it.
- Stats panel: seed `backup_repo_stats` + a couple of `backup_runs`; assert the
  numbers and recent-run rows render; `bucket_bytes` NULL renders as "unknown",
  not hidden (per the user's "indicators show unknown state" rule).

### Frontend typecheck / unit

- `just typecheck` for TS (not bare `tsc` — per AGENTS.md). Run `just
  gen-openapi` first so `api-types.ts` matches the handlers.
- Optional vitest unit tests for any pure helper (e.g. interval↔minutes,
  retention-floor validation) mirroring `humanDuration` style.

---

## 9. Open questions / decisions to make

1. **Where does `reveal_escrow` read the Secret?** The plan says the repo
   password is a k8s Secret Canopy owns, and that `public-server` gets a kube
   client for `/backup-target`. But escrow reveal is a **private-server**
   (admin) concern. Decide: (a) give private-server its own kube client +
   Secret-read RBAC, or (b) have private-server proxy to an internal endpoint,
   or (c) store the from-birth passphrase transiently for the escrow window.
   Leaning (a) — least machinery, and private-server is already the admin trust
   surface. This is a dependency on the AWS/k8s-infra component.

2. **One-off "backup now" granularity.** `backup_requests` is keyed by
   `server_id`. Do we expose per-server buttons only, a group-wide "back up all
   members" fan-out (N rows), or both? The main plan's operator-story says
   "trigger a best-effort immediate backup for any group", implying a
   group-level affordance. Proposal: per-server buttons + a group-level "Back
   up all" that fans out, restore as a separate (less prominent) action.

3. **Init-Job handoff signal.** This UI sets `status='provisioning'` and clears
   `last_init_error` on `create_repo`; the exact mechanism the jobs component
   uses to notice (poll `provisioning` rows? a dedicated `init_requested_at`
   column? a queue table?) is the jobs component's call. Confirm so the UI
   writes whatever field the Job reads. Default assumption: Job polls
   `status='provisioning'` rows.

4. **Retry semantics on init failure.** Is "Retry repo creation" just
   `create_repo` again (idempotent), or does a failed repo need cleanup first
   (e.g. a half-created kopia format blob)? UI calls `create_repo`; the Job
   owns idempotency/cleanup. Flag for the jobs component.

5. **Editing structural config.** Spec rejects bucket/role/mode edits
   post-creation (repo migration is out of scope). Confirm with product that
   the only "change" path for those is delete-config + re-onboard, and document
   that in the onboarding runbook.

6. **`region` change UX.** The plan notes changing region/bucket is really a
   repo migration. `region` is editable per the config schema but pointing at a
   different region typically means a different bucket. Decide whether the edit
   form allows `region` edits at all, or gates them behind a warning. Proposal:
   allow but warn loudly.

7. **Group-level vs server-level stats anchor.** Stats are per-group
   (`backup_repo_stats` PK `group_id`) but runs/requests are per-server/device.
   The panel mixes both; confirm the grouping the operator expects (group
   headline stats + per-server run history).

8. **Decommission flow in UI.** `backups_delete` removes the config row;
   audit tables intentionally have no CASCADE and the bucket persists
   (object-locked). Decide how much of that the UI explains vs. defers to the
   runbook — at minimum a confirm dialog noting "the bucket and its locked
   objects persist; this only stops issuance".
