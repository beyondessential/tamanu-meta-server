---
id: RST
---

# Managed restore replicas

Canopy is the control plane for a fleet's *managed restore replicas*: standing replicas that Canopy decides should exist and keeps restored from the latest backups, driven through a restore consumer.
An external restore consumer — first-party infrastructure that restores backups into working Postgres replicas — is driven entirely by Canopy: Canopy declares which replicas should exist, hands out the snapshot to restore and short-lived read-only credentials for each, and records the restorability of every replica as the strongest backup-health signal.

## Scope

This spec covers *managed* restore replicas only: the standing replicas Canopy decides should exist and keeps current, and the restore-health signal they produce.

It does not cover an operator restoring a backup by hand.
An operator performing disaster recovery or an ad-hoc restore selects a specific snapshot for a specific server and restores it through that server's own device tooling and credentials — the existing per-server restore path, unchanged by this spec.
That path is operator-driven and server-scoped: the operator chooses what to restore and where, and Canopy only issues the read-only credentials and snapshot information for that one server.
Managed replicas are the opposite mode: Canopy chooses what should be restored, continuously, with no operator selecting each one.
The two modes share Canopy's read-only credential issuance and snapshot authority; they differ in who decides what gets restored.

## Why it exists

A backup is only as good as its last successful restore.
Producing snapshots (a device backed up) and confirming they landed in the repo (a snapshot exists) are weaker guarantees than actually restoring one into a live database.
Canopy already knows every group, every server, every backup type, and the latest snapshot for each — so it is the natural authority on *what should be restored*.
Centralising that decision in Canopy eliminates the long-lived AWS keys a restore consumer would otherwise hold, makes the restore consumer a stateless executor of Canopy's intent, and closes the lifecycle loop end-to-end: produced, persisted, restorable.

## Actors

A **restore consumer** is first-party infrastructure that restores backups and reports their health.
It holds no standing access to any backup repo and stores no list of what to restore: it asks Canopy what replicas should exist, restores them, and reports back.
It owns only the mechanics of restoration — how a replica is provisioned, where it runs, how much storage it gets, when it is torn down.

An **operator** declares, through Canopy, which replicas should exist and why.

Canopy owns the *what* and the *why* (which group, which server, which type, to what end, how fresh) and the *authority* (which snapshot, which credentials, is it restorable).
The consumer owns the *how*.
This boundary is load-bearing: Canopy never models a consumer's runtime placement, and a consumer never decides on its own what to restore.

## Identity and authorization

A restore consumer authenticates as a single device holding the `backup-restore` role.
The role is generic: any future restore consumer uses the same role with its own declared replicas.
A `backup-restore` device has no implicit server and no implicit group; it is not a member of any group it reads.

The role is read-only by contract, enforced at the API:

- A `backup-restore` caller requesting backup (write) credentials is rejected.
  The read-only guarantee is server-enforced, so a compromised consumer cannot pivot to writing or poisoning a repo.
- A `backup-restore` caller may obtain credentials and the worklist only for a `(group, type)` it has been authorised for.

Authorization is the set of declared replicas (below): a consumer is authorised for exactly the `(group, type)` pairs that appear in its enabled replica declarations.
There is no separate grant object — declaring a replica *is* the authorization to read what that replica needs.

A device reaches this role through one-off operator promotion, the same path a release-publishing device uses; no fleet-enrolment flow is involved.
Either transport Canopy already accepts for devices — tailnet identity or a client certificate — satisfies the role; the role, not the transport, is the contract.

## Consumer capabilities

A restore consumer advertises the set of intents it can satisfy, and registers it with Canopy when it starts and whenever it changes.
Canopy stores the set against the consumer and treats it as the authority on what that consumer can be asked to do.

The registered set governs two things:

- **What can be declared.** Canopy offers operators the intents the chosen consumer supports when they declare a replica.
- **What is dispatched.** A consumer's worklist includes only entries whose intent it currently supports; Canopy never asks a consumer to satisfy an intent it has not advertised.

When a consumer's set grows, the new intents become available for operators to assign, so a consumer gaining a capability is reflected without operator guesswork.
When a consumer's set shrinks, any enabled declaration whose intent is no longer supported becomes a *gap*: Canopy drops it from the worklist immediately and surfaces it to operators as a declaration no consumer can currently satisfy, to reassign or retire.
A gap is a configuration state shown to the operator, not a restore-health incident; the backups themselves are unaffected.

## Declared replicas

An operator declares replicas against Canopy.
Each declaration carries:

- the **group** whose repo holds the backups;
- the **type** of backup to restore;
- a **server** within the group, or all servers in the group when none is named;
- an **intent** describing what the replica is for;
- a human-readable **name**;
- a **freshness** bound: the maximum time the replica may go without a fresh successful restore before it is considered overdue — a bound on the consumer's *restore* cadence, deliberately independent of how often backups are produced (below);
- whether the declaration is **enabled**.

Intent is an open set; unrecognised intents are preserved verbatim rather than rejected, so a consumer may advertise intents Canopy does not model.
The well-known intents are:

