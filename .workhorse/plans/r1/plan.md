# Restore replica conflict error with different names

## The defect

Declaring a second restore replica for the same `(consumer, group, type, intent, server)` is
refused with `conflict: a matching restore replica is already declared`, whatever the operator
names it. The intent is the opposite: an operator may declare as many replicas of the same thing
as they like, so long as each has its own name.

Two unique indexes stand on `restore_replicas`:

- `restore_replicas_scope_server` / `restore_replicas_scope_group` on
  `(consumer, group, type, intent, server_id)` — from the original migration
- `restore_replicas_consumer_name` on `(consumer, name)` — added later

The scope pair is what refuses the declaration. The name index is the one that should govern.

The declare dialog makes it the default outcome rather than an edge: it defaults to whole-group
scope, auto-selects the consumer's first intent, and suggests a name built from group, server, and
intent. A second declaration in the same group therefore arrives with both the same suggested name
*and* the same scope. The operator is told the name is taken, renames, resubmits, and is then told
a matching replica already exists — a second error that names nothing, which is why it reads as
firing despite the name having changed.

## Why it isn't just an index drop

`(server, type, intent)` is the replica's identity everywhere downstream, and dropping the scope
index makes it ambiguous:

- the worklist dedupes on it, so the second replica would be silently dropped from dispatch
  (`crates/public-server/src/restore.rs`)
- `ReplicaKey` keys check derivation on it, so two replicas would merge into one check instance
  (`crates/database/src/restore.rs`)
- the three `*_by_key*` report queries and `latest_verdict_by_key` group on it, so the two
  replicas' reports would overwrite each other

So the declaration's name has to become part of the replica's identity, not just a label.
Reports already denormalise `(group, server, type, intent)` so they outlive the declaration they
came from; the name joins them, resolved from `replica_id` when the report is recorded.

## Build steps

- [x] Migration: drop both scope unique indexes; add `replica_name` to `backup_restore_checks` and
      backfill it from `restore_replicas` via `replica_id`
- [x] Record the declaration's name onto each report at report time, resolved from `replica_id`
- [x] Widen `ReplicaKey` to carry the name; split `KeyWork.declared_as` into a `declared` flag plus
      the key's own name, since a report-derived key has a name but no declaration
- [x] Key the three `BackupRestoreCheck::*_by_key*` queries and `latest_verdict_by_key` on the name
      as well
- [x] Dedupe the worklist on `(server, name)` rather than `(server, type, intent)`, so a group-wide
      and a server-scoped declaration with different names both dispatch
- [x] Simplify `unique_violation` now that only the name constraint remains
- [x] Suggest a non-colliding name in the declare dialog, so the first submission doesn't collide
      by construction
- [x] Update RST: the same scope may be declared more than once under different names, and the
      name is part of a replica's identity for dispatch and alerting
- [x] Tests: same-scope coexistence, per-name report and check-instance separation, and pin the
      name-collision message (the existing conflict assertions all match `Conflict(_)`, so a
      mis-attributed message would ship unnoticed)

## Decisions taken

**A group-wide and a server-scoped declaration covering the same server now both dispatch.**
Previously the server-scoped one won a dedup over the group-wide one. With the name as the
discriminator they are two different named replicas, so both are entries. This is a behaviour
change beyond the reported symptom and follows directly from the intent.

**Two consumers declaring the same scope was already possible** (the dropped indexes keyed on
consumer), and their reports already shared a `ReplicaKey`. Carrying the name narrows this but
does not close it, since names are unique per consumer rather than fleet-wide.

**A report must name a declaration that exists and is the caller's own.** `replica_id` was
optional so a report still landed when its declaration was retired mid-restore. With several
replicas per scope, a report naming none cannot be attributed to one of them, and recording it
would hold a finding against a replica nothing declares — which RST already says stops being an
instance precisely because nothing could ever recover it. So the field is required, an unknown
declaration is a 404, and another consumer's is a 403. The consumer that loses its declaration
mid-restore now has its report refused rather than recorded namelessly.

Reports predating this still carry no name, so `ReplicaKey` keeps an optional name and the DB
layer can still record one without. Nothing new joins that class.
