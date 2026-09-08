---
id: ESC
---

# Recovery escrow

Canopy holds the only copy of some of what it keeps: a group's repo passphrase, an environment's secret variables.
It continuously escrows that state so it can be recovered without Canopy, and does so in a way a compromise of Canopy cannot read back.

## Scope

This spec covers the escrow itself: what it is encrypted to, where it is written, and what recovering from it takes.
What each area puts in it is that area's to say, and the areas that do are the backup control plane (see [BKJ](backup.md)) and the environment inventory (see [INV](../private-server/inventory.md)).

## The mechanism

The escrow is encrypted to a set of recipient public keys whose private halves Canopy never holds.
So Canopy can write the escrow and never read it back: a full compromise of Canopy discloses nothing that is only in there.

Recipients are mandatory.
Canopy refuses to run without them, so there is never a silent recovery gap.
A recipient set is identified by its public keys, which is what a reader compares to tell a deliberate rotation from a substitution.

The escrow is written to versioned, object-locked storage, so a past version stays undeletable until its retention expires.
Canopy writes the current state on a cadence to one object rather than accumulating objects; the storage's own versioning is the history.

## What a recovery needs

One recipient's private key decrypts the escrow, so recovery needs no quorum and no other recipient's cooperation.

The escrow carries its own version, so a reader knows the shape it is reading rather than inferring it.
It records when it was taken, since a recovery has to know how stale what it holds is.

A value Canopy could not read when the escrow was taken is written as absent and logged, rather than failing the whole write: a snapshot missing one group's passphrase is worth more than none at all.

## Verification

An escrow nobody has ever decrypted is not a recovery path.
An operator periodically proves the recipients can still decrypt what Canopy writes, and Canopy records when that was last done and against which recipients (see [BKO](../private-server/backup.md)).

## Out of scope

- What any one area escrows, which each area's spec states.
- The operator ceremony's own flow (see [BKO](../private-server/backup.md)).
