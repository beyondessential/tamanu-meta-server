---
id: INV
---

# Environment inventory

Canopy answers, for one environment, which servers it comprises and what each of them is configured with.
The tooling that configures those servers reads that answer as it runs, so a run acts on the fleet as Canopy holds it rather than on a description kept in step by hand.

## Why it exists

Canopy already records every server, its group, its rank, its product and kind, the names it answers on, and the tags describing it.
A configuration run needs exactly that, and takes it instead from a file nothing reconciles: a server added, renamed, reclassified, or archived in Canopy stays as it was in the file until someone notices.
That file also holds, in the clear, the values a run must not disclose, so everyone who can read the tooling reads them.

## What an inventory is

An inventory covers one environment: a group's live servers at one rank (see [GRP](../servers/groups.md)).
A server carrying no rank is at the default rank, so every live server belongs to exactly one of its group's environments.
An archived server is not in it, and a server belonging to no group is in no inventory.
A group holding more than one environment is refused unless the rank is named, since configuring a site's demo servers alongside its production ones is never what was meant.

Each member carries its name, its product and kind, the address it is reached at, and its variables.

## Variables

A variable is either plain or secret, and a name is one or the other: setting a name that is already the other is refused.

A plain variable is a tag, a server's own overlaid on its group's, which is the merge its sources read back as effective tags (see [STA](../public-server/statuses.md)).
The reserved read-only tags are not variables.
Canopy holds no plain value against an environment, so one belonging to a single environment of a group that holds several is set on each of that environment's servers.

A tag is stored as text.
A value of exactly `true` or `false` is served as a boolean and a JSON array or object as that array or object; every other value is served as the text it was stored as, a bare number included, since a number in this fleet is more often a version or an identifier than a quantity.
A secret variable is served under the same rules.

## Secret variables

A run needs values that must not be readable by everyone who can read Canopy: a salt an environment's servers share, an enrolment token, an access key.
A tag cannot hold one, being readable by every operator and, where it is the group's, served to every server in that group as part of its effective tags.

A secret variable is held in Canopy's secret store instead, the one holding a group's backup passphrase (see [BKO](backup.md)).
It is never served as a tag, never appears in an effective-tags read, and is never logged.
It is keyed to the scope its value belongs to, an environment or one server of one, the server's overlaying the environment's.
A group is not a scope for one: the values its several environments would share are the ones that most need to differ between them.

An administrator sets, replaces, and removes a secret variable, and Canopy does not serve one back to a reader of the group or the server.
It is served only as part of an inventory, which marks which of a member's variables are secret so a caller can keep them out of anything it writes down.
A copy kept against Canopy being unreachable therefore carries the plain variables alone, and a run needing a secret one offline fails for want of it rather than proceeding on a stale value.

A credential Canopy issues to a device is provisioned rather than set as a variable (see [DPK](provisioned-credentials.md)).

## Reaching a server

A server's address is the tailnet name of the device bound to it, or its recorded host where no device is bound.
A variable of the server's own overrides that, and which account a run connects as is a variable like any other.

## Work under way

A configuration run begins by reading the inventory, so the read is where two runs on one environment are kept apart.
Canopy refuses an inventory while a maintenance window declared by someone other than the reader holds over the environment: over its group, or over any server the environment comprises (see [MNT](../monitoring/maintenance.md)).
The refusal names who declared the window, when it is expected to end, and its note, so the reader knows who to talk to and what they are in the middle of.

A window over one member refuses the whole environment, since a run acts on the environment as a whole and serving the rest would configure around a server someone is part-way through changing.
A window over a server at another rank refuses nothing, that server being in no environment the inventory covers.

A window is the declaration that a deployment is being worked on, and declaring one is one action at the moment work starts, so an operator about to run declares theirs first and is served the inventory their own window covers.
A target holds at most one open window, so a second operator's declaration amends the first's rather than opening one of their own, and the inventory stays refused to them.
Lifting a window someone else declared is the deliberate step that takes the work over, audited as every lift is, so a run never proceeds over another operator's work by accident.

The refusal lasts exactly as long as the window holds.
Settling is about how Canopy grades what it observes once work is done and holds no one's work, so the inventory is served again the moment the window ends.

A change someone else has just made to the environment's settings is work under way too, for a short while after it is made.
Canopy observes such a change where it records who made it, which is a secret variable set at the environment or on one of its members; a plain variable and a server's membership record when they changed and not by whom, so they hold nothing.
The inventory is refused while a secret variable set by someone other than the reader is newer than a recency period, the same for every environment, and the refusal names the variable, who set it, and when.
The reader's own changes hold nothing, setting a value and then running on it being the usual order of things.

## Planned upgrades

A run says what it intends: to configure the environment as it stands, or to upgrade it.
A run naming no intent is configuring.

An upgrade of a production environment is refused unless its group has an open upgrade plan (see [UPG](upgrade-plans.md)), and the refusal says the deployment has no plan and that recording one is what permits the run.
The plan is the permission and its day is not: which night a deployment moves is often settled after the plan is recorded, and a run held to a typed date would block a person rather than a collision.
A configuration run needs no plan, a plan recording a version move and nothing else.
An environment at any other rank is served either way, a plan being for where a deployment's real users are.

## Refusal

Canopy either serves an inventory or refuses it, and a refusal names why: a group it does not have, a name answering for more than one group, an archived group, a group holding several environments with no rank named, a rank with no live server to configure, an environment someone else has work under way on, an upgrade of a production environment with no plan recorded, and a secret variable whose value cannot be read.
Serving the rest of that last one would hand a run a member that looks configured and is missing a value.
A refusal is distinguishable from a failure to answer at all, the two meaning opposite things: Canopy declining is a decision to respect, while Canopy being unreachable is the absence of one.

## Authorisation

An inventory carries secret values, so it is served to an administrator (see [ADM](admin-access.md)), and each read is logged with the identity that asked for it and the intent it declared.
A run reads the inventory as the administrator running it, with no separate credential to distribute.

What it is assembled from stays as readable as it was: any operator Canopy authenticates reads an environment's servers, groups, and tags, and sees which secret names are set without their values.

## Presentation

A group presents, for each environment it holds, the inventory Canopy serves for it: each server with its address and its effective variables.
An operator reads there what a run would receive, with a value inherited from the environment distinguished from one the server sets itself, so a variable that is not taking effect is diagnosed where it is set.
A secret variable appears by name, with the scope it is set at and when it last changed, and never its value.
While a window someone else declared holds over the environment, the presentation says a run would be refused and names the window, so an operator reads why a run is being held without starting one.

## Out of scope

- Performing a configuration run, scheduling one, or triggering one.
- The inventory format of any particular configuration-management tool: Canopy serves the environment's shape, and the caller renders it.
- Authoring plain variables anywhere other than the tags an operator already sets.
- Checking that an upgrade run moves the deployment to the version its plan names: Canopy learns what a deployment runs from the deployment, after the fact (see [UPG](upgrade-plans.md), "When a plan is met").
