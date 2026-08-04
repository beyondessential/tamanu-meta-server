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
- **Reachability for a k8s server is Canopy-determined, not source-driven.** It's always green by default — a same-region cloud server is always reachable to Canopy, so reachability never alerts on harvest cadence (it's a coarse legacy signal; the full check suite tells you whether a server is serving). The one genuine failure is when the server's configured namespace no longer exists in the cluster: the server is gone, reported as unreachable. "Is this server up?" in the serving sense is the dedicated k8s liveness check (ingress/Gateway + front-end API), not reachability.
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

## Config surface

- **Cluster registration** is a Canopy settings page, managed in-app rather than by environment variables. Wizard-style: the admin enters the cluster's details and Canopy tests the connection on add, so bad details are caught up front. The per-server k8s form's cluster picker draws from this registry.
- **DB harvest credentials** come from the cluster, not per-server config. Tamanu's k8s setup uses CNPG, which stores each instance's Postgres credentials as a Kubernetes secret in the instance's namespace. Canopy discovers the databases available and the secret backing each, so registering the cluster is enough to harvest.

## Specs written on this card

This card is the tracking issue/PR; it holds the specs, and the implementation sub-cards merge into it. Specs so far:

- **New:** `monitoring/kubernetes.md` (id `K8S`) — the umbrella: deployment shape, Kubernetes servers and identity picker, cluster registry, pulled infra checks (`kubernetes` source), harvested DB checks (`alertd` source), reachability for k8s servers, and cluster-connection failure handling.
- **Fold** into `private-server/self-alerts.md` — the per-cluster connection/permissions self-alert (escalating).
- **Fold** into `monitoring/checks.md` — `kubernetes` added to the reserved sources, the notion of Canopy-populated sources, and Canopy-determined reachability for pulled servers.
- **Fold** into `public-server/statuses.md` — `alertd` has two origins (device push and Canopy harvest).

Behavioural level only: auth mechanism and exact namespace resource names are left to the two spikes (H1, J1).

## Open questions

- Cluster auth mechanism (EKS OIDC vs alternatives) — moved to a research spike (see card-plan).
- Exact Tamanu k8s namespace layout and where each check input lives — moved to a research spike (see card-plan).
- When to bring backups into Canopy (AWS-level + CNPG Barman) — deliberately deferred.

## Resolved: identity stability

The namespace is the stable identity. A namespace changing means the server is gone, not that its identity drifted, so Canopy doesn't reconcile or protect against reassignment — that's an intentional operator act. The only guard needed is the reachability failure when the configured namespace is absent. A namespace disappearing and reappearing under the same name is picked back up, which is fine and intended.
