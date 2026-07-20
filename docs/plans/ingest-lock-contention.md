# Ingest lock-contention hardening

Follow-up depth work after PR #379 ("defer incident evaluation off the
status-ingest path"). #379 removed the per-group `server_groups FOR UPDATE`
lock from the device `/status` request; this plan audits **all** database
access on and around the critical ingest pathway and eliminates the
remaining contention risks, plus adds the connection-level safety net that
would have bounded the original incident.

## Background

A deploy-time reconnect herd plus a Postgres switchover convoyed the fleet:
ingestion transactions held a per-group `server_groups` row lock for
minutes, `/status` timed out at 30s → 500, and the monitor's linger sweep —
which needs the same lock — starved, leaving recovered incidents stuck
"Recovering". #379 moved incident evaluation to a single-writer queue worker
so request traffic no longer takes that lock. This plan closes the rest.

## Goals

- No request-path transaction holds a **cross-server** exclusive lock.
- Every pooled connection has a bounded lock-wait and idle-in-transaction
  budget, so a stall can never convoy unbounded again (the incident had all
  three timeouts unset).
- The status-ingest write transaction is short: cut its per-check round-trip
  count from ~7N toward a small constant.
- No remaining background path accumulates cross-group locks across a long
  transaction.

## Non-goals

- Rewriting the ingest write model (append-only fast path with all
  derivation async). Considered and rejected as too large for this pass; the
  measures below achieve the goal without it.
- Indexing/re-shaping the slow operator `device /search` query (noted as a
  separate follow-up under Open questions).

## Audit: DB access on and around the ingest pathway

Classification: **PS** per-server (isolated), **PG** per-group, **PD**
per-device, **SHARED** cross-server key. "hot" = device-facing,
high-frequency.

### Status-ingest request `POST /status/{id}` (post-#379)

Verified there is **no cross-server lock held on the request in steady
state** — #379's goal is met. What remains is round-trip volume inside the
write transaction, not contention:

| Op | Table | Key | Scope | Note |
|----|-------|-----|-------|------|
| insert | `statuses` | new PK | PS append | partitioned, no lock |
| **upsert ×N** | `check_policies` | `(source,check_name)` | **SHARED** | `ON CONFLICT DO NOTHING`; steady-state no row lock, but N wasted writes; brief contention only on first sight of a new check name |
| select ×N | `check_policies` / `scoped_check_policies` | `(source,check_name)[+scope]` | SHARED/PS read | MVCC reads, not contention |
| select ×N | `servers` | `id` | PS read | **redundant** re-read per check inside `save_with_state` |
| `FOR UPDATE` ×N | `issues` | `(server_id,source,ref)` | PS lock | per-server; only same-server pushes serialise (intended) |
| update ×N | `issues` (`stamp_check_state`) | `id` | PS | |
| insert/update ×N | `issues` | `id`/PK | PS | |
| insert/`FOR UPDATE`/update ×D | `check_stability` | `issue_id` | PS | degraded-observed checks only |
| insert | `incident_reeval_queue` | `server_id` | PS | #379 queue enqueue; coalesces a server's burst |
| insert | `device_connections` (auth) | new PK | PD append | outside the write tx |

Per push ≈ **7N + 3D + ~15** round-trips (N = checks, D = degraded). The
dominant cost is ~7 statements per check inside the write transaction.

### Other hot-path / concurrent writers

Ranked by convoy risk (excluding the ingest path):

1. **`reconcile_open_incidents`** (`crates/database/src/issues.rs:1571`, monitor
   startup) — **highest remaining risk.** One transaction walks *every* open
   issue and accumulates per-group `server_groups FOR UPDATE` locks across the
   whole walk; fires at deploy time. #379 did not touch it.
2. **Monitor minute sweeps** via `raise_group_event` / `raise_global_event`
   (`monitor.rs` loop; `issues.rs:530`/`665`) — take the same per-group lock
   and the single global advisory lock across statements + a Slack enqueue.
   Low cadence, single pod; amplifier during a staleness storm, not a source.
3. `sweep_lingering_incidents` — per-incident transaction, `incidents` row
   lock only; bounded, low risk.
4. `register_complete` enrollment — per-server `FOR UPDATE` across round-trips
   but rate-limited and human-paced; low.
