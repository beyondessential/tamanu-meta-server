# Restore replica conflict error with different names

Scenarios verifying that a scope may hold several replicas, told apart by name, and that the
name is what a collision is reported against.

## Declaring

- [x] A second declaration of one `(consumer, group, type, intent, server)` scope under a
      different name is accepted (verifies spec: RST)
- [x] A server-scoped declaration coexists with a group-wide one covering the same server
      (verifies spec: RST)
- [x] Reusing a name already assigned to the consumer is refused, and the error names the name
      rather than the scope (verifies spec: RST)
- [x] Surrounding whitespace does not buy a way around the name uniqueness
- [x] Another consumer may reuse a name (verifies spec: RST)
- [x] Editing a declaration onto another declaration's scope is accepted; editing it onto that
      declaration's name is refused (verifies spec: RST)

## Declare dialog

- [x] Declaring into a group that already has a replica of the same scope suggests a name that
      counts past the one in use, so the first submission does not collide
- [ ] The suggested name still counts past a name the consumer holds in a *different* group
      (names are unique per consumer fleet-wide, and the dialog only sees this group's list, so
      this case surfaces as the backend's 409)

## Dispatch

- [x] Two named replicas of one server both appear on the worklist, neither standing in for the
      other (verifies spec: RST)
- [x] A `once` intent's suppression is per named replica: one settling its snapshot does not
      take a sibling off the worklist (verifies spec: RST)

## Alerting

- [x] Two replicas of one `(type, intent)` on one server grade as two instances, and one failing
      leaves the other's result untouched (verifies spec: RST)
- [x] The failing replica is named in the check message; its healthy sibling is not
      (verifies spec: RST)
- [ ] A silence written against one replica's name leaves its same-scope sibling alerting
      (verifies spec: RST)

## Reports

- [x] A report carries the declaration's name, resolved from `replica_id` at record time
      (verifies spec: RST)
- [x] A report that names no declaration stands as its own replica rather than attaching to a
      named one (verifies spec: RST)
- [ ] A report recorded before the declaration is deleted goes on naming its replica once
      `replica_id` is nulled (verifies spec: RST)

## Migration

- [x] The migration backfills `replica_name` onto existing reports from their declaration
- [ ] The down migration leaves the scope indexes off where a scope has since gained a second
      declaration, rather than failing or dropping one
