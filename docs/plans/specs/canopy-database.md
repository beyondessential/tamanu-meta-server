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

Provide the persistent state for:

- **`server_group_backup_config`** — per-group backup configuration + lifecycle
  status (one row per configured group; `group_id` PK → `server_groups`).
- **`backup_credential_issuances`** — audit log of every STS credential issuance.
- **`backup_runs`** — what bestool reported per backup/restore run (client-minted UUID PK).
- **`backup_maintenance_runs`** — Canopy-owned maintenance-Job outcomes.
- **`backup_repo_snapshots`** — ground-truth inventory from the read-only inspection Job.
- **`backup_repo_stats`** — cached repo + bucket size/stats for operator display.
- **`backup_requests`** — pending operator one-off "backup now" flags.

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
  **Recommended: one migration `backup_credentials` creating all seven tables**
  — they're a single feature landing together, with no data backfill needed
  (all net-new tables), so splitting buys nothing. Keep `down.sql` a clean
  `DROP TABLE ... ;` in reverse-dependency order.
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
- **Arbitrary JSONB** (`retention`) maps to `serde_json::Value` directly —
  diesel handles `Jsonb -> serde_json::Value` natively (see `statuses.health`,
  `statuses.extra`). The column is `JSONB NOT NULL`; in code we treat it as a
  kopia keep-policy object. Do **not** invent a `TagMap`-style newtype unless a
  validated shape is wanted — `serde_json::Value` matches existing JSONB columns.
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
- **Schema regen verification**: after the migration, `schema.rs` gains seven
  `diesel::table!` blocks, plus `joinable!` and `allow_tables_to_appear_in_same_query!`
  entries. Confirm `bigserial` surfaces as `Int8`, `JSONB` as `Jsonb`,
  `INTERVAL` as `Interval`/`Nullable<Interval>`.

---

## Migration: `backup_credentials`

`up.sql` creates the seven tables below. DDL is normative (it is what the
diesel schema regen reads); the design doc's snippets are the source. Notes
on the **decided FK semantics** are load-bearing and called out per table.

### `server_group_backup_config`

```sql
CREATE TABLE server_group_backup_config (
    group_id          UUID PRIMARY KEY REFERENCES server_groups(id) ON DELETE CASCADE,
    bucket            TEXT NOT NULL,
    prefix            TEXT NOT NULL DEFAULT '',
    target_role_arn   TEXT NOT NULL,
    region            TEXT,
    expected_interval INTERVAL,
    retention         JSONB NOT NULL CHECK (jsonb_typeof(retention) = 'object'),
    repo_password_ref TEXT NOT NULL,
    status            TEXT NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
SELECT diesel_manage_updated_at('server_group_backup_config');
```

- **`ON DELETE CASCADE` is intentional** here (config is derived state, not
  audit): deleting a `server_groups` row removes its config. This is the *only*
  backup table that cascades — contrast the audit tables below.
- `status` is a free-text column validated in code against
  `{provisioning, escrow_pending, ready}`. Follow the codebase pattern of
  storing enums as `TEXT` and validating at the API/model layer rather than a
  PG enum or a diesel-orphan-rules dance (see `issues.resolved_reason`,
  `servers.kind`). A `CHECK (status IN (...))` is acceptable and cheap; prefer
  it for a closed three-value set.
- The `jsonb_typeof = 'object'` CHECK mirrors the `tags` columns and guards
  against a stray array/scalar landing in `retention`.

### `backup_credential_issuances`

```sql
CREATE TABLE backup_credential_issuances (
    id                  BIGSERIAL PRIMARY KEY,
    device_id           UUID NOT NULL REFERENCES devices(id),
    group_id            UUID NOT NULL REFERENCES server_groups(id),
    issued_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at          TIMESTAMPTZ NOT NULL,
    purpose             TEXT NOT NULL,
    sts_assumed_role    TEXT NOT NULL,
    sts_request_id      TEXT,
    access_key_id       TEXT,
    bucket              TEXT NOT NULL,
    prefix              TEXT NOT NULL
);
CREATE INDEX ON backup_credential_issuances (device_id, issued_at DESC);
CREATE INDEX ON backup_credential_issuances (group_id, issued_at DESC);
```

- **No CASCADE on `group_id`/`device_id` — deliberate audit preservation.**
  Deleting a `server_groups` (or `devices`) row that has issuance history
  **fails** the FK until the operator deals with it (archive/detach). This is
  the decided "no-CASCADE audit-FK" rule (design §"Decommissioning a group").
  Do not add `ON DELETE CASCADE`/`SET NULL`.
