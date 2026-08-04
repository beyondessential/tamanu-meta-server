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
- **Harvested Tamanu DB checks are filed under `alertd`** (the source a VM Tamanu server uses), so a k8s server and a VM server share one catalog entry and one policy per check — sync/FHIR health means the same thing regardless of substrate. `alertd` is therefore a source with both pushed and Canopy-injected origins.
- **Harvest failure is a single Canopy-wide failure, not per-server noise.** A Canopy-wide check per cluster covers connection + permissions (modelled like the backup-storage-identity self-alert). When Canopy can't reach a cluster, that check is the actionable failure; the affected servers' pulled/harvested checks go **broken** (unconfirmed) but are graded so they don't fail, so the fleet isn't polluted.

## Check suite

Two families per k8s server:

1. **Infra checks — pulled from the kube API.** Server live (ingress/Gateway + front-end API answers), workloads ready (expected replicas ready), crashlooping/restarts, Postgres instance healthy, storage/PVC, resource pressure (OOM/eviction). These also replace bestool's systemd service-level checks: k8s liveness/health is a better signal than "is this service up", and FHIR/sync are covered via the database anyway. Container-composition checks aren't needed — trust k8s' own tracking; crashloop/restart + workloads-ready cover the shape.
2. **Tamanu database checks — harvested via embedded bestool.** The valuable part of bestool (sync system metrics/checks, FHIR processing, and the rest of the Tamanu DB-level checks). Approach: integrate the published `bestool-alertd` crate as a **Rust library** inside a Canopy worker, connect it to each instance's own Postgres, run the checks, and inject the results directly into Canopy — no device-API push, no bestool binary running in-cluster.

### Out of scope now

- **Backups.** K8s backups run at two layers (AWS-level, and Postgres via the CNPG Barman plugin), covered externally and not integrated in Canopy. Bringing them into Canopy is a separate future effort.
- **bestool systemd service checks** — superseded by k8s liveness/health.
- **Container-composition checks** — covered by crashloop/restart + workloads-ready.

## Access to clusters

- **Canopy's own cluster** (in-cluster): supports Tamanu test/dev instances co-resident with Canopy. A worker gains an RBAC policy to reach beyond its own namespace (kubelet etc.). Supported on principle; secondary.
- **External clusters** (the real target): design for **multiple**; one today, more later. All AWS EKS with OIDC identities. Auth layer undecided — could be k8s-layer delegation/tunnel, network layer, or Canopy owning OIDC/IAM to mint a per-cluster token and pull directly. **Open — needs research.**

## Open questions

- The check suite: what conditions the `kubernetes` source determines per server (pods/deployments readiness, crashloops/restarts, per-container health, per-instance Postgres, PVC/storage, resource pressure/OOM, the "server live" check).
- The "some bestool checks somehow" gap: which bestool-only checks (backups, migrations, disk, cert expiry) still matter for k8s servers, and how they're covered.
- Cluster auth mechanism (EKS OIDC/IRSA vs alternatives).
- How the server↔workload identity mapping stays valid as namespace contents drift.
- How/where cluster connection credentials are stored and configured.
