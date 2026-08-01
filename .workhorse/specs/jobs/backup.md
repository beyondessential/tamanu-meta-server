---
id: BKJ
---

# Backup control plane

Canopy maintains, verifies, and watches over the fleet's backups itself — clients neither run maintenance nor hold the rights to.
This is the autonomous half of the backup system: the work Canopy does on a cadence with no device asking, and the health signals it raises from it.

## Scope

This spec covers Canopy's own background backup work: repo maintenance, inspection, storage metering, upstream preflight, and the detection and alerting that turn all of it into incidents.

It does not cover the device contract (see [BAK](../public-server/backup.md)), the operator's configuration of a group (see [BKO](../private-server/backup.md)), or restore-health (the managed restore replicas spec, `RST`).

Canopy acts only on groups whose configuration is ready, runs at most one operation per group at a time, and bounds how many groups it works on at once.

## Maintenance

Canopy runs each group's repo maintenance on a cadence — clients are never granted the rights to.
It enforces the group's retention as part of maintenance, and records every run's outcome so a stuck or failing maintenance is itself detectable.
Maintenance also re-asserts that the repo carries no repo-level object-lock retention mode of its own, healing a repo that was imported with one before Canopy disabled it (see [BKO](../private-server/backup.md)).
Beyond the cadence, an operator may request a one-off full maintenance run for a group (see [BKO](../private-server/backup.md)); Canopy runs it on the next scheduling opportunity, ahead of the jittered cadence slot, subject to the same one-run-per-group interlock — so a forced run never overlaps an in-flight one.

## Passphrase rotation

Canopy rotates each group's repo passphrase on a cadence, so a leaked passphrase is useful only until the next rotation rather than indefinitely.
Rotation is crash-safe: an interrupted rotation is reconciled on the next attempt, and throughout it the repo stays openable with either the previous or the new passphrase — it is never left unopenable.
Rotation contends with the rest of a group's background work for the one-operation-per-group interlock; losing it defers the rotation to the next opportunity within the same period rather than to the next period, so the cadence holds even when a group is persistently busy at its scheduled moment.
Like maintenance, rotation is Canopy's to do; operators never run it.

## Recovery escrow

Because Canopy holds the only copy of every passphrase, it continuously escrows the state needed to recover access without it: the per-group passphrases and repo coordinates, and the group, server, configuration, schedule, and capability records that frame them.
The escrow is encrypted to a set of offline recipient keys whose private halves Canopy never holds, and written to versioned, object-locked storage.
So Canopy can write the escrow but never read it back — a full Canopy compromise cannot disclose the escrowed secrets, and object-lock keeps past versions undeletable until they expire.
Recipients are mandatory: Canopy refuses to run without them, so there is never a silent recovery gap.
This is the escrow the operator recovery ceremony verifies (see [BKO](../private-server/backup.md)).

## Inspection

Canopy periodically inspects each group's repo against the storage directly, independent of what devices reported:

- it verifies repo integrity, and a failed verification is repo corruption;
- it inventories the repo — the latest snapshot per source — as the ground truth a device's report is reconciled against;
- it records repo size, logical and physical, and the storage cost basis for display;
- it records each snapshot's logical size and matches it to the device run that produced it by snapshot id, so the repo's own size stands in when a run reported none, and is cross-checked against the size a run did report. A snapshot's recorded size is written once, because a snapshot is immutable.

## Upstream preflight

Canopy watches its own access to each group's storage, so a broken control plane is caught at the source rather than when the fleet starts failing.
It checks that its identity resolves, that it can assume each group's role and perform a read-only no-op, and that the bucket's object-lock is present and at least the required retention.
Preflight only alerts; it never pulls Canopy out of service, because a failing check must not make a degraded situation worse.

## Detection

Canopy reconciles three sources — what a device reported, what credentials were issued, and what actually landed in the repo — and alerts on disagreement:

- **staleness** — a server with a prior successful backup but none recent, or one that has never backed up though it has been expected long enough.
- **reconcile** — a device reported a successful backup but no matching snapshot landed (the report is false or the upload didn't persist), or a fresh snapshot exists but no recent report (the reporting path is broken).
- **size** — a device reported a snapshot size that disagrees with the size the same snapshot occupies in the repo; only compared when both sizes are known and non-zero.
- **maintenance** — a group whose maintenance is overdue, or whose most recent maintenance failed.

## Alerting

Backup alerts are raised at one of two scopes:

- **Per-server** signals (staleness, never-backed-up, the report-gap, the size discrepancy) are subject to the server's monitoring gate: still recorded for visibility, but they contribute to an incident only when the server is monitored, because some servers are intentionally intermittent.
- **Group-level** signals (repo corruption, maintenance failure, missing-snapshot reconciliation, preflight failures, and restore-verification — see the managed restore replicas spec, `RST`) page regardless of any member's monitoring state, because they are control-plane or data-safety concerns that belong to no single server.

Each signal has a stable key by which operators silence or snooze it and by which the interface and notifications refer to it; the keys are a contract and are not renamed without migrating stored silences.
A signal recovers when the condition that raised it clears.