- `bucket`/`prefix` are **snapshots at issuance time**, not FKs back to config.
- `purpose` is `TEXT` validated in code (`backup`|`restore`).

### `backup_runs`

```sql
CREATE TABLE backup_runs (
    id              UUID PRIMARY KEY,
    device_id       UUID NOT NULL REFERENCES devices(id),
    group_id        UUID NOT NULL REFERENCES server_groups(id),
    purpose         TEXT NOT NULL,
    outcome         TEXT NOT NULL,
    error           TEXT,
    bytes_uploaded  BIGINT,
    snapshot_id     TEXT,
    reported_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX ON backup_runs (group_id, reported_at DESC);
CREATE INDEX ON backup_runs (device_id, reported_at DESC);
```

- **`id` is a client-supplied UUID** (the run-uuid bestool mints at run start),
  **not** `gen_random_uuid()` and **not** `BIGSERIAL`. No `DEFAULT`. The
  `New`-side insert provides it. A duplicate `id` fails its own insert (PK
  violation) — that's the intended safety (design §`backup_runs`); the model
  helper should surface that as a clean error, not panic.
- `device_id`/`group_id` come from the authenticated `ServerDevice` context in
  the caller, **never** from the client body — the model helper takes them as
  parameters (see contract below), it does not read them from a deserialized
  client struct.
- No CASCADE on the FKs (audit table — same rule as issuances).
- For the staleness scan, the hot query is "latest successful `purpose='backup'`
  run per server/group"; the `(group_id, reported_at DESC)` index serves it.

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

- No CASCADE on `group_id` (audit table).
- `outcome` NULL = still running; the model helper has a `start()` (insert,
  returns the new `i64` id) and a `finish(id, outcome, error, bytes_reclaimed)`
  update — the Job-side caller (jobs crate) owns the start/finish bracket.

### `backup_repo_snapshots`

```sql
CREATE TABLE backup_repo_snapshots (
    group_id           UUID NOT NULL REFERENCES server_groups(id),
    source             TEXT NOT NULL,
    server_id          UUID REFERENCES servers(id),
    latest_snapshot_at TIMESTAMPTZ,
    observed_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, source)
);
```

- Composite PK `(group_id, source)`. The inspection Job **upserts** per source
  (`ON CONFLICT (group_id, source) DO UPDATE`) — provide an `upsert` helper.
