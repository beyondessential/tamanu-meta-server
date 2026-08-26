# Remove unnecessary target_version_id from PGRO

Drop the redundant `target_version_id` UUID from the PGRO restore wire. On a
`migrate` worklist entry PGRO is given both `target_version` (semver) and
`target_version_id`, but only uses the semver — it fetches that version's
published migration artefacts the same way an upgrading server does — and echoes
the UUID back verbatim in its migration report. Canopy can resolve the version
from the semver itself, so the UUID buys nothing on the wire.

## Scope

Canopy-side only. This card makes Canopy's report endpoint tolerant and gives
PGRO a semver to report instead of the UUID. Removing `target_version_id` from
the worklist entry is a phase-2 cleanup, gated on the PGRO rollout, tracked as a
follow-up card.

Out of scope: the private-server `GroupVerdict.target_version_id` and
`Upgrades.tsx` are Canopy's internal admin API, not the PGRO wire — untouched.
The `migration_tests.target_version_id` FK column and all verdict/known-issue
logic keyed on it stay exactly as they are; only the wire changes.

## The redundancy

- **Worklist entry** (`WorklistEntry`, `crates/public-server/src/restore.rs`)
  carries `target_version: Option<String>` (semver) and
  `target_version_id: Option<Uuid>`. PGRO reads only the semver.
- **Migration report** (`MigrationArgs`, same file) carries a required
  `target_version_id: Uuid`, echoed straight back from the worklist entry.
- Canopy stores it as the `migration_tests.target_version_id` FK
  (`NewMigrationTest`, `crates/database/src/migration_tests.rs`) and joins on it
  for verdicts (`latest_test`, `verdict`, `has_verdict`, `latest_verdict_by_key`)
  and known-issue filing (`file_outcome`).

## Approach

Resolve the version from the semver on the report side instead of trusting an
echoed UUID. The existing `Version::get_by_version(db, VersionStr)` already
resolves a semver string to a `Version`, so no new query is needed.

The `migration_tests` table is unchanged: Canopy resolves semver -> version id
before the insert, so the internal model and every verdict join keep using the
id. No database migration.

## Wire changes (this card)

- **`MigrationArgs`**: add `target_version: Option<String>` (semver); make
  `target_version_id: Option<Uuid>` optional.
- **Report handler (`verification`)**: resolve the version id to store as
  follows —
  - prefer `target_version` (semver) when present, resolving it via
    `Version::get_by_version`; the semver is the going-forward source of truth;
  - fall back to `target_version_id` when the semver is absent (old PGRO);
  - reject the report when neither field is present, and when a supplied semver
    resolves to no known version. A `migrate` report that cannot be attributed
    to a version has nowhere to be stored (FK) and no verdict to reach.
- **Worklist entry**: keep sending `target_version_id` for now. Old PGRO echoes
  what it is given; withdrawing the field before old PGRO is gone would strand
  its report with neither identifier.

## Compatibility matrix

- **Old PGRO** (echoes UUID): worklist still carries the UUID; report carries
  `target_version_id`; Canopy resolves via the id. Works.
- **New PGRO** (reports semver): worklist still carries the UUID (ignored);
  report carries `target_version` semver; Canopy resolves via the semver. Works.

## Phase 2 (follow-up card, not this one)

Once the semver-reporting PGRO build is rolled out fleet-wide, drop
`target_version_id` from the worklist entry (`WorklistEntry`). Nothing reads it
by then.

## Tests to touch

- `crates/public-server/tests/it/restore.rs` — the worklist assertion on
  `target_version_id` (~line 1040) and the report body that echoes it (~line
  1197). Add coverage for a report that carries only the semver, and for the
  rejection when neither identifier is present / the semver is unknown.
- Regenerate the public-server OpenAPI (`just gen-openapi`) after the
  `MigrationArgs` change.

## Status

- [x] `MigrationArgs`: added `target_version` (semver), made `target_version_id`
      optional.
- [x] Report handler resolves the version — prefer semver via
      `Version::get_by_version`, fall back to the id, reject when neither is
      given (`BadRequest`/400) or the semver is unknown (diesel NotFound/404).
- [x] Replaced `From<MigrationArgs>` with `into_new(target_version_id)`.
- [x] Worklist entry left unchanged (still carries `target_version_id`
      transitionally).
- [x] Tests: happy path now reports by semver; added by-id back-compat,
      no-version-refused, and unknown-version-refused cases.
- [x] Hand-updated `crates/public-server/openapi.json` to match.
- [ ] **Needs a machine with the toolchain** (I can't run cargo/just here):
      `just gen-openapi` to regenerate the public-server spec authoritatively
      (my hand-edit is a best-effort approximation), `just test-package
      public-server`, `just check`, and `cargo fmt`.

## Spec

No spec change. `Pre-upgrade migration testing` in
`.workhorse/specs/public-server/restore-replicas.md` already describes the
report as carrying "the target version whose migrations were applied" (semver,
product-level) and never names the UUID. The UUID was an implementation detail
that over-delivered against the spec; removing it aligns the wire with what the
spec already says.
