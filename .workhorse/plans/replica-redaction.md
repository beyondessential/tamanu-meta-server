# Redact managed restore replicas

The restore consumer can now de-identify a replica before serving it: it
applies a dbt-shaped masking manifest to each restore that reaches Ready,
and holds the switchover until that settles. It advertises this as the
`redact` semantic on the `analytics` intent, plus three parameters
(`redaction_manifest_url`, `redaction_version_query`,
`redaction_version_fallback_to_base`), where setting the manifest URL is
what turns redaction on.

Canopy stores unrecognised semantics and passes parameters through
verbatim, so redaction already works end-to-end with an operator pasting a
URL. What canopy can't do is *know*: the consumer's redaction outcome lives
only in its own runtime state, so canopy can't tell a redacted replica from
a raw one, can't alert when masking only partly applied, and can't say why
a replica has frozen on stale data.

Spec: [RST](../specs/public-server/restore-replicas.md#redaction).

## Shape

**Canopy ships first.** The consumer's report body is a typed client
generated from canopy's OpenAPI, so the new report fields have to exist in
a released canopy before the consumer can compile against them. Everything
here lands without a consumer change and degrades to "no redaction data
yet" until the companion PR follows.

**Two asymmetric failure modes.** A *partial* redaction goes live and
reports success, so canopy currently shows a healthy replica and hands out
its URL while an unidentified set of columns is in the clear. A *failed*
redaction holds the switchover, so no report is sent at all and canopy sees
only a bare overdue once the declaration's bound elapses. The companion PR
fixes the second by reporting when redaction settles rather than when a
switchover that will never come completes — the restore genuinely is
healthy at that point, and a broken manifest host should stop suppressing
the backup-health signal.

**Redaction settings are canopy's, not the operator's.** `redacts` on the
declaration is the only answer to "is this replica meant to be masked". The
three parameters resolve from product configuration for any intent carrying
`redact`, whether or not the declaration redacts, so an operator can't
hand-set a manifest URL and have a replica redact behind canopy's back.
This is the RST#parameters carve-out.

**Per-server, not per-declaration.** `resolve_params` runs once per
declaration, outside the server loop, but the manifest depends on the
server's product. The redaction overlay goes inside the loop, alongside the
`migrate` candidate-version lookup it mirrors — including the `continue`
that withholds an entry rather than dispatching an unredacted replica.

## Product configuration

- [x] Add `RedactionManifest { url_template, version_query,
      fallback_to_base }` to `crates/commons-types/src/server/product.rs`
      with `&'static str` fields so `Caps` stays `Copy`, and a
      `redaction: Option<RedactionManifest>` field on `Caps`
- [x] `Product::Tamanu` carries
      `https://docs.data.bes.au/tamanu/v{version}/manifest.json`, the
      `local_system_facts` current-version query, and
      `fallback_to_base: true`; `Senaite` and `Canopy` carry `None`
- [x] Unit test asserting substituting a version into the template yields
      the URL shape Tamanu's docs deploy publishes

## Wire and storage

- [x] `just migration add_replica_redaction` — `redacts BOOLEAN NOT NULL
      DEFAULT FALSE` on `restore_replicas`; `redaction_outcome TEXT`,
      `redaction_manifest_version TEXT`, `redaction_columns_masked INT8`,
      `redaction_columns_skipped INT8`, `redaction_error TEXT` (all
      nullable) on `backup_restore_checks`
- [x] `RedactionOutcome` enum in `crates/commons-types/src/backup.rs`
      (`complete` / `partial` / `failed`) alongside `RunOutcome`, and
      `semantics::REDACT`
- [x] `RedactionArgs` on `VerificationArgs` in
      `crates/public-server/src/restore.rs`, `Option`, mirroring how
      `MigrationArgs` hangs off the same body
- [x] Fan the sub-object into the check row in the `verification` handler
      the way `MigrationTest::record` is called for `migration`
- [x] `redacts` through `NewRestoreReplica`, `RestoreReplicaUpdate`, and
      the `BackupRestoreCheck` / `NewBackupRestoreCheck` structs in
      `crates/database/src/restore.rs`
- [x] Scrub `schema.rs` against main before committing — `just migrate`
      regenerates it from the local DB and pulls in other branches' lines

## Worklist

- [x] In `crates/public-server/src/restore.rs`, read
      `descriptor.has_semantic(semantics::REDACT)` alongside `once` and
      `migrates`
- [x] Inside the server loop, look up `server.product.caps().redaction`;
      when the intent redacts and the product has no manifest, `continue`
      — the same withholding shape as a `migrate` server with no candidate
- [x] Overlay the three parameters onto the per-declaration `params` for a
      `redact` intent: resolved from the product manifest when `redacts`,
      JSON `null` when not, in both cases replacing whatever the
      declaration stored
- [x] Endpoint test: a redacting declaration over a mixed-product group
      yields entries only for Tamanu servers, with the manifest template
      resolved and the version query set
- [x] Endpoint test: a non-redacting declaration of the same intent sends
      all three parameters as `null` even when the stored values set them

## The redaction check

- [x] `refs::REDACTION` + a documentation ref in the check catalog,
      following `refs::MIGRATION_TEST`
- [x] `file_outcome`-style raise/recover in `crates/database/src/restore.rs`
      keyed `redaction:{type}:{intent}`, `Scope::Server`, `CANOPY_SOURCE`,
      `default_ceiling: Warning`, `default_escalates: false`
- [x] `partial` and `failed` raise with the outcome, manifest version and
      both column counts in the detail; `complete` recovers
- [x] Call it from the verification handler when the report carries a
      redaction sub-object
- [x] Recover an active redaction check when its declaration is deleted or
      re-scoped, alongside the existing restore-verification recovery at
      `restore.rs:460`
- [x] Write the check documentation markdown — it's the only thing telling
      an operator what a partial redaction means for the replica they were
      about to query
- [x] Database tests for raise on partial, raise on failed, recover on
      complete, and no check at all for a non-redacting declaration

## Artefact corroboration

- [x] Per-server redaction gap: for each server a redacting declaration
      covers, resolve its reported version and check for a `dbt-manifest`
      artefact via `Artifact::get_for_version`
- [x] Surface it on `RestoreReplicaView` in
      `crates/private-server/src/fns/restore_replicas.rs` as a list of
      affected servers rather than folding into the existing `gap` bool —
      the two have different causes and different fixes
- [x] Evaluate on list and on create/update, so an operator declaring a
      redacting replica for a version with no published manifest learns it
      at declaration time

## Operator interface

- [x] Redaction toggle in the declare/edit form in
      `private-web/src/components/RestoreReplicasSection.tsx`, shown only
      when the selected intent advertises `redact`, defaulting off
- [x] Hide the three canopy-owned parameters from the parameter form for a
      `redact` intent — they're not operator-settable in either state
- [x] Redaction state column on the declarations table: off / redacted /
      partial / failed, from the latest check
- [x] Promote the redaction fields to labelled rows in the expanded health
      row, above the existing `health_details` JSON dump
- [x] Redaction-gap indication on a declaration whose servers have no
      published manifest
- [x] `just gen-openapi`, commit `openapi.json` and `api-types.ts` for both
      servers — public-server's spec is drift-checked too

## Tests

- [x] Public-server endpoint tests for the verification body carrying each
      redaction outcome, and for a body with no redaction sub-object
- [x] Playwright coverage in `private-web/e2e/`: declaring a redacting
      replica, the parameter fields staying hidden, and a seeded partial
      report showing as partial on the table and in the expanded row —
      extend `e2e/seed.ts` for the new columns

## Deviations from the plan as written

- The redaction **gap** narrowed while building it. A server Canopy holds
  no version for was going to be a gap; it isn't, because the consumer
  resolves the manifest against the version in the data it restored, not
  against what the server last reported, so Canopy simply can't
  corroborate. Only two reasons survive, and only one of them
  (`product_has_no_manifest`) withholds the replica — the other lets the
  restore proceed and lets the consumer hold the switchover.
- The **enabled toggle** on the declarations list needed `redacts` added
  to it. `update` replaces every field, so the toggle would silently have
  cleared the flag on any redacting declaration.

## Companion change in the restore consumer

Not in this repo; blocked on a canopy release carrying the schema.

- [ ] Count skipped columns — the replica status carries only
      `redactionColumnsApplied` today, and the per-column skip logging has
      the information but doesn't total it
- [ ] Report when redaction settles as failed, with the restore reported
      healthy, instead of waiting on a switchover that is being held
- [ ] Accept that a replica whose redaction later succeeds reports twice
      for one snapshot: the check history is an append log and overdue keys
      off the latest healthy report