5. `pingtask`, `ownstatus`, `device_connections` middleware, `slacker_outbox`
   (SKIP LOCKED), `mcp touch_last_used` — append-only / correctly bounded /
   throttled. Negligible convoy risk (`device_connections` is a write-*volume*
   watch item only, not a lock risk).

## Workstreams

### WS0 — Find and fix the long-idle transaction

The ad-hoc 30s `idle_in_transaction_session_timeout` (since reverted) surfaced a
real flow that holds a transaction open and idle for tens of seconds. A
transaction that sits idle while holding locks is precisely the convoy shape
this plan targets, so it must be found and fixed structurally rather than
masked. Likely causes: a request handler that keeps a transaction open across a
slow non-DB `await` (external call, in-process work) between statements, or an
operator flow that begins a transaction and waits on further input/work.

Find it via `pg_stat_activity` on the primary (`state = 'idle in transaction'`,
long `now() - state_change`, capture the `query` of the last statement and the
`application_name`/`client_addr` → pod), and by auditing handlers for a
`db.transaction { … }` block that awaits non-DB work mid-transaction. Fix by
moving the slow work outside the transaction (do the reads/compute first, open
the write transaction last and keep it tight). This unblocks reintroducing a
scoped idle timeout under WS1.

### WS1 — Per-connection lock/idle timeouts (safety net)

The pool (`mobc`, `build_pool` at `crates/database/src/lib.rs:97`) exposes a
per-connection `custom_setup` hook (`AsyncDieselConnectionManager::new_with_config`,
`ManagerConfig.custom_setup`), which runs once per physical connection. Run
`SET` statements there, env-gated. The migrator bypasses the pool entirely
(`crates/database/src/bin/migrate.rs:55`), so pool-level timeouts can never
cap DDL.

Crucially, a per-connection `SET` affects **only canopy's own sessions**. The
`app` database role is **shared with other applications**, so a role-level
`ALTER ROLE app SET …` (or a cluster GUC) would silently change behaviour for
every other `app`-role user — which is what caused the outages when the idle
timeout was set that way by hand. Keep these in canopy's `custom_setup`; do
**not** use `ALTER ROLE` / cluster GUCs for them.

Three settings, deliberately different in reach:

- **`lock_timeout`** — caps time *waiting to acquire a lock*. Never touches a
  running, unblocked query. Kills convoys fail-fast. **All pools.** ~5s.
- **`idle_in_transaction_session_timeout`** — reaps a transaction sitting
  *idle between statements* past the threshold. It does not touch a running
  query, but it **does** abort a transaction that legitimately stays open and
  idle — and it is **not** the free safety net an earlier draft of this plan
  (and the ad-hoc prod change) assumed. The role-level
  `ALTER ROLE app SET idle_in_transaction_session_timeout = '30s'` applied by
  hand during the incident aborted a real long-idle flow and caused DB outage
  reports; it has since been reverted (`ALTER ROLE app RESET …`).

  **Prerequisite (WS0 below): find and fix the flow that holds a transaction
  open and idle for tens of seconds** — that idle-holding-locks transaction is
  itself a convoy source. Only *after* that is fixed should an idle timeout be
  (re)introduced, and then scoped to the **device-ingest pool only** at a
  value comfortably above the ingest path's real maximum. Do **not** apply it
  to private-server or jobs.
- **`statement_timeout`** — caps total statement execution; the only one that
  can kill a legitimately long *running* query. Apply to the **device-ingest
  (public) pool only**, generous (~30s) as a runaway backstop — nothing on the
  device path is legitimately long. **Not** on private-server (the operator
  `device /search` query is a legitimate 90-day seq scan, see Open questions)
  or jobs (reconcile/sweeps are legitimately long).

Scoping (env vars, following the `env_u64` pattern; set per-container in
`ops/pulumi/canopy/src/servers.ts`):

| Pool | `lock_timeout` | `idle_in_transaction` | `statement_timeout` |
|------|----------------|------------------------|----------------------|
| public-server | 5s | deferred to WS0 | 30s |
| private-server | 5s | — (unset) | — (unset) |
| jobs / monitor | 5s | — (unset) | — (unset) |
| migrator | n/a (bypasses pool) | n/a | n/a |

`lock_timeout` is the safe, immediately-shippable part of WS1. `statement_timeout`
follows on the ingest pool. `idle_in_transaction_session_timeout` is **not**
shipped here — it waits on WS0.

