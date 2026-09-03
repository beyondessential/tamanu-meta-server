---
id: DPK
---

# Operator-provisioned device credentials

An operator provisions a trusted device and its authentication credential directly through Canopy, without the device performing a self-enrolment handshake.
Canopy generates the keypair, records the public key against a new or existing device at an operator-chosen role, and returns the private key exactly once, encrypted under a freshly generated passphrase, for the operator to download and carry to the target host.

## Why it exists

Some trusted roles are not tied to a machine enrolment, and some are operated by hand rather than by an agent on a managed machine.
Obtaining a credential for one of those roles otherwise means generating a keypair off-platform and registering its public key by hand.
Provisioning the credential centrally removes that manual step: Canopy already holds and trusts the public key by the time the operator has the private key.

## Scope

This spec covers the operator-facing provisioning surface: creating or selecting a device, minting its credential, the one-time delivery of the private key, and the trust and lifecycle rules for provisioned keys.
It does not cover the machine enrolment handshake, which is the path for agent-managed machines.

## Provisioning

Provisioning is available to operators only.

The operator either creates a new device at a chosen role, or provisions an additional credential onto an existing device.
Any trustable role may be provisioned: administrator, machine, releaser, backup-restore, and relay (see [DTR](device-trust.md)).

On provisioning, Canopy generates a fresh device keypair and records its public key as an active key on the device.
The key material and its representation match what device authentication accepts over mutual TLS: an elliptic-curve P-256 keypair, whose public half is stored as the subject public key info that authentication matches against.
The device is set to the chosen role at the same time, and the credential is immediately valid for authentication — no further handshake or approval step is required before the host can use it.

Provisioning a credential onto an existing device adds an active key alongside any it already has; it does not revoke the others.
The operator manages the lifecycle of individual keys (naming, deactivation) through the existing per-key controls.

## Delivery of the private key

The private key is returned exactly once, at provisioning time.

It is delivered in PKCS#8 PEM form, wrapped in a passphrase-encrypted file in the standard age format (scrypt recipient).
The wrapping passphrase is freshly generated per provisioning and returned alongside the encrypted file; it is a short human-transcribable word sequence, to be shared out of band on a separate channel from the file.
Because the file is standard age, the host decrypts it with the operator's ordinary secret-reveal tooling to recover the PEM.

The response also carries the device identifier and a fingerprint of the public key, so the operator can correlate the credential with the device record in the management UI.

## Non-retention of the private key

Canopy never retains the private key.
It exists only in memory for the duration of the provisioning request and in the encrypted file returned to the operator; it is never persisted to storage and never written to logs.

If the encrypted file or its passphrase is lost, the credential cannot be recovered.
Recovery is by provisioning a new key and deactivating or removing the old one; the lost private key's public half remains inert in the device record until the operator removes it.
