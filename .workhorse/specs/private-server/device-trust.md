---
id: DTR
---

# Device trust model

Every device Canopy records holds a role that grants access: administrator, server, releaser, backup-restore, or relay.
There is no untrusted or pending state.
A device exists only because a deliberate act created it, and that act records the device at its role from the outset.

A relay is the role held by a relay Canopy runs in a Kubernetes cluster to read that cluster on its behalf (see [K8S](../monitoring/kubernetes.md)); it is associated with no server, and it reports the checks of the servers in its cluster rather than of a server of its own.

## How a device comes to exist

A device is recorded through one of:

- Server enrolment — a box proves possession of its key against a gated enrolment token and is recorded as a server.
- Tailnet attachment — an operator binds a known tailnet identity to a device, which is trusted as a server and thereafter authenticates by that identity.
- Provisioning — an operator has Canopy mint a credential at a chosen role (see [DPK](provisioned-credentials.md)).

A device is never created merely because a client connected.
An mTLS client presenting an unrecognised key, and a tailnet node with no device record, do not authenticate and cause no record to be created.

## Authentication

A device authenticates by one of two means, according to the path it arrives on:

- mTLS: the public key derived from the presented certificate matches an active key on the device.
- Tailnet identity: the calling node's identity matches the device's recorded tailnet identity.

A request is authorised only when it resolves to an existing device whose role permits the endpoint.
A tailnet identity may be held by at most one device; attaching one already held elsewhere is refused.

## Revocation

An operator revokes a device's access by deactivating its keys and detaching its tailnet identity.
The device record and its role are retained as history; a revoked device simply can no longer authenticate.
Archiving a server revokes its bound device the same way, so the box can only return through enrolment.
