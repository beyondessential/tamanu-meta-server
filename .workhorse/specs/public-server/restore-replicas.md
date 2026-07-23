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
Each advertised intent carries:

- a stable **name**, an open identifier for the intent;
- a human-readable **description**, shown to operators choosing an intent;
- a set of **semantics**: well-known tags, defined by Canopy, that the consumer opts the intent into, each granting Canopy a specific behaviour for that intent (see [Semantics](#semantics));
- a **parameter schema**: named, typed settings the consumer accepts per replica of the intent, each optionally with a default (see [Parameters](#parameters)).

Canopy stores the advertised set against the consumer and treats it as the authority on what that consumer can be asked to do and how.
Registration replaces the consumer's whole advertised set.

The advertised set governs three things:

- **What can be declared.** Canopy offers operators the intents the chosen consumer advertises when they declare a replica, along with each intent's description and parameter fields.
- **What is dispatched.** A consumer's worklist includes only entries whose intent it currently advertises; Canopy never asks a consumer to satisfy an intent it has not advertised.
- **How Canopy behaves.** The intent's semantics select Canopy's dispatch and alerting behaviour for its replicas.

When a consumer's set grows, the new intents become available for operators to assign, so a consumer gaining a capability is reflected without operator guesswork.
When a consumer's set shrinks, any enabled declaration whose intent is no longer advertised becomes a *gap*: Canopy drops it from the worklist immediately and surfaces it to operators as a declaration no consumer can currently satisfy, to reassign or retire.
A gap is a configuration state shown to the operator, not a restore-health incident; the backups themselves are unaffected.

Intent is an open set defined by consumers; Canopy fixes no canonical list.
For example, an intent that restores a snapshot solely to prove it is restorable and then discards it opts into `check` and `once`; an intent that keeps a queryable replica running opts into `check` and `url` and accepts parameters such as a minimum uptime and a size cap.

### Semantics

A semantic is a Canopy-defined behaviour an intent opts into.
Canopy acts only on the semantics it recognises; an unrecognised semantic is stored and preserved but changes no behaviour, so a consumer may advertise ahead of Canopy support.
The recognised semantics are:

- **check** — the intent produces restore-health feedback.
  Canopy expects a report for each of the intent's replicas and holds it to an overdue bound (see [Alerting](#alerting)).
  An intent without `check` is dispatched but never reported on nor alerted.
- **once** — a given snapshot is dispatched to the intent at most once.
  Canopy omits a replica from the worklist once the intent has a healthy report for that replica's current snapshot, and reinstates it only when a newer snapshot exists.
  Without `once`, the intent is always pointed at the latest snapshot and manages its own refresh.
- **url** — the intent's health report carries a link to the running replica within its attached health data, which Canopy surfaces to operators.

### Parameters

An intent's parameter schema names the settings the consumer accepts for each replica of that intent.
Each parameter has a type — a duration, a size in bytes, a boolean, an integer, or text — and may carry a default.
Canopy uses the schema to collect values when a replica is declared, validates the values against it, stores them with the declaration, and returns them in the worklist so the consumer receives its per-replica settings.
Canopy does not interpret parameter values beyond their type; they are settings passed through to the consumer.

Every parameter is optional: an operator may leave any one unset.
The worklist carries a resolved value for each parameter the intent advertises, and only for those: an unset parameter that has a default is sent as its default, and an unset parameter without a default is sent as JSON `null`.
A value for a parameter the intent no longer advertises is preserved with the declaration rather than rejected, mirroring how an unrecognised intent is preserved, but is not sent in the worklist.

## Declared replicas

An operator declares replicas against Canopy.
Each declaration carries:

- the **group** whose repo holds the backups;
- the **type** of backup to restore;
- a **server** within the group, or all servers in the group when none is named;
- an **intent** the chosen consumer advertises;
- a human-readable **name**;
- **parameter values** for the intent's schema, defaulted where the schema provides one;
- an **overdue bound**: the maximum time a replica may go without meeting its intent's health expectation before Canopy considers it overdue, interpreted per the intent's semantics (see [Alerting](#alerting));
- whether the declaration is **enabled**.

A declaration's intent must be one the chosen consumer advertises (see [Consumer capabilities](#consumer-capabilities)); a declaration whose intent is unadvertised is a gap, surfaced to the operator and never dispatched.

A declaration scoped to a whole group expands to one replica per current server in that group.
Servers joining or leaving a group change what the consumer is asked to maintain, with no per-server operator action.

Declarations are managed through the operator interface (create, edit, enable/disable, delete) and are audited.
Deleting a declaration stops the consumer being asked to maintain that replica and revokes its authorization for that `(group, type)` if no other declaration covers it; recorded restore-health history is retained.

## The worklist

A restore consumer fetches its complete desired state from Canopy in one request, scoped to the calling consumer.
Canopy expands the consumer's enabled declarations — those whose intent the consumer currently advertises — against the current servers and the latest known snapshot for each, and returns one entry per concrete replica the consumer should currently act on:

- the declaration's identifier, group, server, type, intent, name, and overdue bound;
- the resolved **parameter values** for the replica, one per parameter the intent advertises (unset parameters resolved to their default, or JSON `null` when they have none);
- the **snapshot to restore**: the snapshot identifier and its timestamp, or empty when no successful backup is yet known for that server and type;
- the repo coordinates needed to locate the backups (storage, bucket, prefix, region).

The worklist does not carry credentials or the repo password.
The consumer reconciles the worklist against what it is actually running — creating, refreshing, and tearing down replicas to match — and is responsible for converging on the desired state over time.

An intent carrying `once` contributes an entry only while the latest snapshot for its `(server, type)` has no healthy report; once that snapshot is verified the entry is omitted until a newer snapshot exists.
An intent without `once` always contributes an entry naming the latest snapshot.

### Latest state, not a queue

Each entry names the *latest* snapshot for its `(server, type)`, not a backlog to drain.
A consumer restores on its own cadence and skips the intermediate snapshots produced since its last restore; restoring less often than backups are produced is expected, not a failure.
A restore can take far longer than the interval between backups — the data is slow to download and restore, and a persistent replica may be held up while its workload runs.
A `once` intent verifies each snapshot at most once and is not asked to restore again until a newer snapshot exists, so its work follows the backup cadence rather than the clock; an intent without `once` is always pointed at the latest snapshot and refreshes on its own cadence.

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
A consumer may include an optional run correlation identifier with a credential request; Canopy records it on the issuance so the run is tied to its later health report.
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
- optionally, the same run correlation identifier used when the credentials were obtained, tying the report to its issuance;
- optionally, arbitrary health data the consumer chooses to attach (e.g. cluster statistics, whether indexes needed fixing). Canopy stores and displays it as-is without interpreting it; specific fields may later be promoted to first-class, queryable form.

When the intent carries the `url` semantic, the attached health data includes a link to the running replica, which Canopy surfaces to operators alongside the report.

Restored-and-healthy means the snapshot restored, the database started, and the consumer's readiness checks passed — a stronger statement than a snapshot merely existing.
A failure covers any stage: the restore itself, the database failing to come up, or a readiness check failing.

Only intents carrying `check` are expected to report; an intent without it is dispatched but produces no restore-health signal and is never alerted on.

Reports are retained indefinitely as an audit trail.

Canopy derives each restore's duration from the interval between its first credential issuance and its report.
A restore for which credentials were issued but no report has arrived is shown as in progress while the credentials remain valid, otherwise as a restore whose outcome is unknown; this surfaces in-flight and terminated-without-report restores in the operator view, including those under intents that produce no health report.

## Alerting

A failed or overdue restore-health report raises a restore-verification check on the affected server, subject to the same monitoring and incident gates as any other of that server's checks.

Restore-health is tracked independently per server, type, and intent: the affected server is the check's scope, and the type and intent name it, so one replica's failed restore does not mask or merge with another's, and the snapshot is carried in the check's detail.
The check recovers when the next report for the same server, type, and intent is healthy.

A replica is also overdue — raising the same check on a periodic sweep, rather than waiting for a report that never arrives — when it has not met its intent's health expectation within the declaration's overdue bound.
For an intent carrying `once`, the expectation is measured against the latest snapshot: the replica is overdue when the latest snapshot has gone unverified for longer than the bound, not merely because time has passed since an earlier snapshot was verified.
For an intent without `once`, it is measured against wall-clock time since the last healthy report.
Overdue applies only to intents carrying `check`.

## Out of scope

- How a consumer provisions, runs, names, or tears down a replica.
- A consumer's runtime placement, storage sizing, or scheduling.
- Scoping object-storage credentials below the granularity of a group's repo: one repo holds all of a group's servers' snapshots, so credentials are necessarily group-wide while targeting and reporting are per-server.
- Longer-lived or non-chained credentials: a consumer refreshes within a restore, so the per-issuance lifetime is not a constraint.
