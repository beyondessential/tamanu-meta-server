---
id: BAK
---

# Device backups

A machine's agent backs up to object storage that Canopy mediates: it holds no long-lived storage credentials and knows nothing of the bucket layout until Canopy tells it, per run.
Canopy is the control plane — it owns the credentials, the repo location, the passphrase, and the record of what ran.

Backup is a machine concern rather than an application one: what a run captures is a box's databases, configuration and filesystems, and a box shared by two workloads backs up once (see [FLT](../servers/overview.md)).

## Scope

This spec covers the device-facing contract: how a device learns what it may back up, obtains short-lived credentials and the repo coordinates for a run, reports its progress while the run is in flight, and reports the outcome.

It does not cover what an operator configures (see [BKO](../private-server/backup.md)), what Canopy does on its own — maintenance, inspection, detection, alerting (see [BKJ](../jobs/backup.md)) — or restoring backups (the managed restore replicas spec, `RST`).

## Identity and resolution

A device authenticates with the machine role (see [DTR](../private-server/device-trust.md)), over either transport Canopy accepts (a client certificate on the internet-facing path, or tailnet identity on the private mount).
Every device request resolves through the authenticated identity, never the request body: identity → its machine → that machine's group → the group's backup configuration.
A device belonging to no live machine is refused; a machine with no group, or whose group has no ready configuration, is refused.

Resolution stops at the machine and never reaches the applications on it, so a box running two workloads resolves the same way as one running a single workload.

## Capabilities

A device registers the backup types it can run on its machine.
A newly seen type is enabled for scheduling or not according to that type's fleet default; a type already known keeps the operator's setting.
Registration requires the machine to be grouped, but not the group's configuration to be ready.

What a type captures is the agent's to decide, and Canopy does not interpret it.
A type is a name Canopy schedules against, issues credentials for, and tracks snapshots under, with no view of what was written beneath it and no meaning attached to the name itself.
So a machine's backups are its own whatever it runs, and Canopy never derives from a type which of a box's workloads a snapshot belongs to.

## Credentials

A device requests credentials for a `(type, purpose)`.
Canopy issues short-lived credentials by assuming the group's dedicated cross-account storage role under a session policy that confines them to the group's bucket and prefix:

- **backup** purpose grants the write set kopia needs, including a version-less delete — but never deletion of a locked version, nor any weakening of object-lock or retention.
- **restore** purpose grants read-only access.

The credentials carry the storage role's identity for at most an hour; a device refreshes them as a run outruns that lifetime.
Every issuance is recorded before the credentials are returned.
A device may include the run identifier it minted (the same one it reports the run under) with the credential request; Canopy records it on the issuance so the issuance ties to the run, and derives the run's duration from the interval between the first such issuance and the report.

A `(type, purpose)` is issuable only when the type is an enabled capability of the machine, or an operator has queued a one-off request of that purpose for it; otherwise it is refused.
The group's configuration must be ready: until then the endpoints refuse, so a half-provisioned group cannot be written to.

### The restore window

Restore credentials read the group's entire backup history, so a device may mint them only while an operator has the machine's restore window open (see [BKO](../private-server/backup.md)).
A device whose machine has no open window is refused, and told that restores must be enabled in Canopy first.

The window is the machine's, matching what a restore does: a restore rewrites the box, so one is opened once for the box however many workloads it carries.
Credentials already minted keep their own short lifetime when the window closes, and no further ones are issued.

## Target

A device fetches the repo coordinates for its group each run: the storage kind, bucket, prefix, region, and the repo passphrase.
The passphrase is Canopy-owned and read from the group's secret store at request time; the device never stores it.

## Reporting

A device reports each run's outcome: the type and purpose, success or failure, an error when it failed, the resulting snapshot identifier, the bytes uploaded, the object-storage traffic the run moved, and the moment the run froze the data it backed up.
The run is keyed by an identifier the device mints at the start of the run; the device, machine, and group are taken from the authenticated context, so a device cannot report a run as another group's.
A duplicate run identifier is refused.
Reporting a run clears any matching operator one-off request, so the standing "back up now" prompt stops.

## Snapshot moment

The moment a run froze the data it backs up — the point in time the backup represents — is distinct from when the run finished uploading and from when Canopy received the report.
For a large backup those moments are hours apart, and the freeze may happen below the backup engine, so it is not recoverable from the repository afterwards.

A device reports that moment on a progress report, on the completion report, or on both; it is known before any transfer begins.
Canopy records it once per run: the first value seen stands and a later report does not overwrite it.
It is the device's assertion about its own clock, so Canopy stores it as reported and does not reconcile it against its own.
A run whose device never reports the moment has none, and Canopy falls back to the report time wherever the moment is needed.

## Progress

A device may report progress while a run is in flight, as often as it chooses.
Progress reporting is optional: a device that never reports it is treated exactly as one that cannot, and the run's own record is unaffected either way.
Progress is accepted for any device belonging to a live grouped machine, without requiring the group's configuration to be ready or the type to be issuable — it describes a run already under way, and refusing it would blind Canopy precisely when something is misconfigured.

Each progress report carries the run identifier, the type and purpose, and two independent sets of counters:

- **transfer counters** describing the backup engine's own work — source bytes read, bytes processed, bytes uploaded, bytes found already present, the total the run expects, files done and expected, and errors encountered and ignored — together with what the run is working on at that moment.
- **object-storage traffic counters** — the same raw and payload, sent and received figures a completed run reports — as tallied to that point.

All counters are cumulative from the start of the run rather than per-interval, so a lost or repeated report costs resolution but never corrupts a total.
A device omits any counter it does not measure.
A device may also include engine-specific detail Canopy makes no commitment about; Canopy stores and surfaces it verbatim without interpreting it.

Canopy times each progress report on receipt rather than trusting a device clock, and derives transfer rate, progress against the expected total, and time since last contact from the resulting series.
The series is retained for a bounded period after the run — long enough to review a past run's behaviour — and is not part of the run's permanent record.

Progress reports are rate-limited per device.
A progress report for a run already reported complete is accepted rather than refused, so a report racing the completion is not an error.
Where a completed run's report omits a figure the progress series already carries, Canopy takes the last value from the series; a figure the report does supply always wins.

## Guarantees

A compromised device cannot destroy backups.
Its credentials cannot delete a locked object version or weaken the bucket's object-lock; at worst it writes a delete-marker that object-lock and versioning leave recoverable.
Decommissioning a device is revoking its certificate: it can no longer obtain credentials, and any it already holds expire within the hour.

## Failure contract

The device endpoints distinguish: the caller belongs to no live machine; the machine is ungrouped, has no ready configuration, the type is not issuable, or a run identifier is duplicate; the caller is reporting progress faster than Canopy accepts; and Canopy's own dependency — the credential issuer or the secret store — is unavailable or unconfigured.
Each is a distinct, stable status so a device need not guess.
A refused progress report is never a reason for a device to abandon a run: progress is telemetry, and a run continues regardless of whether Canopy accepted the last report.
