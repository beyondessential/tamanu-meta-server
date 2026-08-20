# Cluster authentication mechanism — research spike (H1)

Output of the H1 spike. Determines how Canopy authenticates to and reads from external
Kubernetes clusters, feeding the cluster-registration settings page specified in
`monitoring/kubernetes.md` (K8S). Companion to J1, which maps *what* Canopy reads inside
a namespace; this document covers *how* Canopy is authorised to read it.

Working direction, not yet settled. Open items are listed at the end.

## Sources

Every infrastructure fact below is read from `beyondessential/ops` (`pulumi/`) or the
Canopy tree, not inferred: `k8s-essentials/tailscale.ts` (operator install),
`k8s-essentials/roles/data-engineer.ts` (RBAC precedent),
`tamanu/on-k8s/src/central/CentralServer.ts` (database exposure),
`canopy/src/canopy-sa.ts` and `canopy/src/servers.ts` (Canopy's own posture),
`canopy/src/jobs.ts` (worker replica counts), `first-officer/src/index.ts` (per-cluster
service precedent), and `crates/commons-servers/src/` (device auth, backup secrets).

## The problem splits in two

The card frames the choice as Kubernetes-layer delegation vs network layer vs Canopy
owning an OIDC/IAM flow. Those stop competing once Canopy's two distinct needs are
separated, because they are answered at different layers:

1. **Reading Kubernetes objects** for the six infra checks, the identity picker, and the
   CNPG secret. This goes through the kube API server and needs *authorisation*.
2. **Opening a Postgres connection** to `<prefix>-db-rw` for the `alertd` harvest. This
   needs L4 *reachability* to a service inside the cluster, which reading objects through
   the API server does not provide.

Any design that only answers the first leaves the harvest needing a second mechanism.

## Decision: an in-cluster relay that dials Canopy

A small Canopy-authored service runs in each cluster. It holds the Kubernetes read
permissions, connects to each local Postgres, and opens an outbound long-lived connection
to Canopy. Canopy asks it for what it needs; the relay never accepts inbound connections
and Canopy never talks to the kube API of an external cluster directly.

### Why this over direct access

**The capability surface is the relay's method set, not the RBAC surface.** This is the
decisive argument. RBAC cannot express "you may read this Secret only in order to connect
to that database". Under any direct-access design Canopy holds `secrets: get/list/watch`
across every Tamanu namespace and is trusted to touch only `<prefix>-db-app`. Under the
relay, Canopy holds a verb that returns check results, and the credential is never on the
wire.

**Per-server database reachability comes free.** The relay reaches `<prefix>-db-rw` on a
ClusterIP. Direct access would need either one tailnet-exposed Service per facility per
deploy, or `create` on `pods/portforward`, which grants reach to any port on any pod and
defeats read-only access for a service identity.

**Canopy needs no outbound path to clusters.** The relay dials Canopy, which already
accepts inbound.

**Relay liveness is a direct connectivity signal.** A dropped connection is immediate,
rather than inferred from a failed poll. This maps onto the per-cluster connectivity
self-alert K8S already specifies.

### The alertd harvest runs in the relay

B1's plan has `bestool-alertd` embedded as a library in a Canopy worker, dialling each
instance's Postgres. It belongs in the relay instead: the relay runs the checks against
local Postgres and reports results upward. Database credentials never leave the cluster
and query traffic never crosses the network, only check results.

This is a revision to the B1 plan, and N1 is scoped from it.

### How the relay authenticates to Canopy

Preferred: a Tailscale sidecar on the relay pod, making the pod itself a tailnet node, so
no cluster-level egress plumbing is needed in the Tamanu cluster. One tailnet node per
cluster, not per facility. It dials Canopy's existing tailnet name and Canopy authorises
it by tag.

The alternative is reusing Canopy's device authentication (`device_auth/mtls.rs`,
`pop.rs`, `tailnet.rs`) with the enrollment-token and challenge flow in
`server_enrollment_tokens.rs` / `server_enrollment_challenges.rs`. This fits without
straining the model: `devices` carries no `server_id`, association runs through the
many-to-many `device_server_associations`, and a device with no server associations is
already an ordinary case that `tailnet_sweeps` explicitly handles. A relay is therefore a
device with a new `role` value, not a new principal type, and it inherits enrollment,
rotation and connection tracking as they stand.

The trade is that the sidecar route needs no PKI work at all, while the device route
reuses machinery Canopy already operates and keeps relay identity in the same place as
every other credentialed thing Canopy talks to.

### RBAC the relay needs

The read set does not disappear, it moves to the relay's ServiceAccount. What shrinks is
the blast radius of a Canopy compromise, which becomes the relay's method list rather than
the whole read set.

