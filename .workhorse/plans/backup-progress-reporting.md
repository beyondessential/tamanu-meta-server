# Backup progress reporting (Canopy side)

Specs: [BAK](../specs/public-server/backup.md) (device contract, snapshot moment,
progress), [BKJ](../specs/jobs/backup.md) (staleness anchor, pruning),
[BKO](../specs/private-server/backup.md) (operator view).

## Problem

A 600GB / 21h backup shows in Canopy as an in-flight row with no figures on it.
The only evidence a run exists is the credential issuance that started it, so
"is it actually uploading, or wedged" is unanswerable from Canopy — the data
exists in bestool's logs and goes nowhere.

Separately, Canopy knows when a run *reported*, never when it *froze the data*.
bestool commonly takes a filesystem-level snapshot below kopia, so that moment
is not recoverable from the repository afterwards, and a 21h run's data is a day
old by the time it lands.

This plan covers Canopy only. The bestool half is a separate handoff document;
the wire contract below is what Canopy will offer.

## Wire contract

`POST /backup-progress` in `crates/public-server/src/backup.rs`, `ServerDevice`
auth, mounted at root alongside the existing four. `204` on success.

```jsonc
{
  "run_id": "uuid",                  // required — the run-uuid bestool minted
  "type": "tamanu-postgres",         // required
  "purpose": "backup",               // default "backup"
  "snapshot_taken_at": "2026-07-27T04:12:00Z",   // optional, write-once

  // engine counters — all optional, all cumulative since run start
  "bytes_read": 0, "bytes_hashed": 0, "bytes_uploaded": 0,
  "bytes_cached": 0, "bytes_estimated": 0,
  "files_done": 0, "files_estimated": 0,
  "errors": 0, "ignored_errors": 0,
  "current_path": "/var/lib/…",

  // proxy tallies — same four names as /backup-report, cumulative
  "s3_sent_raw_bytes": 0, "s3_sent_payload_bytes": 0,
  "s3_received_raw_bytes": 0, "s3_received_payload_bytes": 0,

  "extra": {}                        // engine's raw blob, no schema commitment
}
```

Counter names are Canopy's, engine-agnostic, and bestool maps kopia onto them.
Pinning columns to kopia's own progress struct would mean a kopia upgrade
renames Canopy's schema, and bestool already does non-kopia work (the
filesystem snapshot).

Gating matches `/backup-report`, **not** `/backup-credentials`: live server
(412) and grouped (409) only — no ready-config gate, no capability gate. Plus
429 when rate-limited.

Cadence is bestool's to choose; Canopy only caps it. Rate limit keyed on
`device_id` through the existing `RateLimiter` in
`crates/public-server/src/ratelimit.rs` — 60 per 5-minute window. Note that
limiter is per-process, so with multiple replicas each gets its own window; it's
an abuse backstop, not a quota, same as the enrollment limiter it was built for.

A sample arriving after the run has reported is stored, not refused.

## Database

New migration via `just migration add_backup_run_progress`.

```sql
CREATE TABLE backup_run_progress (
	id                        BIGSERIAL PRIMARY KEY,
	run_id                    UUID NOT NULL,   -- no FK: no run row exists yet
	device_id                 UUID NOT NULL REFERENCES devices(id),
	group_id                  UUID NOT NULL REFERENCES server_groups(id),
	server_id                 UUID REFERENCES servers(id),
	type                      TEXT NOT NULL,
	purpose                   TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
	observed_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
	snapshot_taken_at         TIMESTAMPTZ,
	bytes_read                BIGINT,
	bytes_hashed              BIGINT,
	bytes_uploaded            BIGINT,
	bytes_cached              BIGINT,
	bytes_estimated           BIGINT,
	files_done                BIGINT,
	files_estimated           BIGINT,
	errors                    BIGINT,
	ignored_errors            BIGINT,
	current_path              TEXT,
	s3_sent_raw_bytes         BIGINT,
	s3_sent_payload_bytes     BIGINT,
	s3_received_raw_bytes     BIGINT,
	s3_received_payload_bytes BIGINT,
	extra                     JSONB NOT NULL DEFAULT '{}'
);
CREATE INDEX ON backup_run_progress (run_id, observed_at DESC);
CREATE INDEX ON backup_run_progress (group_id, observed_at DESC);
CREATE INDEX ON backup_run_progress (observed_at);   -- pruning

ALTER TABLE backup_runs ADD COLUMN snapshot_taken_at TIMESTAMPTZ;
```

Plain FK refs, no cascade — matches `backup_runs`.

`run_id` gets no FK because the run row doesn't exist until the report; the
table is self-describing on device/group/server/type/purpose for the same reason
`backup_credential_issuances` is.

`observed_at` is server-stamped, not device-supplied. Rate is derived from it,
and "is it moving from Canopy's vantage point" is a receipt-time question.
`snapshot_taken_at` is unavoidably a device clock claim; stored as reported.

Model work in `crates/database/src/backups.rs` (the existing home for the whole
backup family): `BackupRunProgress` + `NewBackupRunProgress`, plus queries for
latest-per-run, series-for-run, and latest-per-run-across-a-group (one query
feeding every in-flight row in the group view — do not do this per row).

After `just migrate`, scrub `schema.rs` against `main` and keep only the lines
this change adds.

## Read path

