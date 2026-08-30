---
id: INV
---

# Deployment inventory

Canopy answers, for one deployment, which hosts it comprises and what each of them is configured with.
The tooling that configures those hosts reads that answer from Canopy at the moment it runs, so what a run acts on is the fleet as Canopy holds it rather than a description of the fleet kept in step by hand.

## Why it exists

Canopy already records every server, the group it belongs to, its product and kind, the names it answers on, and the operator-set tags describing it.
A configuration run needs exactly that, and takes it instead from a file an operator edits alongside the run's own code.
Nothing reconciles the two, so a server added, renamed, reclassified, or archived in Canopy stays as it was in the file until someone notices, and a run configures the fleet of whenever the file was last correct.

The file is also where a deployment's identity lives: the hostnames it answers on, the facilities it serves, the account each host is reached as.
Held in Canopy, that identity is read by the people and the runs authorised to read it, rather than by everyone who can read the tooling.

## What an inventory is

An inventory is scoped to one server group, the group being the deployment.
It comprises the group's live members: an archived server is not in it, and a server belonging to no group is in no inventory.

Each member carries the identity Canopy holds for it and the variables that configure it.
The identity is the server's name, its product and kind, its rank, and the address it is reached at (see [APP](../servers/products.md)).
Membership carries no product constraint, so a deployment running more than one application is one inventory covering all of it.

The inventory carries the group as a whole as well as its members, since a deployment has values that belong to it rather than to any one host.

## Variables

A member's variables are its own tags overlaid on its group's, which is the same merge its sources read back as its effective tags (see [STA](../public-server/statuses.md)).
A value common to a deployment is therefore set once on the group, and a host that differs sets its own.
The reserved read-only tags are not variables: the classification they carry is already in the member's identity, and serving it twice invites the two copies to disagree.

A tag is stored as text, and most variables are text.
A variable whose stored value is exactly `true` or `false` is served as a boolean, and one whose value is a JSON array or object is served as that array or object.
Every other value is served as the text it was stored as, a bare number included, since a number in this fleet is far more often an identifier or a version than a quantity, and reading one as arithmetic where it was meant as a label is the more damaging mistake.

## Reaching a host

A member's address is the tailnet name of the device bound to it, or its recorded host where it has no bound device.
A variable of the member's own overrides that, for the host that has to be reached some other way.
Which account a run connects as, and anything else the connection needs, is a variable like any other.

## Secrets

An inventory carries no secret.

A tag is readable by every operator, and a tag on a group is served to every server in that group as part of its effective tags, so a value that must not be read by all of those must not be a tag.
This holds whatever the value is worth to whoever set it: the inventory is a description of the fleet, and a description is not a place to keep the keys to it.
What a run needs and may not read from here is obtained the way any other secret Canopy is involved in is obtained, against the identity of whoever is asking (see [DPK](provisioned-credentials.md)).

## Refusal

Canopy either serves a group's inventory or refuses it, and a refusal names why.
A group Canopy does not have, one that has been archived, and one with no live member to configure are each refused, and each says which it was.

A refusal is distinguishable by a reader from a failure to answer at all.
The two mean opposite things: Canopy declining is a decision to be respected, while Canopy being unreachable is an absence of one, and a caller that conflates them either ignores a refusal or invents one.

## Authorisation

Reading an inventory is administrative, and is authorised as every other administrative read is, from the caller's authenticated identity (see [ADM](admin-access.md)).
There is no separate credential to hold or distribute: an operator who may read a deployment in Canopy may read its inventory.

Canopy records each inventory read with the caller, the group, and the time.
A run configures hosts from what it read, so what it read, and who asked for it, is part of that deployment's history.

## Presentation

A group presents the inventory Canopy serves for it, showing each member with its address and its effective variables, and the group's own.
An operator reads there what a run would receive, and sees a value inherited from the group distinguished from one the member sets itself, so a variable that is not taking effect is diagnosed where it is set rather than by running something to find out.

## Out of scope

- Performing a configuration run, scheduling one, or triggering one.
- The inventory format of any particular configuration-management tool: Canopy serves the deployment's shape, and the caller renders it into whatever its own tooling reads.
- Holding the credentials or other secrets a run needs.
- Any way of authoring a member's variables other than the tags an operator already sets on servers and groups.
