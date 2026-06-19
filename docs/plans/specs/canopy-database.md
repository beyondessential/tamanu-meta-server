# Implementation spec: `canopy-database` (backup-credentials)

Component: the **database crate** (`crates/database`) changes for the
backup-credentials system. This is the foundational layer every other
component (public-server endpoints, the `jobs`-crate schedulers, the
private-server operator UI) builds on: it owns the migrations, the diesel
models, and the `lib.rs` re-exports for all backup tables.

Authoritative design: [`../backup-credentials.md`](../backup-credentials.md)
(stage-2 stub: [`../backup-credentials-blind-relay.md`](../backup-credentials-blind-relay.md)).
This spec does not re-litigate decided shape — it makes the DB layer concrete.

---

## Purpose

Provide the persistent state for (**10 tables** as shipped — the "Backup
types addendum" at the foot of this spec added `backup_type_defaults`,
`server_backup_capabilities`, and `server_group_backup_schedule` to the
original seven; they're folded into the list here):

- **`server_group_backup_config`** — per-group repo-level backup configuration +
  lifecycle status (one row per configured group; `group_id` PK → `server_groups`).
- **`backup_type_defaults`** — Canopy-wide per-type defaults (`default_interval`,
  `default_retention`, `auto_enable`).
- **`server_backup_capabilities`** — what each server advertises it can back up
  (bestool-registered), with a per-server `enabled` toggle.
- **`server_group_backup_schedule`** — per-`(group, type)` schedule/retention
  overrides over the type defaults.
- **`backup_credential_issuances`** — audit log of every STS credential issuance.
- **`backup_runs`** — what bestool reported per backup/restore run (client-minted UUID PK).
- **`backup_maintenance_runs`** — Canopy-owned maintenance-Job outcomes (per-group).
- **`backup_repo_snapshots`** — ground-truth inventory from the read-only inspection Job.
- **`backup_repo_stats`** — cached repo + bucket size/stats for operator display (per-group).
- **`backup_requests`** — pending operator one-off "backup now" flags (per `(server, type, purpose)`).

Plus the diesel model structs, insert/query helpers, and `lib.rs` module +
re-exports. Where helpers fall on a component boundary (e.g. staleness scan
queries used by the `jobs` crate, issuance recording used by public-server)
this spec defines the **signatures and ownership**; the calling logic lives
in those components' own specs.

---

## Conventions to follow (grounded in the repo)

Read before implementing: `crates/database/src/{schema,servers,server_groups,devices,issues,statuses,pg_duration}.rs`
and `migrations/2026-05-22-120000-0000_server_groups/{up,down}.sql`.

- **Migrations are scaffolded with `just migration NAME`** (never hand-create
  the directory — inconsistent naming; this is a flagged repeat mistake).
  That runs `diesel migration generate`, producing
  `migrations/<ts>_<name>/{up,down}.sql`. Then `just migrate` runs them and
  `cargo fmt`s the regenerated `schema.rs`. The diesel CLI **regenerates
  `crates/database/src/schema.rs`** from the live DB — do **not** hand-edit
  `schema.rs`; let the migration drive it, then commit the diff.
- One migration per logical change is the norm, but a cohesive feature can be
  several sequential migrations (see the `2026-06-01-012906-000{0,1,2}` triple).
  RESOLVED (impl): the core landed as one migration
  `2026-06-12-090526-0000_backup_credentials` creating all **10** tables (the
  addendum tables included from the start), with a clean reverse-order
  `DROP TABLE` `down.sql`. Two follow-up migrations layered on later:
  `2026-06-15-064431-0000_backup_group_scoped_issues` and
  `2026-06-16-001346-0000_backup_config_lifecycle_columns` (adds `mode`,
  `last_init_error`, `escrow_acked_at`, `escrow_acked_by` to
  `server_group_backup_config`).
- **Timestamps**: columns are `TIMESTAMPTZ NOT NULL DEFAULT NOW()`. Models map
  them with `#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as =
  jiff_diesel::Timestamp)]` over a `jiff::Timestamp` field; nullable ones use
  `jiff_diesel::NullableTimestamp` over `Option<Timestamp>`.
- **`updated_at` auto-touch**: for tables with an `updated_at`, call
  `SELECT diesel_manage_updated_at('<table>');` in `up.sql` (as
  `server_groups` does). Only `server_group_backup_config` needs this here.
- **INTERVAL columns** map to `crate::pg_duration::PgDuration` (wraps
  `jiff::SignedDuration`; serde wire form is whole seconds as `i64`). For a
  *nullable* interval (`expected_interval`), the field is
  `Option<PgDuration>`; annotate the schema with `#[schema(value_type =
  Option<i64>, format = "int64")]` for utoipa (see `ServerGroup::slack_open_delay`).
- **JSONB `retention`** maps to `serde_json::Value` *at the storage layer* —
  diesel handles `Jsonb -> serde_json::Value` natively (see `statuses.health`,
  `statuses.extra`). The retention columns stay `JsonValue` in the model structs.
  RESOLVED (impl): a validated shape **was** wanted after all — a typed
  `RetentionPolicy` struct (`backups::RetentionPolicy`) sits *over* the raw
  value with the kopia `keep_*` fields, `FLOOR_DAILY/WEEKLY/MONTHLY` constants,
  `validate_floor()` (returns `AppError::BadRequest` listing the violated
  fields), and `from_json`/`to_json`/`to_value` converters. The floor logic
  lives in the DB crate and is called by the private-server write path; the
  storage columns themselves remain `JsonValue` (so `RetentionPolicy` is a
  helper, not a diesel column type).
- **Models**: `#[derive(Debug, Clone, Serialize, Deserialize, Queryable,
  Selectable, Insertable, utoipa::ToSchema)]`, `#[diesel(table_name =
  crate::schema::<table>)]`, `#[diesel(check_for_backend(diesel::pg::Pg))]`.
  A separate `New<Table>` `Insertable` struct is used where the insert shape
  differs from the row (see `NewServerGroup`, `NewStatus`). Add
  `#[diesel(belongs_to(...))]` + a `joinable!` entry where a join is wanted.
- **Helper methods** are `impl` blocks with
  `pub async fn (db: &mut AsyncPgConnection, ...) -> Result<...>` returning
  `commons_errors::Result`, ending each query with `.map_err(AppError::from)`.
  Use `use crate::schema::<table>::dsl;` inside each fn (the established style).
- **`BIGSERIAL` PK** maps to `pub id: i64` in the model and is **omitted** from
  the `New<Table>` insertable.
- **Schema regen verification**: after the migration, `schema.rs` gains the 10
  `diesel::table!` blocks, plus `joinable!` and `allow_tables_to_appear_in_same_query!`
  entries. Confirm `bigserial` surfaces as `Int8`, `JSONB` as `Jsonb`,
  `INTERVAL` as `Interval`/`Nullable<Interval>`.

---

## Migration: `backup_credentials`

`up.sql` creates the 10 tables below. DDL is normative (it is what the
diesel schema regen reads); the design doc's snippets are the source.

RESOLVED (impl) — **FK semantics are uniform: plain `REFERENCES` with NO
`ON DELETE`/`ON UPDATE` clause everywhere.** `server_groups`, `servers`, and
`devices` are *archived* (`deleted_at` soft-delete), never hard-deleted, so the
cascade-vs-preserve distinction the original per-table notes agonised over is
moot in practice. The per-table "CASCADE here / no-CASCADE there" prose below
is **superseded** by this single rule; open questions 1 & 2 are resolved
accordingly (see below). The original notes are kept inline for design history,
struck through where they no longer hold.

The addendum tables (`backup_type_defaults`, `server_backup_capabilities`,
`server_group_backup_schedule`) and the type-keying deltas are folded into the
DDL shown here; see the addendum at the foot for the design rationale.

### `server_group_backup_config`

RESOLVED (impl) — `expected_interval` and `retention` moved off this table
(repo-level only now; schedule/retention live per-`(group, type)` in
`server_group_backup_schedule` per the addendum). Lifecycle columns (`mode`,
`last_init_error`, `escrow_acked_at`, `escrow_acked_by`) were added by the
`2026-06-16-...backup_config_lifecycle_columns` migration. As-shipped DDL:

```sql
CREATE TABLE server_group_backup_config (
    group_id          UUID PRIMARY KEY REFERENCES server_groups(id),
    bucket            TEXT NOT NULL,
    prefix            TEXT NOT NULL DEFAULT '',
    target_role_arn   TEXT NOT NULL,
    region            TEXT,
    repo_password_ref TEXT NOT NULL,
    status            TEXT NOT NULL CHECK (status IN ('provisioning', 'escrow_pending', 'ready')),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- added by 2026-06-16-...backup_config_lifecycle_columns:
    mode              TEXT NOT NULL DEFAULT 'from_birth' CHECK (mode IN ('from_birth', 'import')),
    last_init_error   TEXT,
    escrow_acked_at   TIMESTAMPTZ,
    escrow_acked_by   TEXT
);
SELECT diesel_manage_updated_at('server_group_backup_config');
```

- ~~**`ON DELETE CASCADE` is intentional** here~~ — superseded: plain
  `REFERENCES`, no cascade (archival model; groups are soft-deleted, not
  hard-deleted).
- `status` is a `TEXT` column with a `CHECK (status IN (...))` for the closed
  three-value set `{provisioning, escrow_pending, ready}`, validated in code via
  the `BackupConfigStatus` enum. The closed enums all carry a DB `CHECK`
  in the shipped schema (status, mode, purpose, outcome, kind).
- **Lifecycle columns:** `mode` is the 5th closed enum `BackupRepoMode`
  (`from_birth` / `import`, with a DB CHECK); `last_init_error` is set by the
  init Job on `kopia repository create` failure and cleared by the operator-UI
  on retry; `escrow_acked_at`/`escrow_acked_by` stamp the Bitwarden-escrow ack
  that flips `escrow_pending → ready`.

### `backup_type_defaults`, `server_backup_capabilities`, `server_group_backup_schedule` (addendum tables)

```sql
CREATE TABLE backup_type_defaults (
    type              TEXT PRIMARY KEY,
    default_interval  INTERVAL,
    default_retention JSONB NOT NULL CHECK (jsonb_typeof(default_retention) = 'object'),
    auto_enable       BOOLEAN NOT NULL DEFAULT false
);

CREATE TABLE server_backup_capabilities (
    server_id     UUID NOT NULL REFERENCES servers(id),
    type          TEXT NOT NULL,
    enabled       BOOLEAN NOT NULL,
    registered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (server_id, type)
);

CREATE TABLE server_group_backup_schedule (
    group_id          UUID NOT NULL REFERENCES server_groups(id),
    type              TEXT NOT NULL,
    expected_interval INTERVAL,
    retention         JSONB CHECK (retention IS NULL OR jsonb_typeof(retention) = 'object'),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, type)
);
SELECT diesel_manage_updated_at('server_group_backup_schedule');
```

- `retention` now lives here (nullable, `jsonb_typeof='object'` CHECK when
  present) and on `backup_type_defaults.default_retention` (NOT NULL, same
  CHECK) — **superseding** the original `retention` column on
  `server_group_backup_config`. Effective value for a `(group, type)` is the
  schedule override `?? backup_type_defaults`, with the org retention floor
  applied in code (`RetentionPolicy::validate_floor`).
- `server_backup_capabilities.enabled` is **seeded** from
  `backup_type_defaults.auto_enable` at first registration, then operator-
  toggleable per server.

### `backup_credential_issuances`

```sql
CREATE TABLE backup_credential_issuances (
    id                  BIGSERIAL PRIMARY KEY,
    device_id           UUID NOT NULL REFERENCES devices(id),
    group_id            UUID NOT NULL REFERENCES server_groups(id),
    type                TEXT NOT NULL,
    issued_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at          TIMESTAMPTZ NOT NULL,
    purpose             TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
    sts_assumed_role    TEXT NOT NULL,
    sts_request_id      TEXT,
    access_key_id       TEXT,
    bucket              TEXT NOT NULL,
    prefix              TEXT NOT NULL
);
CREATE INDEX ON backup_credential_issuances (device_id, issued_at DESC);
CREATE INDEX ON backup_credential_issuances (group_id, issued_at DESC);
```

- ~~**No CASCADE on `group_id`/`device_id` — deliberate audit preservation**~~ —
  superseded by the uniform no-cascade/archival rule (the FK is plain
  `REFERENCES` regardless; the audit data is preserved because rows are
  soft-deleted, not hard-deleted).
- `bucket`/`prefix` are **snapshots at issuance time**, not FKs back to config.
- `type TEXT` (addendum) — backups are keyed `(server, type)`.
- `purpose` is `TEXT` with a DB `CHECK (purpose IN ('backup','restore'))`,
  also validated in code via `BackupPurpose`.

### `backup_runs`

```sql
CREATE TABLE backup_runs (
    id              UUID PRIMARY KEY,
    device_id       UUID NOT NULL REFERENCES devices(id),
    group_id        UUID NOT NULL REFERENCES server_groups(id),
    server_id       UUID REFERENCES servers(id),
    type            TEXT NOT NULL,
    purpose         TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
    outcome         TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
    error           TEXT,
    bytes_uploaded  BIGINT,
    snapshot_id     TEXT,
    reported_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON backup_runs (group_id, reported_at DESC);
CREATE INDEX ON backup_runs (device_id, reported_at DESC);
CREATE INDEX ON backup_runs (server_id, type, reported_at DESC);
```

- `server_id` (nullable) + `type TEXT` were added by the addendum so staleness
  is per-`(server, type)`. The third index `(server_id, type, reported_at DESC)`
  serves that per-`(server, type)` "latest run" staleness scan.

- **`id` is a client-supplied UUID** (the run-uuid bestool mints at run start),
  **not** `gen_random_uuid()` and **not** `BIGSERIAL`. No `DEFAULT`. The
  `New`-side insert provides it. A duplicate `id` fails its own insert (PK
  violation) — that's the intended safety (design §`backup_runs`); the model
  helper should surface that as a clean error, not panic.
- `device_id`/`group_id` come from the authenticated `ServerDevice` context in
  the caller, **never** from the client body — the model helper takes them as
  parameters (see contract below), it does not read them from a deserialized
  client struct.
- Plain `REFERENCES` on the FKs (uniform no-cascade/archival rule).
- For the staleness scan, the hot query is "latest successful `purpose='backup'`
  run per `(server, type)`"; the `(server_id, type, reported_at DESC)` index
  serves it (the `(group_id, reported_at DESC)` index serves repo-level cuts).

### `backup_maintenance_runs`

```sql
CREATE TABLE backup_maintenance_runs (
    id              BIGSERIAL PRIMARY KEY,
    group_id        UUID NOT NULL REFERENCES server_groups(id),
    kind            TEXT NOT NULL,            -- "quick" | "full"
    started_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    finished_at     TIMESTAMPTZ,
    outcome         TEXT,                     -- NULL while running
    error           TEXT,
    bytes_reclaimed BIGINT
);
CREATE INDEX ON backup_maintenance_runs (group_id, started_at DESC);
```

- Plain `REFERENCES` on `group_id` (uniform no-cascade/archival rule).
- `outcome` NULL = still running; the model helper has a `start()` (insert,
  returns the new `i64` id) and a `finish(id, outcome, error, bytes_reclaimed)`
  update — the Job-side caller (jobs crate) owns the start/finish bracket.

### `backup_repo_snapshots`

```sql
CREATE TABLE backup_repo_snapshots (
    group_id           UUID NOT NULL REFERENCES server_groups(id),
    source             TEXT NOT NULL,
    server_id          UUID REFERENCES servers(id),
    type               TEXT,
    latest_snapshot_at TIMESTAMPTZ,
    observed_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, source)
);
```

- Composite PK `(group_id, source)`. The inspection Job **upserts** per source
  (`ON CONFLICT (group_id, source) DO UPDATE`) — provide an `upsert` helper.
- `server_id` (and `type`, addendum) are parsed from `source` by the caller and
  are **nullable** (a source whose server-id no longer resolves still records).
  Plain `REFERENCES` on both FKs (uniform no-cascade/archival rule) — RESOLVED:
  no `SET NULL`; servers are archived not deleted, so the "block on delete"
  worry is moot.

### `backup_repo_stats`

```sql
CREATE TABLE backup_repo_stats (
    group_id         UUID PRIMARY KEY REFERENCES server_groups(id),
    snapshot_count   INTEGER,
    source_count     INTEGER,
    logical_bytes    BIGINT,
    physical_bytes   BIGINT,
    bucket_bytes     BIGINT,
    observed_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- One row per group (PK = `group_id`). Filled by **two distinct writers**: the
  inspection Job sets the repo-derived fields + `source_count`/`snapshot_count`;
  the S3-metrics task sets `bucket_bytes` (best-effort/nullable, may lag).
  Provide **two separate update helpers** so each writer touches only its
  fields (don't clobber `bucket_bytes` from the inspection writer, or vice
  versa) — both upsert on `group_id`.
- This is a *cache*, not audit. RESOLVED (impl): plain `REFERENCES`, no
  cascade — the uniform archival rule applies here too (groups are
  soft-deleted, so a "rebuildable cache should cascade" exception isn't needed).

### `backup_requests`

```sql
CREATE TABLE backup_requests (
    server_id    UUID NOT NULL REFERENCES servers(id),
    type         TEXT NOT NULL,
    purpose      TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    requested_by TEXT,
    PRIMARY KEY (server_id, type, purpose)
);
```

- Keyed on `server_id` (one-off requests are server-scoped, cleared when the
  run is reported). Composite PK is `(server_id, type, purpose)` (addendum
  added `type`) — one pending request per `(server, type, purpose)`; a second
  request is an upsert (refresh `requested_at`/`requested_by`).
- This is transient operator intent, not audit. RESOLVED (impl): plain
  `REFERENCES` on `server_id`, no cascade (uniform archival rule).

`down.sql`: `DROP TABLE` all 10 in reverse-dependency order (no inter-table
FKs among them; all FKs point at pre-existing tables).

---

## Diesel models + `lib.rs`

New module `crates/database/src/backups.rs` (single module for all 10
tables — they're one cohesive feature, mirroring how `issues.rs` holds
issues/events/incidents together). RESOLVED (impl) — the as-shipped `lib.rs`
re-export superset:

```rust
pub mod backups;
pub use backups::{
    BackupCredentialIssuance, BackupMaintenanceRun, BackupRepoSnapshot, BackupRepoStats,
    BackupRequest, BackupRun, BackupTypeDefault, NewBackupCredentialIssuance, NewBackupRun,
    NewBackupTypeDefault, NewServerGroupBackupConfig, NewServerGroupBackupSchedule,
    ServerBackupCapability, ServerGroupBackupConfig, ServerGroupBackupSchedule,
};
// the enums are defined in commons-types and re-exported through database:
pub use commons_types::backup::{
    BackupConfigStatus, BackupPurpose, BackupRepoMode, BackupType, MaintenanceKind, RunOutcome,
};
// RetentionPolicy is reached as `database::backups::RetentionPolicy`
// (it lives in the backups module; not in the flat re-export set).
```

(Existing `lib.rs` re-exports `devices::*` and `bestool_snippets::*`; backups
is the same pattern. The five closed enums — `BackupPurpose`, `RunOutcome`,
`MaintenanceKind`, `BackupRepoMode`, `BackupConfigStatus` — live in
`commons-types` plus the open `BackupType{Custom}`; see below.)

### String-typed enums

`purpose`, `status`, `outcome`, `kind` are `TEXT` in the DB. Two options,
consistent with existing code:

- **Plain `String` fields**, validated at the API layer (matches
  `issues.resolved_reason`, `servers.kind`). Simplest; lowest ceremony.
- A small enum in `commons-types` with `Display`/`FromStr` and stored via
  `deserialize_as = String, serialize_as = String` (matches how `Severity` and
  `ServerKind` are handled). Preferred if the values are reused across
  public-server, jobs, and private-web wire types — which they are
  (`purpose` flows through three components).

RESOLVED (impl): the enum option won across the board — **all** the closed
enum-ish columns are typed `commons-types` enums (via a `text_enum!` macro that
implements `Display`/`FromStr` + diesel `ToSql`/`FromSql` over `Text`), each
backed by a DB `CHECK`. The five closed enums are `BackupPurpose
{Backup, Restore}`, `RunOutcome {Success, Failure}`, `MaintenanceKind
{Quick, Full}`, `BackupRepoMode {FromBirth, Import}`, and `BackupConfigStatus
{Provisioning, EscrowPending, Ready}` (the original spec listed only the first
four — `BackupRepoMode` is the 5th, added with the lifecycle columns).
`status` did **not** stay a bare `String` — it's `BackupConfigStatus`.
Separately, `backup type` is the **open** enum `BackupType` with a `Custom(String)`
arm (no DB CHECK, any advertised name preserved verbatim). The model field
types and the `CHECK` constraints agree.

### Model sketches (abbreviated; full set in `backups.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_group_backup_config)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerGroupBackupConfig {
    pub group_id: Uuid,
    pub bucket: String,
    pub prefix: String,
    pub target_role_arn: String,
    pub region: Option<String>,
    pub repo_password_ref: String,
    pub status: BackupConfigStatus,
    #[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
    pub created_at: Timestamp,
    #[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
    pub updated_at: Timestamp,
    // lifecycle columns (2026-06-16 migration):
    #[schema(value_type = String)]
    pub mode: BackupRepoMode,
    pub last_init_error: Option<String>,
    #[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
    pub escrow_acked_at: Option<Timestamp>,
    pub escrow_acked_by: Option<String>,
}
```

(Note: `expected_interval`/`retention` are **not** on this struct — they moved
to `server_group_backup_schedule` / `backup_type_defaults` per the addendum.)

`backup_runs` row maps `id: Uuid` (client-supplied, no default). Its
`NewBackupRun` insertable **includes** `id` (unlike the BIGSERIAL tables which
omit it). `bytes_uploaded`/`snapshot_id`/`error` are `Option<_>`.

### Model helper methods (DB-layer surface this component owns)

Defined here; their callers live in other components' specs. Signatures
(`db: &mut AsyncPgConnection`, returning `commons_errors::Result`):

- `ServerGroupBackupConfig::get(db, group_id) -> Result<Option<Self>>` — the
  endpoint resolution path (absent → caller maps to 409).
- `ServerGroupBackupConfig::upsert(db, NewServerGroupBackupConfig) -> Result<Self>`
  and `set_status(db, group_id, status) -> Result<Self>` — operator UI / repo-init flow.
- `ServerGroupBackupConfig::list_scheduled(db) -> Result<Vec<Self>>` — rows
  with `expected_interval IS NOT NULL` (the staleness-scan set).
- `BackupCredentialIssuance::record(db, NewBackupCredentialIssuance) -> Result<Self>`
  — called by public-server step 7. Snapshots bucket/prefix; takes resolved
  `device_id`/`group_id`/`access_key_id`/`sts_request_id`.
- `BackupRun::record(db, NewBackupRun) -> Result<Self>` — called by
  `POST /backup-report`. PK violation on duplicate `id` returns a clean
  `Result::Err` (caller decides idempotency response).
- `BackupRun::latest_success_for_server(db, server_id) -> Result<Option<Self>>`
  and a bulk `latest_success_by_server(db, &[Uuid]) -> Result<HashMap<Uuid, Self>>`
  filtered to `purpose='backup'`, `outcome='success'` — the staleness join.
  (Server-centric: `backup_runs` rows carry `device_id`; the scan joins via the
  server's associated devices, or filters by `group_id` then maps device→server
  in the caller. Provide the query keyed the way the `jobs` scan needs — settle
  with the jobs spec; the DB helper exposes both a per-group and per-device cut.)
- `BackupMaintenanceRun::start(db, group_id, kind) -> Result<i64>` and
  `finish(db, id, outcome, error, bytes_reclaimed) -> Result<()>`.
- `BackupRepoSnapshot::upsert(db, group_id, source, server_id, latest_snapshot_at) -> Result<()>`
  and `list_for_group(db, group_id) -> Result<Vec<Self>>`.
- `BackupRepoStats::upsert_repo_fields(db, group_id, snapshot_count, source_count, logical, physical) -> Result<()>`
  and `upsert_bucket_bytes(db, group_id, bucket_bytes) -> Result<()>` — the two
  separate writers; both `ON CONFLICT (group_id) DO UPDATE` touching only their
  own columns. `get(db, group_id) -> Result<Option<Self>>` for the stats panel.
- `BackupRequest::enqueue(db, server_id, purpose, requested_by) -> Result<()>`
  (upsert), `clear(db, server_id, purpose) -> Result<()>`,
  `pending_for_server(db, server_id) -> Result<Vec<Self>>`.

The "present since" anchor for never-backed-up detection
(`max(MIN(device_server_associations.first_seen) over the server,
server_group_backup_config.created_at)`) is a **jobs-crate query** that joins
these tables; the DB crate exposes the building blocks (`list_scheduled`,
`latest_success_*`, and existing `device_server_associations` access). Note
`first_seen` is per `(device_id, server_id)` pair — use `MIN` over the server.

---

## Interfaces / contracts

### Provided (to other components)

- **Tables + schema** — the seven `diesel::table!` blocks in `schema.rs` and
  the typed models in `backups.rs`, re-exported from `lib.rs`. Public-server,
  the jobs crate, and private-server all `use database::{...}`.
- **`ServerGroupBackupConfig` row** — the single source for repo-level
  `bucket`, `prefix`, `region`, `target_role_arn`, `repo_password_ref`,
  `status`, `mode`, and the lifecycle fields. (Schedule/retention are now
  per-`(group, type)` on `server_group_backup_schedule` / `backup_type_defaults`.)
  Consumed by: `GET /backup-target` &
  `POST /backup-credentials` (public-server); maintenance/inspection schedulers
  & preflight (jobs); onboarding/stats UI (private-server).
- **Audit-record helpers** — `BackupCredentialIssuance::record`,
  `BackupRun::record`, `BackupMaintenanceRun::{start,finish}` — the write
  surface for the issuance/report/maintenance flows.
- **Scan helpers** — `list_scheduled`, `latest_success_*`,
  `BackupRepoSnapshot::list_for_group` — inputs to staleness/reconciliation
  detection (jobs crate, signals 1 & 2).
- **`backup_requests` queue** — `enqueue`/`clear`/`pending_for_server` — the
  operator one-off "backup now" home, read by the cadence-trigger path.
- **Wire/utoipa shapes** — every model derives `utoipa::ToSchema`, so the
  private-server handlers can use them in `#[utoipa::path]` and the regenerated
  `openapi.json` → `api-types.ts` exposes them to private-web (run
  `just gen-openapi` in the component that adds the handlers, not here).

