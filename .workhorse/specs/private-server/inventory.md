---
id: INV
---

# Environment inventory

Canopy answers, for one environment, which servers it comprises and what each of them is configured with.
The tooling that configures those servers reads that answer at the moment it runs, so a run acts on the fleet as Canopy holds it rather than on a description of the fleet kept in step by hand.

## Why it exists

Canopy already records every server, the group it belongs to, its rank, its product and kind, the names it answers on, and the tags describing it.
A configuration run needs exactly that, and takes it instead from a file an operator edits alongside the run's own code, which nothing reconciles: a server added, renamed, reclassified, or archived in Canopy stays as it was in the file until someone notices.
The file is also where a site's identity lives, so held in Canopy that identity is read by whoever is authorised to read it rather than by everyone who can read the tooling.

## What an inventory is

An inventory covers one environment: a group's live servers at one rank (see [GRP](../servers/groups.md)).
A server carrying no rank is at the default rank, so every live server belongs to exactly one of its group's environments and none is unreachable for want of one.
An archived server is not in it, and a server belonging to no group is in no inventory.
A group holding more than one environment is refused unless the rank is named, since configuring a site's demo servers alongside its production ones is never what was meant.

Each member carries what Canopy holds for it: its name, its product and kind, the address it is reached at, and its variables.

## Variables

A server's variables are its own tags overlaid on its group's, which is the same merge its sources read back as effective tags (see [STA](../public-server/statuses.md)).
The reserved read-only tags are not variables, the classification they carry being already in the member's identity.
Canopy attaches no state to an environment, so a value belonging to one environment of a group that holds several is set on each of that environment's servers.

A tag is stored as text.
A value of exactly `true` or `false` is served as a boolean and a JSON array or object as that array or object; every other value is served as the text it was stored as, a bare number included, since a number in this fleet is more often a version or an identifier than a quantity.

## Reaching a server

A server's address is the tailnet name of the device bound to it, or its recorded host where no device is bound.
A variable of the server's own overrides that, for one that has to be reached some other way, and which account a run connects as is a variable like any other.

## Secrets

An inventory carries no secret.
A tag is readable by every operator, and a group's tags are served to every server in that group as part of its effective tags, so a value that must not be read by all of those must not be a tag.
What a run needs and may not read from here is obtained against the identity of whoever is asking, as any other secret Canopy is involved in is (see [DPK](provisioned-credentials.md)).

## Refusal

Canopy either serves an inventory or refuses it, and a refusal names why: a group it does not have, a name answering for more than one group, an archived group, a group holding several environments with no rank named, and a rank with no live server to configure.
A refusal is distinguishable by a reader from a failure to answer at all, the two meaning opposite things: Canopy declining is a decision to respect, while Canopy being unreachable is the absence of one.

## Authorisation

An inventory is read by any operator Canopy authenticates, on the same footing as the servers, groups, and tags it is assembled from (see [ADM](admin-access.md)).
There is no separate credential to hold or distribute: a run reads the inventory as the operator running it.

## Presentation

A group presents, for each environment it holds, the inventory Canopy serves for it: each server with its address and its effective variables.
An operator reads there what a run would receive, with a value inherited from the group distinguished from one the server sets itself, so a variable that is not taking effect is diagnosed where it is set.

## Out of scope

- Performing a configuration run, scheduling one, or triggering one.
- The inventory format of any particular configuration-management tool: Canopy serves the environment's shape, and the caller renders it.
- Holding the credentials or other secrets a run needs, or authoring variables anywhere other than the tags an operator already sets.