`crates/private-server/src/run_pairing.rs` keeps issuance-chain pairing as-is:
credentials are always minted before the first sample, so first-issuance remains
the earliest known start and `started_at` / `duration_seconds` are unchanged.

Progress adds a second correlation source — a sample carries `run_id`
explicitly — so an in-flight run is identifiable even when its issuance predates
`run_id` support. Worth wiring, since it's the path off the
`claim_chain_for_report` time-window guesstimate.

`RecentRun` in `crates/private-server/src/fns/backups.rs` gains live fields
populated from the latest sample of each in-flight run: cumulative bytes,
`bytes_estimated`, throughput over a trailing window, seconds since last sample,
`snapshot_taken_at`, and the proxy tallies. Present for `InProgress` rows,
absent otherwise.

Reported rows gain `snapshot_taken_at` from the run row.

New read endpoint for the chart series, by `run_id`, following the module's
existing bare-handler pattern. Regenerate with `just gen-openapi` — note both
public-server and private-server have committed `openapi.json` with drift tests.

### Report-time backfill

In the `/backup-report` handler: where the report omits `bytes_uploaded` or any
of the four traffic counters, take the last progress sample's value; a value the
report supplies always wins. Same for `snapshot_taken_at` — first value seen
stands, whether it arrived by progress or by report.

Because samples are cumulative, the last one is very nearly final. The figure is
therefore as-of-last-sample rather than exact; that's the accepted tradeoff for
having data at all from a sparsely-reporting client, and it keeps the size
discrepancy check in [BKJ](../specs/jobs/backup.md) fed.

## Staleness re-anchor

`crates/database/src/backup/staleness.rs` — `last_success` currently takes
`run.reported_at` from `BackupRun::latest_success_by_server_type_for_group`,
which selects with `DISTINCT ON … ORDER BY reported_at DESC`.

Both the selection and the anchor value move to
`COALESCE(snapshot_taken_at, reported_at)`. Changing only the anchor would let a
server's freshness travel backwards: run A reported 08:00 / taken 04:00 followed
by run B reported 09:00 / taken 03:00 would make the newer run the anchor and
age the server. Selection and measure must agree.

Reconcile and the report-gap signals stay on `reported_at` — see the spec note;
they assert the reporting path works, not anything about data age.

## Pruning

New `jobs::backup::progress_prune` module with the established `spawn()` shape,
wired in `crates/jobs/src/bin/backups.rs` next to the other loops. Deletes
`observed_at < now() - 14 days`.

Fleet-wide, not per-group, so it sits outside the one-run-per-group maintenance
interlock and never delays a group's real work.

## Frontend

`private-web/` — after `just gen-openapi`, types flow through `api-types.ts` into
`types.ts`.

1. In-flight rows in the group backups table and server detail: transferred vs
   expected with a progress bar, current rate, last-heard-from, snapshot-taken-at.
   An absent figure renders as unknown, never as zero.
2. Per-run throughput chart from the series — live and completed runs alike.
3. Proxy-vs-engine comparison, so divergence is visible.
4. Collapsible raw `extra` inspector.

Playwright coverage in `private-web/e2e/` as part of the same change, seeding via
`e2e/seed.ts` extended for progress rows. Run from `private-web/`, not the repo
root.

## Testing

- `crates/public-server/tests/it/` — progress accepted for a grouped live-server
  device; 412 unbound; 409 ungrouped; accepted with a non-ready config (the
  deliberate divergence from `/backup-credentials`); accepted after the run
  reported; rate limit returns 429.
- `snapshot_taken_at` write-once: progress-then-report keeps the progress value;
  report-only sets it; neither leaves it null.
- Backfill: report omitting traffic figures inherits the last sample; report
  supplying them wins.
- `crates/database/` — staleness anchoring: a run taken well before it reported
  is stale on data age though its report is fresh; a run with no
  `snapshot_taken_at` behaves exactly as today; anchor does not regress when a
  newer run has an older snapshot moment.
- Derivation: throughput from a two-sample series; single-sample run yields no
  rate rather than a bogus one; a dropped middle sample leaves the total correct.

## Deploy notes

**The staleness re-anchor changes detection semantics on deploy.** A
long-running backup's data is now correctly aged from when it was taken, so
servers sitting near their staleness threshold can change state. The shift is
asymmetric — only updated bestool clients populate `snapshot_taken_at`, so
nothing moves until the fleet rolls, and then it moves per-server as each one
upgrades. Check which servers are near threshold before rolling bestool, not
before deploying Canopy.

Migration is additive: a new table plus one nullable column. Deploying Canopy
ahead of any bestool change is a no-op — no device calls the endpoint, no run
carries a snapshot moment, and every existing path behaves as it does today.

## Deliberately not in scope

- **No stalled-backup check.** Thresholds for "quiet too long" can't be guessed
  before seeing what a normal 600GB run looks like; quiet stretches mid-backup
  are legitimate. Revisit with real series data.
- **No downsampling or summary rollup.** Prune, don't compact.
- **No cadence negotiation.** bestool picks, Canopy caps. Changing cadence means
  a bestool release; accepted.

## bestool handoff

The handoff document needs: the request shape above; counters cumulative from
run start, never deltas; `run_id` must be the same uuid used on
`/backup-credentials` and `/backup-report`; `snapshot_taken_at` sent as early as
it is known; a refused or failed progress post must never abort a run; and the
proxy counters must be the same tallies already sent on completion, sampled
rather than reset.