### Consumed (from existing code)

- `server_groups(id)`, `servers(id)`, `devices(id)` — FK targets.
- `crate::pg_duration::PgDuration` (INTERVAL ↔ `SignedDuration`).
- `jiff_diesel::{Timestamp, NullableTimestamp}` (timestamp mapping).
- `commons_errors::{AppError, Result}` (return type + `AppError::from`).
- `device_server_associations` (`first_seen`, per device-server pair) — read by
  the staleness "present since" query, joined alongside these tables.
- `database::issues::NewEvent::save(conn, server_id, device_id)` — *not* called
  by this crate, but the staleness/poisoning alerting (jobs) writes through it;
  this crate must not duplicate alerting logic. (`source="canopy"`,
  `ref="backup-staleness"` etc. live in the jobs component.)

### Explicitly NOT in this component

- No AWS SDK / STS / S3 calls (public-server + jobs).
- No kube client / Secret reads (public-server + jobs).
- No HTTP handlers, no scheduler loops, no alerting/`NewEvent` construction.
- No utoipa `#[path]` annotations or `openapi.json` regen (that's whichever
  crate adds the handlers).

---

## Data shapes

- **`retention`** (JSONB, on `server_group_backup_schedule` /
  `backup_type_defaults`): a kopia keep-policy object, e.g.
  `{"keep_latest":1,"keep_daily":7,"keep_weekly":4,"keep_monthly":6,"keep_annual":0}`.
  Stored as `JsonValue`; the typed `RetentionPolicy` helper sits over it.
  **Floor enforcement** (`keep_daily≥7`, `keep_weekly≥4`, `keep_monthly≥6`) is
  `RetentionPolicy::validate_floor()` — a DB-crate function (returns
  `AppError::BadRequest`) called by the private-server write path, **not** a DB
  constraint (the only DB CHECK on these columns is `jsonb_typeof='object'`).
