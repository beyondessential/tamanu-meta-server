# A1 — Declare server maintenance to suspend incidents

Spec: [MNT](../../specs/monitoring/maintenance.md). The window is a target-scoped, time-bounded ceiling of `skipped` over every check, so it rides the existing policy chain rather than adding a parallel suppression path.

## Build

- [x] Migration + `schema.rs` entry for `maintenance_windows` (server xor group, `expected_end`, `note`, `declared_by/at`, `ended_at/by`, `settled_at`). Partial unique index on the open row per target.
- [x] `database::maintenance_windows`: declare (amends an open window), lift, list open, list for target, `suspends()` and its batch form, expiry and settle sweeps.
- [x] Grading: `ScopedCheckPolicy::chain_for`/`chains_for_scope` append a synthetic skipped ceiling while a covering window suspends. Every grading path picks it up with no call-site change.
- [x] `re_evaluate_incident_membership`: maintenance joins snoozed/silenced/unmonitored as a leave reason.
- [x] Declare, amend, and lift re-evaluate the target's open issues, as a silence does.
- [x] Sweeps in `monitor.rs`: end a window at its expected end, and re-evaluate the target once its settle period elapses.
- [x] Slack: `maintenance_declared` and `maintenance_ended` outbox kinds. Webhooks optional, so an unconfigured deployment records without posting.
- [x] Private-server `/api/maintenance` (declare, amend, lift, list, for-target) and regenerated OpenAPI.
- [x] UI: declare/amend/lift on server and group, the fleet view of open windows, maintenance marking on health and status dots, declare from an open incident, declare from an upgrade plan.
- [x] Tests: database integration cases, then Playwright for the surfaces.

## Decisions

- Suspension is `now() < COALESCE(ended_at, expected_end) + settle`, so grading is right even when a sweep is late, and settling needs no second state.
- The settle period is a fleet-wide constant, per the spec's "same for every window".
- Maintenance is not written into `scoped_check_policies`: silences are per (source, check) and operator-owned, and a window is neither.