- `server_id` is parsed from `source` by the caller and is **nullable** (a
  source whose server-id no longer resolves still records). No CASCADE on
  either FK — but note `server_id` referencing `servers(id)` without CASCADE
  means a server delete with snapshot rows fails; that's consistent with the
  audit-preservation stance (the inventory is evidence). Flag if the operator
  workflow needs `SET NULL` here instead (open question below).

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
- This is a *cache*, not audit. CASCADE is defensible here (display-only,
  rebuildable). **Decision needed** (open question): cascade vs no-cascade.
  Lean **CASCADE** (it's not audit), but keep it separate from the
  audit-table rule so the distinction is explicit.

### `backup_requests`

```sql
CREATE TABLE backup_requests (
    server_id    UUID NOT NULL REFERENCES servers(id),
    purpose      TEXT NOT NULL,            -- "backup" | "restore"
    requested_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    requested_by TEXT,
    PRIMARY KEY (server_id, purpose)
);
```

- Keyed on `server_id` (per the design — one-off requests are server-scoped,
  cleared when the run is reported). Composite PK `(server_id, purpose)` means
  one pending request per (server, purpose); a second request is an upsert
  (refresh `requested_at`/`requested_by`).
- This is transient operator intent, not audit. CASCADE on `server_id` is
  appropriate (a deleted server's pending flag is meaningless).

`down.sql`: `DROP TABLE` all seven in any order (no inter-table FKs among
them; all FKs point at pre-existing tables).

---

## Diesel models + `lib.rs`

New module `crates/database/src/backups.rs` (single module for all seven
tables — they're one cohesive feature, mirroring how `issues.rs` holds
issues/events/incidents together). Register and re-export in `lib.rs`:

```rust
pub mod backups;
pub use backups::{
    ServerGroupBackupConfig, NewServerGroupBackupConfig,
    BackupCredentialIssuance, NewBackupCredentialIssuance,
    BackupRun, NewBackupRun,
    BackupMaintenanceRun,
    BackupRepoSnapshot,
    BackupRepoStats,
    BackupRequest, NewBackupRequest,
    BackupPurpose, BackupConfigStatus, MaintenanceKind, RunOutcome,
};
```

(Existing `lib.rs` re-exports `devices::*` and `bestool_snippets::*`; backups
is the same pattern. The string-enum newtypes are optional — see below.)

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

**Recommendation:** add `BackupPurpose { Backup, Restore }` and the run/maint
outcome enums to `commons-types` (so public-server, jobs, and the generated
`api-types.ts` share one definition), and keep `status` as a validated
`String` for now (three-value, config-internal). Whichever is chosen, the
model field types and the `CHECK` constraints must agree.

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
    #[schema(value_type = Option<i64>, format = "int64")]
    pub expected_interval: Option<PgDuration>,
    pub retention: serde_json::Value,
    pub repo_password_ref: String,
    pub status: String,
    #[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
    pub created_at: Timestamp,
    #[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
    pub updated_at: Timestamp,
}
```

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
- **`ServerGroupBackupConfig` row** — the single source for `bucket`, `prefix`,
  `region`, `target_role_arn`, `repo_password_ref`, `expected_interval`,
  `retention`, `status`. Consumed by: `GET /backup-target` &
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

- **`retention`** (JSONB): a kopia keep-policy object, e.g.
  `{"keep_latest":1,"keep_daily":7,"keep_weekly":4,"keep_monthly":6,"keep_annual":0}`.
  Stored as `serde_json::Value`; **floor enforcement** (`keep_daily≥7`,
  `keep_weekly≥4`, `keep_monthly≥6`) is **code-level in the config write path
  (private-server)**, not a DB constraint — the DB crate stores what it's given.
  Document this so a reviewer doesn't expect a DB CHECK.
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
3. **Cascade vs no-cascade** — the load-bearing FK behaviour:
   - Deleting a `server_groups` row **with** a config row but **no** audit rows
     cascades the config away (success).
   - Deleting a `server_groups` row that has `backup_credential_issuances` /
     `backup_runs` / `backup_maintenance_runs` rows **fails** (FK violation) —
     assert the error. This is the audit-preservation guarantee; test it
     explicitly so a future "helpful" CASCADE can't silently slip in.
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

1. **`backup_repo_stats` / `backup_requests` cascade.** Audit tables are
   firmly no-cascade. Stats is a rebuildable cache and requests are transient
   intent — both lean **CASCADE on group/server delete**. Confirm this is the
   intended split (audit = preserve+block; cache/transient = cascade) and that
   it's documented so the "no-CASCADE" rule isn't read as universal.
2. **`backup_repo_snapshots.server_id` FK on a server delete.** As speced it's
   `REFERENCES servers(id)` no-cascade, so deleting a server with snapshot rows
   blocks. Is the inventory "evidence" (block, like audit) or derived display
   (`SET NULL`/cascade)? The column is already nullable, so `ON DELETE SET NULL`
   is a clean option — decide.
3. **Enum representation.** `commons-types` enums (shared with public-server,
   jobs, private-web wire types) vs plain validated `String`. Recommendation
   above is enums for `purpose`/`outcome`/`kind`, `String` for `status` — needs
   sign-off since it touches the generated `api-types.ts`.
4. **Where `purpose`/`status` CHECK constraints live** — DB `CHECK (... IN ...)`
   vs code-only validation. The codebase leans code-only for enum-ish text
   (`servers.kind`), but the three-value `status` is a cheap CHECK win. Pick one
   and apply consistently.
5. **`backup_runs.id` collision response contract.** DB returns a PK-violation
   error; the *endpoint* decides whether a duplicate report is a 409 or an
   idempotent 204. That's a public-server decision, but the DB helper's
   error-mapping (does `record` map unique-violation to a typed `AppError`
   variant, or pass the raw diesel error?) should be settled here so the caller
   can match on it. Lean: map to `AppError::Conflict` so the caller can branch.
6. **`retention` validation surface.** Confirmed code-level (private-server
   write path), not DB. Flagged only so no one expects a DB floor-CHECK.
7. **Indexing for the staleness scan.** The provided indexes cover the
   per-group/per-device "latest run" cuts. If the jobs scan ends up doing a
   `DISTINCT ON (server)` over `backup_runs` joined through
   `device_server_associations`, a covering index may be wanted — defer until
   the jobs query is concrete, then add in a follow-up migration.
