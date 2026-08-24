---
id: K8S
---

# Kubernetes monitoring

Canopy monitors Tamanu deployments running on Kubernetes through one relay in each cluster, which determines those deployments' checks and files them, rather than through a device on each server reporting its own (see [STA](../public-server/statuses.md)).
A Kubernetes server is an ordinary server in the fleet — it carries the same check state, health, incidents, and operator controls as any other (see [CHK](checks.md)) — monitored through its cluster rather than by an agent of its own.

## Deployment shape Canopy relies on

A namespace holds one deployment: a server group at a particular rank (for example the Nauru group at the demo rank is one namespace). Separate ranks are separate namespaces.
Within a namespace each central server and each facility has its own Postgres instance and its own workloads per duty — a central's tasks, sync, and API, and each facility's processes — with no database or workload shared between duties or between servers.
So a namespace's contents map precisely onto the servers Canopy already tracks, one server to one set of workloads and one Postgres.

## Kubernetes servers

A server running on Kubernetes is a normal server record whose target is the server itself.
It carries its Kubernetes coordinates: the cluster it runs in, the namespace it lives in, and, for a facility, the facility identity Canopy uses to locate that facility's databases and workloads within the namespace.

Whether a server runs on Kubernetes is a per-server fact, so a group at one rank can hold both Kubernetes servers and on-premises servers that sync to a Kubernetes central. Each server is monitored by the means its own record describes.

### Setting a server's identity

An operator marks a server as running on Kubernetes when creating or editing it, and chooses its cluster from the registered clusters and its namespace within that cluster.
Canopy reads the chosen namespace and lists the central servers and facilities running there; the operator selects which one this record is, and that selection becomes the identity Canopy's Kubernetes monitoring uses for the server.
Canopy does not discover Kubernetes servers on its own; identity is set by an operator.

The namespace is the server's stable identity. A namespace that changes means the server is gone rather than that its identity has drifted, so Canopy neither reconciles the mapping nor guards against an operator reassigning a record to a different deployment. A namespace that disappears and later returns under the same name is picked up again.

## The relay in each cluster

Canopy reads a cluster it does not run in through a relay Canopy runs inside that cluster, rather than by reaching the cluster's Kubernetes API itself.
The relay holds the permissions for its cluster, and it opens its connection to Canopy outward, so Canopy holds no credential to the cluster and the cluster accepts no connection from Canopy.
Canopy's authority over a cluster is therefore the set of requests its relay answers, each scoped to what a check, the identity picker, or an operator action needs, rather than a set of permissions over the cluster's objects.

A relay's connection is continuous, so Canopy observes the loss of a relay directly rather than inferring it from a request failing.
A relay is enrolled as a device carrying the relay role and is associated with no server, so it is created, authenticated, tracked, and revoked as any other device is (see [DTR](../private-server/device-trust.md)).

### What crosses the connection

The relay determines a Kubernetes server's checks itself, under both sources, and files the results upward.
Everything crossing the connection is a filed check, an answer to one named question, or an action on a deployment, so the cluster's objects stay in the cluster and what Canopy learns of a cluster is what its relay has already made a check of.

The relay holds the current state of what it watches and files a check when that check's result changes, so a condition surfaces when it arises rather than on the next turn of a polling loop.
It also refiles what it holds periodically, so a check's state is re-established after an observation the relay missed, a relay restart, or a reconnection, rather than resting on a change it may never see.

Beyond filings, Canopy asks a relay only for what is not a check: the roster of servers in a namespace for the identity picker, whether the relay is connected and answering for cluster registration, and the version of the check suite it runs (see [SELF](../private-server/self-alerts.md)).
Each is a named answer to one question rather than a means of reading the cluster.

### Keeping a relay current

Canopy names the version of the check suite each relay should be running, and a relay updates itself to the version named for it.
So the fleet of relays follows Canopy's account of where they should be rather than being rolled cluster by cluster, which is what keeps a change to a check reaching every cluster it applies to.

What Canopy names is a version and never the code itself: a relay obtains its code from where its versions are published, so the most Canopy can ask of a relay is which published version to run.
A relay refuses a version below the floor it carries, so it cannot be sent back to a release already known to be bad.

### Putting a deployment to sleep

Canopy can put a deployment to sleep and wake it again.
A deployment is a namespace and so a server group at a rank, so the action covers every server in that group together and there is no sleeping one server within a namespace.

A deployment that has no scheduled expiry cannot be put to sleep, and its relay is what refuses the request, so the restriction holds where the expiry is known rather than resting on Canopy asking correctly.
Canopy gates the action by what the deployment is, as it gates any action against production.
Sleeping and waking are available to admins and are audited.

Whether a deployment is asleep is a fact Canopy presents on the server group rather than a check, a deployment asleep on purpose being nothing to grade.
Each of its servers already carries it as the reason that server's checks are skipped (see "Checks that cannot run there").

## Cluster registry

Clusters are registered in Canopy through a settings page and managed in-app, not through environment configuration.
Registering a cluster enrols its relay, and Canopy confirms the relay is connected and answering before the cluster is saved, so a cluster Canopy cannot read is caught as the operator adds it.
A registered cluster is its relay's identity: Canopy stores no connection credential for a cluster, so it holds no cluster secret to protect or rotate.
Canopy supports several clusters at once.
Canopy can also read Tamanu instances running in the cluster Canopy itself runs in.

