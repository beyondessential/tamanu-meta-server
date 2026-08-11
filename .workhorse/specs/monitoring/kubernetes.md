---
id: K8S
---

# Kubernetes monitoring

Canopy monitors Tamanu deployments running on Kubernetes by connecting to their clusters and determining checks itself, rather than a device on each server pushing them (see [STA](../public-server/statuses.md)).
A Kubernetes server is an ordinary server in the fleet — it carries the same check state, health, incidents, and operator controls as any other (see [CHK](checks.md)) — reached by pulling rather than by being pushed to.

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

## Cluster registry

Clusters are registered in Canopy through a settings page and managed in-app, not through environment configuration.
Adding a cluster tests the connection before it is saved, so wrong details are caught as the operator enters them.
Canopy reads each registered cluster with read-only access and supports several external clusters at once.
Canopy can also read Tamanu instances running in the cluster Canopy itself runs in.

## Checks Canopy determines from the cluster

Under the `kubernetes` source, Canopy determines a server's infrastructure checks by reading its cluster and namespace.
The `kubernetes` source is populated by Canopy pulling; it is not reported by any device and is reserved from the device API (see [CHK](checks.md), "Sources").
Its checks register already reviewed, each with the policy its condition warrants.

Per server:

- **Server live** — the server's ingress or Gateway resolves and its front-end API answers.
- **Workloads ready** — the server's workloads have their expected replicas ready.
- **Restarts** — none of the server's containers is crash-looping or restarting abnormally.
- **Database up** — the server's own Postgres instance is up and accepting connections.
- **Storage** — the server's volumes are bound and not near full.
- **Resource pressure** — the server's containers are not being out-of-memory killed or evicted.

These carry the signal that a host-level service check carries on a server that reports its own services: Kubernetes' own liveness and health, surfaced as the checks above, is the authoritative account of whether a server's services are running.

## Checks Canopy harvests from the database

Canopy harvests the Tamanu checks a server derives from its own database — the sync system, FHIR processing, and the rest of the database-level conditions — by running the same check logic Tamanu servers use elsewhere against each instance's Postgres and filing the results itself.
These are filed under the `alertd` source, the source a Tamanu server reports on other substrates, so a Kubernetes server and a server that pushes its own reports share one catalog entry and one policy per check and are graded identically (see [CHK](checks.md), "Policy").
The harvest reuses the same alertd check implementation the servers use, so the two never diverge into subtly different checks.

Canopy obtains each instance's database credentials from the cluster rather than from per-server configuration: the namespace holds the databases and the secret backing each, and Canopy reads them there.

## Reachability

A Kubernetes server's reachability is determined by Canopy directly rather than from reporting sources (see [CHK](checks.md), "Reachability").
It passes by default: a server in a cloud Canopy shares a region with is reachable to Canopy under normal conditions, so reachability never alerts on the cadence of Canopy's own pulling.
It fails only when the server's configured namespace no longer exists in its cluster — the server is gone.
Whether a server is serving, as opposed to present, is the **Server live** check above, not reachability.

## When Canopy cannot reach a cluster

Canopy keeps one Canopy-wide check for Kubernetes cluster connectivity, with each registered cluster an instance of it (see [CHK](checks.md), "Checks with instances"), reported as a self-alert (see [SELF](../private-server/self-alerts.md)).
A cluster Canopy cannot reach, or lacks the permissions to read, is a failing instance; the check's detail names every such cluster, and it recovers when every registered cluster is reachable again.
This one check, escalating so it notifies at once, is the actionable failure for a cluster going away.

While a cluster is unreachable, the pulled and harvested checks of the servers on it are broken — their conditions unconfirmed — and are not raised to failures, so an unreachable cluster surfaces through this one check rather than flooding every server on it.
