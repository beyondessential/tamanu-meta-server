---
id: BAK
---

# Device backups

A server device backs up to object storage that Canopy mediates: it holds no long-lived storage credentials and knows nothing of the bucket layout until Canopy tells it, per run.
Canopy is the control plane — it owns the credentials, the repo location, the passphrase, and the record of what ran.

## Scope

This spec covers the device-facing contract: how a device learns what it may back up, obtains short-lived credentials and the repo coordinates for a run, and reports the outcome.

It does not cover what an operator configures (see [BKO](../private-server/backup.md)), what Canopy does on its own — maintenance, inspection, detection, alerting (see [BKJ](../jobs/backup.md)) — or restoring backups (the managed restore replicas spec, `RST`).

## Identity and resolution

A device authenticates with the `server` role, over either transport Canopy accepts (a client certificate on the internet-facing path, or tailnet identity on the private mount).
Every device request resolves through the authenticated identity, never the request body: device → its single live server → that server's group → the group's backup configuration.
A device bound to no live server is refused; a server with no group, or whose group has no ready configuration, is refused.

## Capabilities

A device registers the backup types it can run on its server.
A newly seen type is enabled for scheduling or not according to that type's fleet default; a type already known keeps the operator's setting.
Registration requires the server to be grouped, but not the group's configuration to be ready.

## Credentials

A device requests credentials for a `(type, purpose)`.
Canopy issues short-lived credentials by assuming the group's dedicated cross-account storage role under a session policy that confines them to the group's bucket and prefix:

- **backup** purpose grants the write set kopia needs, including a version-less delete — but never deletion of a locked version, nor any weakening of object-lock or retention.
- **restore** purpose grants read-only access.

The credentials carry the storage role's identity for at most an hour; a device refreshes them as a run outruns that lifetime.
Every issuance is recorded before the credentials are returned.

A `(type, purpose)` is issuable only when the type is an enabled capability of the server, or an operator has queued a one-off request of that purpose for it; otherwise it is refused.
The group's configuration must be ready: until then the endpoints refuse, so a half-provisioned group cannot be written to.

## Target

A device fetches the repo coordinates for its group each run: the storage kind, bucket, prefix, region, and the repo passphrase.
The passphrase is Canopy-owned and read from the group's secret store at request time; the device never stores it.

## Reporting

A device reports each run's outcome: the type and purpose, success or failure, an error when it failed, the resulting snapshot identifier, the bytes uploaded, and the object-storage traffic the run moved.
The run is keyed by an identifier the device mints at the start of the run; the device, server, and group are taken from the authenticated context, so a device cannot report a run as another group's.
A duplicate run identifier is refused.
Reporting a run clears any matching operator one-off request, so the standing "back up now" prompt stops.

## Guarantees

A compromised device cannot destroy backups.
Its credentials cannot delete a locked object version or weaken the bucket's object-lock; at worst it writes a delete-marker that object-lock and versioning leave recoverable.
Decommissioning a device is revoking its certificate: it can no longer obtain credentials, and any it already holds expire within the hour.

## Failure contract

The device endpoints distinguish: the caller is bound to no live server; the server is ungrouped, has no ready configuration, the type is not issuable, or a run identifier is duplicate; and Canopy's own dependency — the credential issuer or the secret store — is unavailable or unconfigured.
Each is a distinct, stable status so a device need not guess.
