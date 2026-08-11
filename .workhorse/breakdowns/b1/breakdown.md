# Kubernetes health checks in Canopy — card breakdown

Children spawned from the k8s-health-checks brainstorm. The parent stays the umbrella; these carry the work. Two research spikes come first because their outcomes constrain the implementation, which the specs (see `monitoring/kubernetes.md`) describe at a behavioural level. The implementation cards below are drafts pending the spikes — each names the spike it waits on, and their scope firms up once H1 and J1 land.

## Research spike: cluster authentication mechanism · H1

Determine how Canopy authenticates to and reads from external Kubernetes clusters, so the implementation can settle its credential storage and connection code. The target clusters are all AWS EKS with OIDC identities; there is one external cluster today and at least one more expected, so the mechanism must support multiple. Canopy also runs in one EKS cluster itself and should be able to read Tamanu instances co-resident there via an in-cluster RBAC policy that reaches beyond its own namespace. Weigh the layer the auth should live at — Kubernetes-level delegation or a tunnel, network-level access, or Canopy owning an OIDC/IAM flow to mint a short-lived per-cluster token and pull directly — against operability and least-privilege read-only access. Output: a chosen mechanism, the RBAC/permissions Canopy needs, and how per-cluster credentials are stored and rotated, feeding the cluster-registration settings page.

## Research spike: Tamanu Kubernetes namespace layout · J1

Map what actually exists in a Tamanu Kubernetes namespace and where, from the perspective of what Canopy needs for its checks and setup. Canopy needs, per namespace (one server group at one rank): the list of central servers and facilities running there for the server-identity picker; each server's workloads by duty (central tasks, central sync, central API replicas, facility processes/tasks) for readiness, crashloop/restart, and resource-pressure checks; the ingress or Gateway resource and front-end API for the liveness check; the PVCs for the storage check; each instance's own Postgres and the CNPG-managed Kubernetes secret backing it for the DB-check harvest; and the facility name/ID/prefix used to locate a facility's databases and containers. The card describes that required-inputs list, then works out where each piece lives in the namespace and how it's labelled or named, so the implementation can query it reliably.

## Cluster registry and connection

Draft, waits on H1. Build the in-app cluster registry the specs describe: a settings page where an admin registers a Kubernetes cluster, with a connection test run before the cluster is saved so wrong details are caught up front. Persist each cluster's connection details and credentials, and provide the read-only cluster access the rest of the work builds on, covering both external clusters and the cluster Canopy runs in. The auth mechanism and credential storage/rotation come from H1; this card wires that mechanism into a managed, tested registry rather than environment configuration. Verifies spec: K8S.

## Server Kubernetes identity and picker

Draft, waits on J1. Add the per-server Kubernetes coordinates — cluster, namespace, and for a facility its facility identity — to server create/edit. When an operator marks a server as running on Kubernetes and picks a registered cluster and a namespace, Canopy reads that namespace, lists the central servers and facilities running there, and the operator selects which one the record is; that selection becomes the identity the monitoring uses. Present a server's Kubernetes coordinates wherever its classification is shown. Depends on the cluster registry for cluster access and on J1 for how centrals and facilities are identified in a namespace. Verifies spec: K8S.

## Infrastructure checks (`kubernetes` source)

Draft, waits on H1 and J1. A worker that reads a Kubernetes server's cluster and namespace and determines its infrastructure checks under the new `kubernetes` source: server live (ingress/Gateway plus front-end API), workloads ready, restarts/crash-looping, database up, storage, and resource pressure. Register the `kubernetes` source as reserved and its checks as already reviewed with sensible policy. Depends on cluster access (H1) and on J1 for locating a server's workloads, ingress/Gateway, Postgres, and volumes within the namespace. Verifies spec: K8S.

## Harvested database checks (`alertd` source)

Draft, waits on H1 and J1. Integrate the published `bestool-alertd` crate as a library in a Canopy worker, connect it to each instance's own Postgres — credentials sourced from the CNPG secret in the namespace, not per-server config — run the Tamanu database-level checks (sync, FHIR, and the rest), and file the results under the `alertd` source so a Kubernetes server and a pushed server share one catalog entry and policy per check. Reuse the same check implementation so the two never diverge. Depends on cluster access (H1) and on J1 for database and secret discovery. Verifies spec: K8S.

## Reachability and cluster-failure handling

Draft, waits on the source worker. Make a Kubernetes server's reachability Canopy-determined: green by default, failing only when the configured namespace no longer exists. Add the Kubernetes cluster connectivity self-alert (escalating) as one Canopy-wide check with each cluster an instance, and when a cluster is unreachable, mark its servers' pulled and harvested checks broken without raising them to failures, so an unreachable cluster surfaces through that one check rather than flooding the fleet. Verifies spec: K8S. Verifies spec: SELF. Verifies spec: CHK.