## What each source reports

A Kubernetes server's checks come from two sources, divided by what each check asserts something about rather than by how Canopy comes to observe it.

The `alertd` source's subject is one server: the thing that has a database, a version, an API, sync state, and a set of duties that ought to be running.
That subject is a coherent assemblage of processes or containers however they happen to be run, so it is the same subject on a Kubernetes server as on a server running the Tamanu services directly, and a condition about it is the same check on both.

The `kubernetes` source's subject is the substrate: what the cluster does with those workloads, at grains other than one server.
Coarser than a server, such as a namespace or a cluster, or finer, such as a single pod that cannot be scheduled or a volume that will not bind.

A check belongs to a source by its subject, not by whether it has a counterpart on other substrates.
So a check that asserts something about one server and is only expressible in Kubernetes is a server check, reported under `alertd`, and a condition that touches a whole cluster is reported at that grain rather than against the servers that happen to run there.

## Checks harvested for the server

Under the `alertd` source, Canopy files a Kubernetes server's checks by running the same Tamanu check suite that Tamanu servers run elsewhere, against that server, and filing the results itself.
The suite covers the conditions a server derives from its own database (the sync system, FHIR processing, migrations, and the rest), whether its duties are running and on the version they should be, whether its API answers, how much storage headroom it has, and its HTTP error rate.

Because it is the same check implementation, a Kubernetes server and a server that pushes its own reports share one catalog entry and one policy per check, are graded identically, and cannot drift into subtly different checks (see [CHK](checks.md), "Policy").
A check's thresholds come from that implementation on either substrate, so there is no per-server threshold configuration to hold and no way for one substrate's thresholds to drift from the other's; operators grade a check through its policy instead.

The relay runs the harvest inside the cluster and reports the results it produces.
It obtains each instance's database credentials from the cluster rather than from per-server configuration: the namespace holds the databases and the secret backing each, and the relay reads them there.
So a database credential and the queries the checks run against it stay within the cluster, and what crosses to Canopy is check results.

### Checks that cannot run there

Each check determines for itself whether it can run against a Kubernetes server, and reports skipped with its reason where it cannot.
A condition that does not exist on the substrate is skipped rather than failed, so it neither alerts nor ages into a check that is broken for want of ever running.

A deployment scaled to zero with its database hibernated is deliberately asleep rather than in trouble, so the server's checks are skipped for as long as it stays that way.
A hibernated namespace is still present, so the server is not gone (see "Reachability").

### The harvest reports on the server, never on the harvester

What the harvest files describes the server it names and nothing else.
The server-wide detail it carries omits anything that describes the process which produced it: the harvester's host, operating system, uptime, network, and its own version are not the server's, and presenting one as the other would state something false about the server rather than leave a gap.

So a figure a Kubernetes server has no source for is simply not reported, and the rules for a figure nothing reports apply as they do anywhere (see [FIG](../private-server/server-figures.md)).
In particular a Kubernetes server presents no bestool version, having no such agent installed on it, and the version of the check suite the harvest runs is a property of the relay rather than of any server it serves (see [SELF](../private-server/self-alerts.md)).

## Checks determined about the substrate

Under the `kubernetes` source, the relay determines checks about what the cluster does with a server's workloads and files them.
The `kubernetes` source is filed only by a relay; no ordinary device reports it and it is reserved from the device API (see [CHK](checks.md), "Sources").
Its checks register already reviewed, each with the policy its condition warrants.

Per server, the relay determines that the server's workloads can be placed, no pod of the server being unschedulable, and that its volumes are bound.

A check under this source can also be scoped past a single server, at either grain.
A check about a namespace targets the server group, a namespace being a server group at a rank (see [CHK](checks.md), "Targets").
A check about a cluster is Canopy-wide with each registered cluster an instance, as the cluster connectivity check is (see "When Canopy cannot read a cluster").

## Reachability

A Kubernetes server's reachability is determined by Canopy directly rather than from reporting sources (see [CHK](checks.md), "Reachability").
It passes by default: a server in a cloud Canopy shares a region with is reachable to Canopy under normal conditions, so reachability never alerts on the cadence at which its relay files.
It fails only when the server's configured namespace no longer exists in its cluster — the server is gone.
Whether a server is serving, as opposed to present, is carried by the server's own harvested checks, not by reachability.

## When Canopy cannot read a cluster

Canopy keeps one Canopy-wide check for Kubernetes cluster connectivity, with each registered cluster an instance of it (see [CHK](checks.md), "Checks with instances"), reported as a self-alert (see [SELF](../private-server/self-alerts.md)).
A cluster whose relay is not connected, or whose relay does not answer what Canopy asks of it, is a failing instance; the check's detail names every such cluster, and it recovers when every registered cluster is answering again.
This one check, escalating so it notifies at once, is the actionable failure for a cluster becoming unreadable.

What the check reports is that Canopy cannot read the cluster, a state a relay of its own that has stopped produces as readily as a cluster in trouble.
So its detail carries what Canopy last observed of the relay, enough for an operator to tell a relay that needs attention from a cluster that does.

While a cluster is unreadable, the substrate and harvested checks of the servers on it are broken — their conditions unconfirmed — and are not raised to failures, so a cluster Canopy cannot read surfaces through this one check rather than flooding every server on it.