A killed statement rolls back its transaction (atomic, no partial state); on
the ingest path the device re-reports next cycle and the monitor reconciles,
so nothing is permanently lost.

### WS2 — Collapse the `check_policies` work

Replace the per-check `upsert_default` (N blind `INSERT … ON CONFLICT DO
NOTHING` to a shared row) and per-check `apply` reads with: one batched
`SELECT` of the policies for all checks in the push up front, insert only the
genuinely-missing `(source,check_name)` rows (rare, batched), and grade from
the loaded set. Removes the only shared-row *writer* on the request (killing
the first-sight contention point) and cuts ~2N round-trips.

### WS3 — Hoist the redundant per-check `servers` read

`save_with_state` re-reads `servers` by id on every check
(`crates/database/src/issues.rs`, the `Server::get_by_id` inside the per-issue
path). The handler already loaded the `Server`. Thread it (or load once per
push) instead of re-fetching per check. Saves N per-server reads.

### WS4 — Batch the per-check issue / check_state writes (best-effort)

The per-check find-or-create `FOR UPDATE` + `stamp_check_state` + insert/update
(~3N in-tx round-trips) are per-server (no cross-server convoy), but they
dominate transaction duration. Explore collapsing into set-based upserts over
the push's checks. Hardest piece; may land incrementally. Value: shorter write
transaction → per-server issue locks held briefly, complementing WS1.

### WS5 — Bound `reconcile_open_incidents`

Restructure the monitor startup reconcile (`crates/database/src/issues.rs:1571`)
from one giant transaction into bounded per-server (or per-group) transactions,
mirroring the #379 reeval worker, so it cannot accumulate `server_groups` locks
across the whole walk. Eliminates the top remaining convoy risk. Idempotency is
already the design intent, so per-unit commits are safe.

## Acceptance criteria

- No request-path (public/private) transaction acquires a cross-server
  exclusive lock (`server_groups FOR UPDATE`, global advisory). Verified by
  reading the post-change ingest path and by a test asserting the ingest
  transaction touches only per-server/append rows.
- Every pooled connection reports the configured `lock_timeout` and
  `idle_in_transaction_session_timeout`; the public pool additionally reports
  `statement_timeout`; private/jobs report `statement_timeout = 0`.
- `reconcile_open_incidents` commits per unit — no single transaction holds
  more than one group's `server_groups` lock at a time.
- Ingest write-transaction round-trips reduced from ~7N to a small constant +
  ≤ a few per check (WS2/WS3 measured; WS4 best-effort).

## Testing

- WS1: a test that opens a pooled connection and reads back `SHOW lock_timeout`
  / `SHOW idle_in_transaction_session_timeout` / `SHOW statement_timeout` for
  each pool variant (gate values via env in the test).
- WS2: existing `crates/public-server/tests/it/statuses.rs` policy/grading
  tests must stay green; add a test that a brand-new check name is registered
  exactly once and grading matches the pre-change result.
- WS5: a DB-level test that reconcile with open incidents across several groups
  commits per group and drives the same end state as today (reuse the
  incident-reeval test patterns).
- No full-suite runs locally; per-package while iterating, then CI.

## Sequencing (PRs)

1. **WS0** — find/fix the long-idle transaction (already causing prod aborts
   until reverted; unblocks the idle timeout). Ships first.
2. **WS1** — timeouts: `lock_timeout` (all pools) + ingest-pool
   `statement_timeout` immediately; `idle_in_transaction_session_timeout`
   (ingest pool) only after WS0. Includes the `ops/pulumi` env wiring.
3. **WS5** — reconcile bounding (independent; eliminates top remaining risk).
4. **WS2 + WS3** — ingest round-trip reduction (together; touch the same code).
5. **WS4** — issue/check_state batching (best-effort; may be split further).

## Open questions / follow-ups

- **`device /search`** (`crates/private-server/src/fns/devices.rs:809` →
  `Device::search_by_connection_ip`, `crates/database/src/devices.rs:940`):
  a 90-day sequential scan with an unindexable `ip::text LIKE '%…%'`. This is
  why private-server gets no `statement_timeout`. Separately worth making
  indexable (trigram index, or narrower bound) so private-server could later
  carry a cap too. `connection_count` (unbounded `COUNT(*)` over partitioned
  history) is a lesser instance of the same.
- Timeout values (5s / 30s / 30s) are starting points; tune from production
  latency once WS1 is live.
