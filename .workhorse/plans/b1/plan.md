# Kubernetes health checks in Canopy — brainstorm

Working notes for shaping k8s monitoring in Canopy. Not a spec yet; brainstorm in progress.

## Problem

Tamanu deployments on Kubernetes run no bestool (`alertd`), so they push no statuses and are invisible to Canopy's monitoring. Instead of porting bestool into k8s, Canopy pulls from the cluster and determines its own suite of checks for the workloads running there.

## Topology (as described)

- K8s is Tamanu-only for now; not used by other products.
- A namespace = one deployment = a server group **at a particular rank** (e.g. "Nauru Demo" = Nauru group, demo rank). Multiple ranks → multiple namespaces.
- Mixed deployments are supported on principle: one rank can span some k8s servers and some on-prem servers syncing to a k8s central. Handled per-server since the k8s flag is per-server.
- No database colocation: every central and every facility has its own Postgres instance. No shared DB clusters.
- No container sharing across duties: always a central tasks container, a central sync container, central API containers (usually two), and separate facility processes/tasks. Duties never share a container.
- No dedicated substrate server; everything runs on the k8s compute pool.

## Decisions so far

- **Each k8s Tamanu server stays a Canopy server record** (target = server). No new target type. The server carries k8s coordinates: it's a k8s server, which cluster, which namespace, and for facilities a facility name/ID/prefix used to locate its DB and containers.
- **Identity is set manually, not auto-discovered** (for now). On server create/edit: "is this server in Kubernetes? → which cluster + namespace?" Canopy then queries that cluster/namespace, lists the central servers and facilities running there, and the operator picks which one this record is. Auto-discovery may come later.
- **Checks come from a new `kubernetes` source** (Canopy-populated by pulling, not the reserved `canopy` source), so general cluster-health checks can grow under it. Distinct from device-pushed sources.
- **Reachability excluded for this source** (mode `off`) — keeps the single "a device reported recently" definition. "Is this server up?" is a dedicated k8s liveness check (ingress/Gateway + live front-end API), not the reachability signal.

## Access to clusters

- **Canopy's own cluster** (in-cluster): supports Tamanu test/dev instances co-resident with Canopy. A worker gains an RBAC policy to reach beyond its own namespace (kubelet etc.). Supported on principle; secondary.
- **External clusters** (the real target): design for **multiple**; one today, more later. All AWS EKS with OIDC identities. Auth layer undecided — could be k8s-layer delegation/tunnel, network layer, or Canopy owning OIDC/IAM to mint a per-cluster token and pull directly. **Open — needs research.**

## Open questions

- The check suite: what conditions the `kubernetes` source determines per server (pods/deployments readiness, crashloops/restarts, per-container health, per-instance Postgres, PVC/storage, resource pressure/OOM, the "server live" check).
- The "some bestool checks somehow" gap: which bestool-only checks (backups, migrations, disk, cert expiry) still matter for k8s servers, and how they're covered.
- Cluster auth mechanism (EKS OIDC/IRSA vs alternatives).
- How the server↔workload identity mapping stays valid as namespace contents drift.
- How/where cluster connection credentials are stored and configured.
