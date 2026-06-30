# Canopy response to the PGRO restore-verification handoff

**From:** canopy
**To:** pgro
**Re:** `pgro/docs/canopy-handoff.md` (copied here as
`pgro-restore-verification-handoff.md`)
**Status:** needs pgro sign-off on the control-model inversion (§3) before
canopy freezes wire shapes and before pgro builds. Canopy will build its side
against the model below; pgro adopting the inverted executor model is pgro's
call.

The spec for canopy's side is `.workhorse/specs/public-server/restore-replicas.md`.

---

## 1. The handoff is conformant

Every load-bearing claim about canopy's current code checks out: the tailnet
node-identity auth and `TAILSCALE_REQUIRED_TAG` gate, the role-gating
extractor macro (note: an `admin` device passes every role-gated route), the
`securitySchemes` block, the `/backup-credentials` + `/backup-target` +
`/backup-report` handlers and their request/response shapes, the
session-policy → per-bucket STS role → repo-password flow, the `backup_runs` /
`backup_repo_snapshots` schemas, and the per-server-vs-group alerting split.
Three small inaccuracies, none of which change the design:

- Device roles are four, not three (`untrusted` is the auto-created pre-trust
  state). The role column is plain `TEXT` with no `CHECK`, so adding
  `backup-restore` is a code change, not a schema migration. "Cert minting"
  for the role is just operator trust-promotion (the releaser model) — no new
  enrolment machinery, exactly as you guessed.
- §4.4.1 reason 7 is wrong: `run_id` is *not* shared between
  `/backup-credentials` and `/backup-report`; the issuance audit row carries
  no `run_id`. The "don't reuse `/backup-report`" conclusion still holds on the
  other six reasons.
- The group-level alerting plumbing you call "concrete in PR #225" is already
  merged: `raise_group_event` exists and `restore-verification` is already a
  defined alert ref. The alert side is one call.

## 2. Two corrections that changed the wire shapes

These came out of review and both made it into the model below:

- **Canopy supplies the snapshot id.** Canopy already knows the latest snapshot
  per `(server, type)` (the latest successful `backup_runs` row). You should
  not list the repo to discover what to restore — canopy hands it to you.
- **Restore is per-server, not per-group.** A group holds many servers, each
  with its own snapshots inside the one shared per-group repo. Credentials are
  necessarily group-wide (one kopia repo per group bucket), but *targeting* and
  *health reporting* are per-server. `backup_restore_checks` and the
  restore-health report carry `server_id`.

## 3. The inversion: canopy drives, pgro executes

This is the part that needs your sign-off, because it changes pgro's
architecture. Rather than pgro statically defining what it restores (a
CRD-defined list of groups/servers) and pulling per-group, **canopy becomes
the source of truth for which replicas should exist, and pgro reconciles
against it.**

- An operator declares **replicas** in canopy: `(group, [server | all], type,
  intent, name, freshness)`. The declaration is both the work item and the
  authorization — there is no separate grant object.
- pgro fetches its **entire desired state in one call** —
  `GET /restore-worklist`, scoped to the calling consumer — and gets one entry
  per concrete replica: declaration id, group, server, type, intent, freshness,
  the snapshot to restore (`{snapshot_id, snapshot_at}` or empty), and the repo
  coordinates.
- pgro **reconciles**: create / refresh / tear down replicas to match the
  worklist, fetching `POST /restore-credentials {group, type}` per group as it
  goes.
- pgro **reports health** per replica: `POST /restore-verification` with the
  declaration, group, server, type, restored snapshot, outcome, replica
  health, Postgres version, and S3 traffic.

**The boundary:** canopy owns *what / why / how-fresh*; pgro owns *how* —
provisioning, placement, storage sizing, scheduling, teardown. Canopy never
models your runtime; you never decide what to restore.

**Intents** are an open set (`verify`, `analytics`, `disaster-recovery`, plus
anything you advertise). `verify` is transient (restore, prove, discard, re-run
on cadence); `analytics` is a persistent replica refreshed to latest on
cadence; `disaster-recovery` is a rehearsal. If canopy modelling your
*analytics/DR* replicas (not just
verification) is more centralisation than you want, say so — that is exactly
the boundary this sign-off is about.

What pgro keeps from the original handoff: one canopy device, promoted once;
read-only by contract (write creds rejected at the API for the role);
best-effort reporting that never blocks restore progress; no `consumer_instance`
(one device, per-replica audit lives in your own records).

## 4. Endpoint surface (shapes to be frozen on sign-off)

- `GET  /restore-worklist` → desired replicas (expanded per server) + per-group
  repo coordinates + the snapshot to restore for each.
- `POST /restore-credentials {group, type}` → short-lived read-only creds +
  repo password. Authorized iff an enabled declaration covers `(group, type)`.
  `purpose=backup` rejected for this role.
- `POST /restore-verification {replica, group, server, type, snapshot_id,
  outcome, error?, replica_healthy, postgres_version?, observed_at, s3_*}` →
  per-server restore-health; 204 on success.

## 5. Appendix A (bestool) deltas

The original A.2/A.3 (`restore_credentials`, `restore_target`) are replaced by
a worklist fetch plus per-group `restore_credentials`; `restore_target`
collapses into the worklist. A.1 `RestoreVerification` gains `server_id` (and a
declaration id). A.4 `restore_verification` is unchanged in spirit. Canopy will
restate the exact bestool deltas once you've signed off on §3 and the shapes
are frozen.

## 6. What canopy is building now

Two PRs:

1. **Control + access** — `backup-restore` role; the declared-replica model +
   operator UI; `GET /restore-worklist`; `POST /restore-credentials`.
2. **Health** — `backup_restore_checks` + `POST /restore-verification`;
   per-server group-level alert routing + recovery; the overdue-freshness sweep;
   restore-health surfacing in the operator UI.

Ping canopy if §3 is contentious; otherwise canopy freezes the shapes at the
end of PR1 and hands the restated Appendix A to bestool.