`k8s-essentials/roles/data-engineer.ts` is the pattern to copy: a ClusterRole bound to a
subject, whose rules are already close to what J1 requires. The relay's role grants
`get`/`list`/`watch` only over `pods`, `services`, `endpoints`, `namespaces`,
`persistentvolumeclaims`, `secrets`, `configmaps`, `apps/deployments`, `batch/jobs`,
`postgresql.cnpg.io/clusters`, `gateway.networking.k8s.io/gateways` and `httproutes`,
`networking.k8s.io/ingresses` while the Envoy migration lasts, and pod metrics. It carries
neither `pods/exec` nor `pods/portforward`, which `data-engineer` holds for humans.

### What Canopy stores per cluster

No cluster credentials. A registered cluster is a relay identity and nothing else, so
there is no secret to encrypt at rest and no rotation schedule to own. Testing a cluster's
connection at registration time is a check that its relay is connected and answering.

### Canopy's own cluster

Canopy already reads Kubernetes in its own namespace through its ServiceAccount
(`commons-servers/src/backup_secrets.rs` via `kube::Client::try_default()`, backed by the
namespaced `Role` in `canopy/src/canopy-sa.ts`). Reading co-resident Tamanu instances
needs that widened beyond its own namespace, which is the card's in-cluster requirement.

Two options: run a relay in Canopy's cluster like any other, keeping one code path and one
model; or read the local cluster directly with a widened ClusterRole, which is more robust
because it does not depend on the relay being up. Not settled.

## Costs

**A second deployable.** Canopy grows a service with its own release cycle, deployed into
every cluster. This brings version skew (relay vN against Canopy vM) and needs protocol
versioning from the start. It is the one cost the direct-access designs did not carry, and
it turns "Canopy connects to clusters" into "Canopy grows a distributed agent".

**Judged worth it** because the database harvest forces either per-facility tailnet nodes
or `portforward` under any direct design, and the relay is the only option that solves it
without widening the surface. Worth revisiting if the fleet stays at two clusters.

## Considered and rejected

**Canopy owning an IAM/OIDC flow.** Canopy already has IRSA: `canopy/src/canopy-sa.ts`
annotates every ServiceAccount with `eks.amazonaws.com/role-arn`. So cross-account
AssumeRole into a reader role, minting an EKS token per request, was viable with no new
network plumbing, reaching the EKS public endpoint over the internet. Rejected because it
answers only the object-read half, leaving the harvest to a second mechanism, and because
it gives Canopy the full RBAC surface.

**The Tailscale operator's API server proxy.** `k8s-essentials/tailscale.ts` sets
`apiServerProxyConfig.mode: 'true'`, which is auth mode rather than `noauth`: the proxy
authenticates the calling tailnet identity and impersonates it into the kube API via
`Impersonate-User` / `Impersonate-Group` headers derived from tailnet ACL grants. Every
cluster with `tailscaleEnabled` already publishes its API server on the tailnet as
`k8s-operator-<clusterFullName>`. This was the leading candidate and remains a sound
fallback. Rejected for the same two reasons as the IAM route, plus it requires Canopy to
gain tailnet egress, which it does not have today: there is no `tailnet-fqdn` ExternalName,
no egress ProxyGroup and no `subnetRouter` anywhere in the ops Pulumi, and the one
Connector is `exitNode: true`, which is the opposite direction.

Note that Canopy's tailnet Ingress (`canopy/src/servers.ts`, `ingressClassName:
'tailscale'`) is inbound only and says nothing about egress, and the existing Secret read
is in-namespace against the in-cluster API server, so neither demonstrates outbound
tailnet reachability.

## Failure domain

Routing cluster access over the tailnet introduces no new single point of failure.
Canopy's admin authentication is already tailnet-gated
(`commons-servers/src/tailscale_auth.rs` trusts `Tailscale-User-Login`), so a tailnet
outage already makes Canopy unreachable to operators.

Relay liveness becomes the per-cluster connectivity self-alert K8S specifies. Note the
diagnosis shifts: "cluster unreachable" becomes "relay not connected", which can mean the
relay is down while the cluster is healthy.

## Open items

- **Whether the extra deployable is warranted** at a fleet of two clusters, versus the API
  server proxy fallback.
- **Relay authentication to Canopy**: Tailscale sidecar and tag, or the existing device
  enrollment and mTLS machinery with a new device role.
- **Canopy's own cluster**: relay like any other, or direct in-cluster reads with a
  widened ClusterRole.
- **The relay's method set**, which is the security boundary and so needs designing
  deliberately rather than growing per check.
- **Who deploys the relay** and how it is versioned against Canopy.
- **K8S spec impact**: "Canopy reads each registered cluster with read-only access" and
  the registration page both change shape if registration becomes relay enrollment.
