---
id: FLT
---

# Machines, applications, and identities

Canopy's fleet is made of three things.
A **machine** is a host: a box, physical or virtual, that Canopy monitors.
An **application** is a piece of software running somewhere, and is what an operator reasons about when they think about a deployment.
An **identity** is a set of keys, with an optional tailnet identity, that authenticates something to Canopy.

Machine-level facts belong to the machine and application-level facts to the application, so a host running two workloads reports its platform, memory and filesystems once rather than once per workload.

## Cardinality

An application runs on at most one machine.
A machine hosts any number of applications, including none.

An application without a machine is a first-class case rather than a degenerate one: on Kubernetes an application is scheduled across a cluster and has no host in the sense a machine means.
A Kubernetes cluster is its own entity beside machines, not a machine of a special class.

A machine has at most one identity, and an identity belongs to at most one machine.
Identities that authenticate something other than a machine — an operator's credential, a relay — belong to no machine at all.

## Machines come from operators

An operator creates a machine and places it in a group.
Canopy issues an enrolment ticket, the agent on the box presents it, and the machine is enrolled.

The group is the only thing an operator supplies, because it is the only thing that exists nowhere else.
Which deployment a box belongs to is an organisational fact the machine has no way of knowing; what is installed on the box is not.

A machine that has been created but has not yet reported has no applications, and presents as one that has not checked in rather than as an error.

## Applications come from reports

A report is the only thing that creates an application.
An operator never enters an application, its type, or its version, and there is no flow that asks them to.

Canopy adopts what it is told without ceremony: an application it has not seen before on a machine that reports it is created and monitored from that moment.

A report never removes an application.
An application that stops appearing in its machine's reports becomes unreachable and stays, however long it stays away, and only an operator archives it (see [CHK](../monitoring/checks.md), "Reachability").

That asymmetry is deliberate.
Because reports create applications, a reporter that malfunctions and reports nothing would otherwise delete the applications it was responsible for, and monitoring would disappear at the moment it was most needed.
The worst a malfunctioning report can do is make an application read as unreachable, which is loud, rather than absent, which is quiet.

## Naming

A machine is named by the operator who creates it.

An application's name is optional and an operator's alone to set.
An application with no name given presents as the sentence case of its type, so an application of type `tamanu-central` reads as "Tamanu central".

A group containing several applications of one type shows that name repeated, told apart by the machine each runs on and the rank it sits under.

## Archival

A machine and an application are each archived rather than deleted, and each is archived on its own.
Archiving a machine archives the applications on it, a box going away taking its workloads with it.

An archived machine or application leaves the live fleet, and its record and history remain.

## Groups

An application belongs to exactly one group, and so does a machine.

An operator sets the group on the machine, and the applications on that machine take it.
An application without a machine takes its group from whatever registers it.

An application's group is never set independently of its machine's, so the two cannot disagree.
Moving a machine to another group moves the applications on it, and there is no separate move for an application that runs on a machine.

## What each carries

A **machine** carries its identity, its group, where it is (whether it is cloud-hosted and its geolocation), and how long it may be silent before it is considered unreachable.

An **application** carries its type, its rank, its optional name, its public name, the URL it serves at and the name-management permissions and pause state that work from that URL, its notes and tags, and how long it may be silent before it is considered unreachable.

Both carry tags and effective billing labels, so a check filed against either can be graded by policy rules against the tags of its own target.
An application's type is among the reserved read-only tags on applications, and appears on no machine, not being a property of a box.

A machine's own name as the operating system reports it is a reported figure rather than a field an operator sets (see [FIG](../private-server/figures.md)).
It is distinct from an application's URL: two applications on one machine share one hostname and have a URL each.

## Identities

An identity carries a role naming what it authenticates: a machine, an administrator, a releaser, a backup-restore agent, or a relay.

The machine an identity belongs to is resolved only where it is needed.
A request authenticated as a machine resolves the machine it belongs to; a request authenticated as anything else resolves no machine, so the association is invisible to everything with no business with it.

Because an identity belongs to at most one machine, resolving it is unambiguous.