- **`status`**: `provisioning` → `escrow_pending` → `ready`. Backups dormant
  (412/409 from the endpoints) until `ready` — enforced by the *endpoint*, but
  the column is the source of truth.
- **`purpose`**: `backup` | `restore`.
- **`outcome`**: `success` | `failure` (`backup_runs`); same plus NULL-while-
  running for `backup_maintenance_runs`.
- **`kind`**: `quick` | `full`.
- **`source`** (`backup_repo_snapshots`): kopia source string
  `canopy@<server-id>:<path>`; `server_id` parsed out by the caller.

---

## Testing approach (per AGENTS.md)

DB-only tests via `commons_tests::db::TestDb::run(|mut conn, _url| async move { ... })`,
`#[tokio::test(flavor = "multi_thread")]`, exercising **model functions
directly** (not HTTP). Put them in `crates/database/tests/` with no `_test`
suffix (e.g. `tests/backups.rs`), `use database::*;` for the models. Run with
`just test-package database` or `just test-name <name>`.

Cover:

1. **Migration applies cleanly** — implicitly via every test (each spins a
   fresh migrated DB) plus an explicit smoke test inserting one row per table.
2. **`server_group_backup_config`** — insert/upsert round-trip incl. NULL
   `region`/`expected_interval`, JSONB `retention` round-trips, `status`
   transitions, `updated_at` auto-touch fires on update, `jsonb_typeof` CHECK
   rejects a non-object retention.
