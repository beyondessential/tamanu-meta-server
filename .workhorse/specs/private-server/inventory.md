---
id: INV
---

# Environment inventory

Canopy answers, for one environment, which applications it comprises and what each of them is configured with.
The tooling that configures those applications reads that answer as it runs, so a run acts on the fleet as Canopy holds it rather than on a description kept in step by hand.

## Why it exists

Canopy already records every application, the machine it runs on, its group, its rank, its type, the names it answers on, and the tags describing it.
A configuration run needs exactly that, and takes it instead from a file nothing reconciles: an application added, renamed, reclassified, or archived in Canopy stays as it was in the file until someone notices.
That file also holds, in the clear, the values a run must not disclose, so everyone who can read the tooling reads them.

## What an inventory is

An inventory covers one environment: a group's live applications at one rank (see [FLT](../servers/overview.md), "Environments").
Rank is an application's rather than its machine's, so an environment is a set of applications, and a box carrying a production workload beside a demo one sits in two of them.
An application carrying no rank is at the default rank, so every live application belongs to exactly one of its group's environments.
An archived application is not in it, and an application belonging to no group is in no inventory.
A machine that has been created and has not yet reported carries no application, so it configures nothing and reads as a rank with nothing to configure.
A group holding more than one environment is refused unless the rank is named, since configuring a site's demo applications alongside its production ones is never what was meant.

Each member carries its name, its type, the machine it runs on, the address it is reached at, and its variables.

## Variables

A variable is either plain or secret, and a name is one or the other: setting a name that is already the other is refused.

A plain variable is a tag, an application's own overlaid on its group's, which is the merge its sources read back as effective tags (see [STA](../public-server/statuses.md)).
The reserved read-only tags are not variables.
A machine's tags are not variables either: they describe the box, and what a run configures is the workload on it.
Canopy holds no plain value against an environment, so one belonging to a single environment of a group that holds several is set on each of that environment's applications.

A tag is stored as text.
A value of exactly `true` or `false` is served as a boolean and a JSON array or object as that array or object; every other value is served as the text it was stored as, a bare number included, since a number in this fleet is more often a version or an identifier than a quantity.
A secret variable is served under the same rules.

## Secret variables

A run needs values that must not be readable by everyone who can read Canopy: a salt an environment's applications share, an enrolment token, an access key.
A tag cannot hold one, being readable by every operator and, where it is the group's, served to every application in that group as part of its effective tags.

A secret variable is held in Canopy's secret store instead, the one holding a group's backup passphrase (see [BKO](backup.md)).
It is never served as a tag, never appears in an effective-tags read, and is never logged.
It is keyed to the scope its value belongs to, an environment or one application in one, the application's overlaying the environment's.
A group is not a scope for one: the values its several environments would share are the ones that most need to differ between them.
Nor is a machine: a box is not a thing a run configures, and two workloads sharing one need not share a value.

An administrator sets, replaces, and removes a secret variable, and Canopy does not serve one back to a reader of the group or the application.
It is served only as part of an inventory, which marks which of a member's variables are secret so a caller can keep them out of anything it writes down.
A copy kept against Canopy being unreachable therefore carries the plain variables alone, and a run needing a secret one offline fails for want of it rather than proceeding on a stale value.

A credential Canopy issues to a device is provisioned rather than set as a variable (see [DPK](provisioned-credentials.md)).

## Reaching an application

An application's address is the tailnet name of the device bound to the machine it runs on, or its own recorded host where no device is bound.
An identity speaks for the box rather than for a workload, so two applications on one machine are reached at the same address.
A variable of the application's own overrides that, and which account a run connects as is a variable like any other.

## Refusal

Canopy either serves an inventory or refuses it, and a refusal names why: a group it does not have, a name answering for more than one group, an archived group, a group holding several environments with no rank named, a rank with no live application to configure, and a secret variable whose value cannot be read.
Serving the rest of that last one would hand a run a member that looks configured and is missing a value.
A refusal is distinguishable from a failure to answer at all, the two meaning opposite things: Canopy declining is a decision to respect, while Canopy being unreachable is the absence of one.

## Authorisation

An inventory carries secret values, so it is served to an administrator (see [ADM](admin-access.md)), and each read is logged with the identity that asked for it.
A run reads the inventory as the administrator running it, with no separate credential to distribute.

What it is assembled from stays as readable as it was: any operator Canopy authenticates reads an environment's applications, machines, groups, and tags, and sees which secret names are set without their values.

## Presentation

A group presents, for each environment it holds, the inventory Canopy serves for it: each application with its address and its effective variables.
An operator reads there what a run would receive, with a value inherited from the environment distinguished from one the application sets itself, so a variable that is not taking effect is diagnosed where it is set.
A secret variable appears by name, with the scope it is set at and when it last changed, and never its value.

## Out of scope

- Performing a configuration run, scheduling one, or triggering one.
- The inventory format of any particular configuration-management tool: Canopy serves the environment's shape, and the caller renders it.
- Authoring plain variables anywhere other than the tags an operator already sets.
