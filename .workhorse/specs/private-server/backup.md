---
id: BKO
---

# Operator backup control

An operator configures, through Canopy, how a server group backs up: where its repo lives, on what cadence, with what retention, and which servers and types participate.
Canopy owns the repo passphrase throughout — it is generated or accepted once, stored in Canopy's secret store, and never handed back except through the audited recovery ceremony.

## Scope

This spec covers the operator-facing control surface: per-group backup configuration and its lifecycle, scheduling and retention, per-server participation, on-demand backups, the status view, and passphrase recovery.

It does not cover the device contract (see [BAK](../public-server/backup.md)) or Canopy's autonomous maintenance, inspection, detection, and alerting (see [BKJ](../jobs/backup.md)).

Reads are available to any tailnet user; changes require an administrator.

## Per-group configuration

A group has at most one backup configuration: the bucket, prefix, region, the cross-account roles Canopy assumes, the reference to the group's passphrase, and its placement and lifecycle state.

Placement is one of:

- **external** — the operator brings their own bucket and supplies the role ARNs Canopy will assume.
- **shared** — Canopy provisions and names a bucket in its own shared account; the operator supplies nothing about location.

A configuration is created once and its structural fields (bucket, roles, placement) are immutable; the region and the operational settings below are editable.
Decommissioning a group deletes its configuration row — which stops all credential issuance for the group — and deletes the Canopy-owned passphrase.
The bucket and its object-locked contents persist independently and are not Canopy's to delete; teardown is a separate, deliberate act gated by the lock window.

## Lifecycle and provisioning

A configuration moves from **provisioning** to **ready**; devices are refused until it is ready.
Creating a configuration sets it provisioning and asks Canopy to create or connect the repo; that work transitions the configuration to ready, or records the error it failed with so the operator sees why.
The operator interface depends only on these observable states, not on how provisioning is carried out.

A configuration may also be created or reconciled idempotently by machine — for infrastructure-as-code — under administrator-equivalent authentication, with the same probe and provisioning behaviour as the interactive path.

## Setup and the passphrase

When a configuration is created, Canopy probes the target bucket and classifies it: empty, an existing kopia repo, holding unrelated content, or inaccessible.
The classification chooses the mode:

- **from-birth** — an empty bucket; Canopy generates a fresh passphrase and creates a new repo.
- **passphrase** — an existing repo; the operator supplies its passphrase and Canopy connects to it. On adoption Canopy disables any repo-level object-lock retention the existing repo carries: immutability is the bucket's Object Lock and expiry is Canopy's maintenance, and a live repo-level retention mode would block both device writes and maintenance reclamation.

A bucket holding unrelated content is refused rather than written into; Canopy never deletes to make room.
Either way Canopy creates and owns the passphrase secret, and configuration and secret are created together — if the secret cannot be stored, the configuration is rolled back, so a configuration never exists without its passphrase.
The supplied or generated passphrase is only the starting point: Canopy rotates it on a cadence thereafter (see [BKJ](../jobs/backup.md)), and the recovery ceremony recovers whatever the current passphrase is.

## Scheduling and retention

Each `(group, type)` has an expected backup interval and a retention policy, taken from a per-`(group, type)` override when set, otherwise from the fleet-wide default for that type.
A manual-only type has no interval and is backed up only on an explicit request.
Retention is floored to an organisational minimum; a configuration may deliberately opt out of the floor, which is recorded as the dangerous choice it is.

## Participation and on-demand

A server participates in a type when that type is an enabled capability on it; an operator toggles participation per `(server, type)`.
An operator may queue a one-off backup — or restore — for a `(server, type)` to run on the next cycle, and may cancel a queued one before it runs.
An operator may also request a one-off full maintenance run for a group, to reclaim storage or apply repo-settings changes without waiting for the scheduled cadence, and may cancel it before the scheduler picks it up (see [BKJ](../jobs/backup.md)); at most one such request is pending per group.

## Status

The operator can see, per group: the repo's size and cost basis, recent runs with their outcomes and errors, recent maintenance, the latest snapshot per server, and any in-flight or pending one-off requests — including a pending on-demand full maintenance request and who made it.

Each run's duration is the time from its first credential issuance to its report; a run that re-issues credentials while it runs is measured from the first issuance of the sequence.
A run for which credentials were issued but no report has arrived is shown from that issuance: in progress while the credentials remain valid, otherwise as a run whose outcome is unknown. This makes runs that don't report — such as an ad-hoc restore — visible; a later report for the same run replaces its issuance-derived entry.

## Passphrase recovery

Because Canopy owns the only copy of each passphrase, the ability to recover it without Canopy is verified, not assumed.
Recovery is a ceremony: a passphrase is escrowed encrypted to a set of offline recipient keys, and an operator periodically proves the recipients can still decrypt it.
The ceremony is recorded so staleness — too long since the last successful proof — is visible.