3. **FK behaviour (archival model)** — RESOLVED (impl): there is **no** cascade
   anywhere; groups/servers are archived (`deleted_at`), never hard-deleted, so
   a config-delete does not cascade and a hard `DELETE` on a `server_groups` row
   with any backup rows simply fails the FK (and is never done in practice). The
   original "cascade the config / block on audit rows" split no longer applies —
   the rule is uniform plain `REFERENCES`. (No dedicated cascade test is needed;
   the archival path is what's exercised.)
4. **`backup_runs` client-supplied PK** — insert with a chosen UUID succeeds;
   re-inserting the same UUID returns an error (PK violation surfaced as
   `Result::Err`, not a panic); `device_id`/`group_id` are taken from
   parameters (a test that the helper signature doesn't read them from a body).
5. **Issuance audit** — `record` snapshots bucket/prefix; later changing the
   config row does not mutate the issuance row. Indexes exercised by an
   ordered `(device_id, issued_at DESC)` query returning newest-first.
6. **Scan helpers** — `list_scheduled` returns only non-NULL-interval rows;
   `latest_success_*` filters to `purpose='backup'` + `outcome='success'` and
   ignores a newer `restore` success (the staleness-reset bug guard).
7. **`backup_repo_stats` split writers** — `upsert_repo_fields` then
   `upsert_bucket_bytes` accumulate without clobbering each other; either order.
8. **`backup_repo_snapshots` upsert** — second observation of the same
   `(group_id, source)` updates `latest_snapshot_at`/`observed_at` in place.
9. **`backup_requests`** — enqueue is upsert on `(server_id, purpose)`; `clear`
   removes; `pending_for_server` lists.

No HTTP/e2e here (those belong to the public-server and private-server specs).
Per repo memory: per-package tests while coding (`just test-package database`),
no final full-suite run.

---

## Open questions / decisions to make

1. **`backup_repo_stats` / `backup_requests` cascade.** RESOLVED (impl):
   **no cascade** — there is *no* cache/transient-vs-audit split. Every backup
   FK is plain `REFERENCES`, because groups/servers are archived (`deleted_at`),
   not hard-deleted, so cascade-vs-preserve never fires.
2. **`backup_repo_snapshots.server_id` FK on a server delete.** RESOLVED (impl):
   plain `REFERENCES servers(id)`, no `SET NULL`/cascade — same archival rule.
   The column is nullable only because a `source` server-id may not resolve at
   observation time, not for delete semantics.
3. **Enum representation.** RESOLVED (impl): `commons-types` enums for **all**
   the closed sets (`purpose`, `outcome`, `kind`, `mode`/`BackupRepoMode`, and
   `status`/`BackupConfigStatus` — `status` did not stay a plain `String`), each
   with a matching DB `CHECK`; the open `BackupType{Custom}` for the type name.
4. **Where `purpose`/`status` CHECK constraints live** — RESOLVED (impl): the
   closed enums carry **both** a DB `CHECK (... IN ...)` *and* the typed
   `commons-types` enum at the model layer.
5. **`backup_runs.id` collision response contract.** DB returns a PK-violation
   error; the *endpoint* decides whether a duplicate report is a 409 or an
   idempotent 204. That's a public-server decision, but the DB helper's
   error-mapping (does `record` map unique-violation to a typed `AppError`
   variant, or pass the raw diesel error?) should be settled here so the caller
   can match on it. Lean: map to `AppError::Conflict` so the caller can branch.
6. **`retention` validation surface.** RESOLVED (impl): code-level via the typed
   `RetentionPolicy::validate_floor()` (DB crate), called by the private-server
   write path; not a DB floor-CHECK (the only DB CHECK is `jsonb_typeof`).
7. **Indexing for the staleness scan.** The provided indexes cover the
   per-group/per-device "latest run" cuts. If the jobs scan ends up doing a
   `DISTINCT ON (server)` over `backup_runs` joined through
   `device_server_associations`, a covering index may be wanted — defer until
   the jobs query is concrete, then add in a follow-up migration.

---

## Backup types addendum (supersedes the relevant schema above)

Added after this spec: backups are keyed `(server, type)`, not `(server)`.
See the plan's "Backup types" section. Concrete deltas:

- **`server_group_backup_config`** drops `expected_interval` and
  `retention` — it's now repo-level only (`bucket`, `prefix`,
  `target_role_arn`, `region`, `repo_password_ref`, `status`).
- **New tables/models:**
  - `server_backup_capabilities(server_id, type, enabled, registered_at)`
    PK `(server_id, type)` — bestool-registered; `enabled` **seeded from**
    `backup_type_defaults.auto_enable` at first registration, then
    operator-toggleable per server.
  - `backup_type_defaults(type PK, default_interval, default_retention
    JSONB, auto_enable BOOL)` — canopy-wide per-type defaults.
  - `server_group_backup_schedule(group_id, type, expected_interval,
    retention)` PK `(group_id, type)` — schedule/retention overrides over
    the type defaults; absent row → defaults.
- **`type TEXT` column added to** `backup_credential_issuances`,
  `backup_runs` (+ a `server_id` column for per-server-type staleness),
  `backup_repo_snapshots`, and `backup_requests` (PK now
  `(server_id, type, purpose)`). `backup_maintenance_runs` and
  `backup_repo_stats` stay per-group (repo-level).
- **New model surface:** capability upsert + per-server toggle; effective
  schedule/retention resolution (`override ?? type-default`, with the org
  retention floor enforced); "list active `(server, type)`" for the
  scheduler/staleness.