- **verify** — a transient replica restored solely to prove the snapshot is restorable, then discarded; re-run on the freshness cadence.
- **analytics** — a persistent replica kept running for querying, refreshed to the latest snapshot on the freshness cadence.
- **disaster-recovery** — a periodic rehearsal of the full recovery path: a replica restored the way a real recovery would be, checked as a viable stand-in for the server, then discarded. It is the managed, automated counterpart to the operator-driven recovery in [Scope](#scope), not the recovery event itself.

A declaration's intent must be one the chosen consumer supports (see [Consumer capabilities](#consumer-capabilities)); a declaration whose intent is unsupported is a gap, surfaced to the operator and never dispatched.

A declaration scoped to a whole group expands to one replica per current server in that group.
Servers joining or leaving a group change what the consumer is asked to maintain, with no per-server operator action.

Declarations are managed through the operator interface (create, edit, enable/disable, delete) and are audited.
Deleting a declaration stops the consumer being asked to maintain that replica and revokes its authorization for that `(group, type)` if no other declaration covers it; recorded restore-health history is retained.

## The worklist

A restore consumer fetches its complete desired state from Canopy in one request, scoped to the calling consumer.
Canopy expands the consumer's enabled declarations — those whose intent the consumer currently supports — against the current servers and the latest known snapshot for each, and returns one entry per concrete replica:

- the declaration's identifier, group, server, type, intent, name, and freshness;
- the **snapshot to restore**: the snapshot identifier and its timestamp, or empty when no successful backup is yet known for that server and type;
- the repo coordinates needed to locate the backups (storage, bucket, prefix, region).

The worklist does not carry credentials or the repo password.
The consumer reconciles the worklist against what it is actually running — creating, refreshing, and tearing down replicas to match — and is responsible for converging on the desired state over time.

### Latest state, not a queue

Each entry names the *latest* snapshot for its `(server, type)`, not a backlog to drain.
A consumer restores on its own cadence and skips the intermediate snapshots produced since its last restore; restoring less often than backups are produced is expected, not a failure.
A restore can take far longer than the interval between backups — the data is slow to download and restore, and a persistent replica may be held up while its workload runs — so the consumer's restore cadence is independent of, and typically much slower than, the backup cadence.
Consequently a replica's **freshness** bound is set to cover the consumer's restore cycle (download, restore, and any hold), not the backup interval: setting it to the backup interval would alert continuously even when restores are keeping pace as designed.

### Snapshot authority

The snapshot Canopy hands out for a `(server, type)` is the snapshot identifier of that server's most recent successful backup run of that type.
This is the same snapshot the operator interface shows as the server's latest.
Canopy's independent repo inventory corroborates the snapshot's existence and timestamp; it is not currently the source of the identifier.

## Credentials

A consumer obtains credentials per `(group, type)` as it works, not for the whole fleet at once.
Canopy verifies the caller has an enabled declaration covering that `(group, type)`, then issues:

- short-lived read-only object-storage credentials scoped to the group's repo;
- the repo password.

The credentials permit reading the repo and nothing else; they cannot write, overwrite, or delete.
Each issuance is audited.
Absence of a covering declaration is a definitive refusal, not a transient error, and a consumer surfaces it as a clear failure for the operator to diagnose by inspecting the declaration in Canopy.

The 1-hour lifetime of an issued credential does not bound restore duration: a consumer refreshes credentials as needed across a long restore.

## Restore-health reporting

A consumer reports the outcome of each replica back to Canopy.
A report carries:

- the declaration, group, server, and type it concerns;
- the **snapshot** that was restored, joining the report to the produced-and-persisted record for that snapshot;
- the **outcome** — restored-and-healthy, or failed — and, on failure, an error description;
- whether the restored database came up healthy, and its Postgres major version;
- when the restore was observed;
- the object-storage traffic the restore moved;
- optionally, arbitrary health data the consumer chooses to attach (e.g. cluster statistics, whether indexes needed fixing). Canopy stores and displays it as-is without interpreting it; specific fields may later be promoted to first-class, queryable form.

Restored-and-healthy means the snapshot restored, the database started, and the consumer's readiness checks passed — a stronger statement than a snapshot merely existing.
A failure covers any stage: the restore itself, the database failing to come up, or a readiness check failing.

Reports are retained indefinitely as an audit trail.

## Alerting

A failed or overdue restore-health report is a group-level incident that pages regardless of any individual server's monitoring state, because an unrestorable backup is a control-plane and data-safety concern, not one server's operational noise.

A failure raises a group-scoped restore-verification alert identifying the affected server and snapshot.
Each server's restore-health is tracked independently, so one server's failed restore does not mask or merge with another's.
The alert recovers when that server's next report for the same type is healthy.

A replica with no recent healthy report within its freshness bound is overdue and raises the same alert; Canopy detects this on a periodic sweep rather than waiting for a report that never arrives.

## Out of scope

- How a consumer provisions, runs, names, or tears down a replica.
- A consumer's runtime placement, storage sizing, or scheduling.
- Scoping object-storage credentials below the granularity of a group's repo: one repo holds all of a group's servers' snapshots, so credentials are necessarily group-wide while targeting and reporting are per-server.
- Longer-lived or non-chained credentials: a consumer refreshes within a restore, so the per-issuance lifetime is not a constraint.
