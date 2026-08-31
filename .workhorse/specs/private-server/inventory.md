---
id: INV
---

# Environment inventory

Canopy answers, for one environment, which servers it comprises and what each of them is configured with.
The tooling that configures those servers reads that answer at the moment it runs, so a run acts on the fleet as Canopy holds it rather than on a description of the fleet kept in step by hand.

## Why it exists

Canopy already records every server, the group it belongs to, its rank, its product and kind, the names it answers on, and the tags describing it.
A configuration run needs exactly that, and takes it instead from a file an operator edits alongside the run's own code, which nothing reconciles: a server added, renamed, reclassified, or archived in Canopy stays as it was in the file until someone notices.
That file also holds, in the clear, the values a run must not disclose, and a site's identity alongside them, so both are read by everyone who can read the tooling.
Held in Canopy, each is read by whoever is authorised to read it.

## What an inventory is

An inventory covers one environment: a group's live servers at one rank (see [GRP](../servers/groups.md)).
A server carrying no rank is at the default rank, so every live server belongs to exactly one of its group's environments and none is unreachable for want of one.
An archived server is not in it, and a server belonging to no group is in no inventory.
A group holding more than one environment is refused unless the rank is named, since configuring a site's demo servers alongside its production ones is never what was meant.

Each member carries what Canopy holds for it: its name, its product and kind, the address it is reached at, and its variables.

## Variables

A variable is either plain or secret.
A name is one or the other throughout an inventory, and setting a name that is already the other is refused, since which of the two a run received would otherwise turn on the order the merge happened to take.

A plain variable is a tag: a server's are its own overlaid on its group's, which is the same merge its sources read back as effective tags (see [STA](../public-server/statuses.md)).
The reserved read-only tags are not variables, the classification they carry being already in the member's identity.
Canopy attaches no plain value to an environment, so a value belonging to one environment of a group that holds several is set on each of that environment's servers.

A tag is stored as text.
A value of exactly `true` or `false` is served as a boolean and a JSON array or object as that array or object; every other value is served as the text it was stored as, a bare number included, since a number in this fleet is more often a version or an identifier than a quantity.
A secret variable is served under the same rules, so a run reads the two the same way.

## Secret variables

A run needs values that must not be readable by everyone who can read Canopy: a salt an environment's servers share, an enrolment token, the access key one server answers a certificate challenge with.
A tag cannot hold one, being readable by every operator and, where it is the group's, served to every server in that group as part of its effective tags.

A secret variable is held in Canopy's secret store instead, the one that holds a group's backup passphrase (see [BKO](backup.md)).
It is never served as a tag, never appears in an effective-tags read, and is never logged.

Because that store is Canopy's own rather than the tag map, a secret variable is keyed to the scope its value belongs to: an environment, or one server of one, the server's overlaying the environment's.
A group is not a scope for one, since the values a group's several environments would then share are the ones that most need to differ between them.

An administrator sets, replaces, and removes a secret variable, and Canopy does not serve one back to a reader of the group or the server.
It is served only as part of an inventory, and the inventory marks which of a member's variables are secret so that a caller can keep them out of anything it writes down.
A copy of an inventory kept against Canopy being unreachable therefore carries the plain variables alone, and a run needing a secret one while offline fails for want of it rather than proceeding on a stale value.

A credential Canopy itself issues to a device is provisioned rather than set as a variable (see [DPK](provisioned-credentials.md)).

## Reaching a server

A server's address is the tailnet name of the device bound to it, or its recorded host where no device is bound.
A variable of the server's own overrides that, for one that has to be reached some other way, and which account a run connects as is a variable like any other.

## Refusal

Canopy either serves an inventory or refuses it, and a refusal names why: a group it does not have, a name answering for more than one group, an archived group, a group holding several environments with no rank named, and a rank with no live server to configure.
An inventory whose secret variables cannot be read is refused rather than served without them, so a run never receives a member that looks configured and is missing a value.
A refusal is distinguishable by a reader from a failure to answer at all, the two meaning opposite things: Canopy declining is a decision to respect, while Canopy being unreachable is the absence of one.

## Authorisation

An inventory carries secret values, so it is served to an administrator (see [ADM](admin-access.md)), and each read is logged with the identity that asked for it.
There is no separate credential to hold or distribute: a run reads the inventory as the administrator running it.

What the inventory is assembled from stays as readable as it was: any operator Canopy authenticates reads an environment's servers, groups, and tags, and sees which secret names are set without their values.

## Presentation

A group presents, for each environment it holds, the inventory Canopy serves for it: each server with its address and its effective variables.
An operator reads there what a run would receive, with a value inherited from the group distinguished from one the server sets itself, so a variable that is not taking effect is diagnosed where it is set.
A secret variable appears by name, with the scope it is set at and when it last changed, and never its value.

## Out of scope

- Performing a configuration run, scheduling one, or triggering one.
- The inventory format of any particular configuration-management tool: Canopy serves the environment's shape, and the caller renders it.
- Authoring plain variables anywhere other than the tags an operator already sets.
