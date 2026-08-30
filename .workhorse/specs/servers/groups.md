---
id: GRP
---

# Server groups

A group is what Canopy holds shared state against: one backup repository with its passphrase and retention, one incident target with its channel and grace period, one open upgrade plan, the domain names it claims, and one billing identity.
Servers are grouped so that what is true of several of them at once is watched, alerted, and paid for in one place.

## What a group is

A group is a flat set of servers, and what makes it one group is the state Canopy attaches to it rather than anything its members have in common.
A server belongs to at most one group, and a server in none of them carries none of that state: its issues reach no incident target (see [INC](../monitoring/incidents.md)) and it has no billing attribution (see [APP](products.md)).
Membership constrains neither product nor rank, so a group holds whatever an operator puts in it.

The fleet is organised one group per customer site, holding that site's servers whatever application each runs and whatever environment each serves.
That is how operators use groups rather than a rule Canopy enforces, and a group may equally hold servers belonging to no customer, such as an internal demo.

## Environments

A server's rank is its environment tier: production, clone, demo, test, or dev.
An operator sets it, and a server may carry none.

A group's servers at one rank are one of its environments, so a site's production central server and the facility servers syncing to it are that site's production environment.
Canopy holds no state against an environment: it presents a group's servers under their rank, and everything it attaches belongs to the group.

## A group's headline rank

A group holds no rank of its own.
Its headline rank is the highest rank held by any of its servers, production outranking clone, then demo, then test, then dev.
The fleet listing buckets each group under its headline rank, and a group whose servers are all unranked is left out of that listing.
A group's billing stage is the same value (see [APP](products.md)).

The headline rank is distinct from the canonical member that gives a group its headline version, which is the highest-ranked *live* member with kind breaking a tie (see [APP](products.md)).

## Naming

Canopy calls a group a group wherever it appears: in the operator interface, in its API, and throughout this spec set.
An environment names a group's servers at one rank, which is the unit some infrastructure outside Canopy calls a deployment.
The Canopy instance names an installation of Canopy itself, so a zone list, an ingress, or an account key belongs to the Canopy instance rather than to any group.

The `billing.deployment` cost-allocation label carries the group's name and keeps that spelling, because it is read outside Canopy: by cloud cost allocation, and by every server device that reads its effective tags (see [APP](products.md), [STA](../public-server/statuses.md)).
