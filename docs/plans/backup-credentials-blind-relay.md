# Backup credentials: blind-relay issuer isolation (STUB — stage 2)

**Status: deferred.** This is a stage-2 hardening of the backup-credentials
design in [`backup-credentials.md`](./backup-credentials.md). Stage 1
knowingly accepts the risk this would close (see that plan's
threat-boundary section: the internet-facing `public-server` holds the
privileged issuer rights). This file is a placeholder to revisit, not a
worked plan.

## Problem it closes

In stage 1, `public-server` — the internet-exposed surface — holds:
- `sts:AssumeRole` on **every** group's per-bucket role → fleet-wide S3
  **read + write** (write = the poisoning vector, see H1 in the main plan), and
- Secret-read for **every** group's kopia repo password → which decrypts
  all backups (so even read access to it is fleet-wide data exposure).

So a single `public-server` RCE yields fleet-wide backup read + poison in
one hop. Stage 1 accepts this (same trust as canopy's other internet
surface); this plan removes it.

## Goal

Make `public-server` a **blind relay**: it authenticates the device (mTLS)
and shuttles bytes, but holds **no** AWS rights and **never sees** creds or
the repo password in plaintext. A `public-server` compromise should yield
nothing usable.

## Sketch

- The privileged capability (STS assume + Secret-read) lives only in an
  **internal, non-internet-facing** component (the jobs/scheduler side),
  driven by Canopy's own backup schedule — minting follows the schedule
  Canopy already owns, not arbitrary device requests.
- That component mints the short-lived creds + fetches the password,
  **envelope-encrypts the bundle to the requesting device's public key**
  (we already have per-device mTLS identities / `device_keys`), and stages
  the opaque blob.
- `public-server` relays the blob to bestool; bestool decrypts with its
  private key. `public-server` can't read it.

Net: the front door holds no secrets and no mint rights; compromise gets
opaque blobs it can't open.

## Hard problems to solve on revisit

1. **Long-backup refresh.** Stage 1's elegance is `credential_process`
   pulling fresh creds on demand (chained STS caps at 1h). Staged delivery
   needs the internal component to keep re-minting + re-staging for the
   life of an active backup (it knows the run started from
   `/backup-report`). Define that orchestration.
2. **Device-initiated restore.** Restore is spontaneous (pre-prod restoring
   from prod), not scheduled, so it doesn't fit pure pre-staging. Note
   restore creds are **read-only** (lower stakes); the password is the part
   that still needs the blind-relay treatment. Decide the restore path.
3. **Creds + password at rest.** Staging puts secrets at rest between mint
   and pickup — short TTL, delete-on-pickup, and the envelope encryption
   (above) is what keeps them opaque to `public-server` and the stage store.
4. **Envelope encryption mechanics.** Which device key (the mTLS cert key,
   or a dedicated backup key?), key availability to bestool, crypto choice,
   rotation, and what happens on device re-provisioning.
5. **Request path without re-trusting `public-server`.** If any request
   flows device → `public-server` → internal minter, the minter must not
   blindly trust `public-server`'s assertion of device identity (else
   compromise re-grants minting). Either drive everything from Canopy's
   schedule (no device-asserted mint), or forward the raw device cert for
   independent validation.

## Relationship to the main plan

Replaces only the *delivery* mechanism: the on-demand `/backup-credentials`
minting and the `/backup-target` password-serving become staged,
envelope-encrypted, blind-relayed delivery. Everything else in
`backup-credentials.md` (per-bucket cross-account roles, the group model,
GOVERNANCE object lock, Canopy-owned maintenance/retention/inspection,
detection, preflight) is unchanged.

See the M7 discussion that produced this split.
