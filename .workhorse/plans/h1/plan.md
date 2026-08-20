# Cluster authentication mechanism — research spike (H1)

Output of the H1 spike. Determines how Canopy authenticates to and reads from external
Kubernetes clusters, feeding the cluster-registration settings page specified in
`monitoring/kubernetes.md` (K8S). Companion to J1, which maps *what* Canopy reads inside
a namespace; this document covers *how* Canopy is authorised to read it.

## Sources

Every infrastructure fact below is read from `beyondessential/ops` (`pulumi/`), not
inferred: `k8s-essentials/tailscale.ts` (operator install),
`k8s-essentials/roles/data-engineer.ts` (RBAC precedent),
`tamanu/on-k8s/src/central/CentralServer.ts` (database exposure),
`canopy/src/servers.ts` (Canopy's own tailnet posture).

## The problem splits in two

The card frames the choice as Kubernetes-layer delegation vs network layer vs Canopy
owning an OIDC/IAM flow. Those stop competing once Canopy's two distinct needs are
separated, because they are answered at different layers:

1. **Reading Kubernetes objects** — the six infra checks, the identity picker, and
   reading the CNPG secret. This goes through the kube API server and needs
   *authorisation*.
2. **Opening a Postgres connection** to `<prefix>-db-rw` for the `alertd` harvest. This
   needs L4 *reachability* to a service inside the cluster. Reading objects through the
   API server does not provide it.

## Decision: both halves ride the Tailscale operator

The Tailscale Kubernetes operator is deployed in all clusters, and is already configured
for the object-read half.

### Object reads: the operator's API server proxy

`k8s-essentials/tailscale.ts` sets `apiServerProxyConfig.mode: 'true'`, which is auth
mode rather than `noauth`. The proxy authenticates the calling tailnet identity and
impersonates it into the kube API using `Impersonate-User` / `Impersonate-Group` headers
derived from tailnet ACL grants. Each cluster with `tailscaleEnabled` therefore already
publishes its API server on the tailnet as `k8s-operator-<clusterFullName>`.

Canopy dials that MagicDNS name as itself, and Kubernetes RBAC governs what it may read.
No AWS IAM, no STS AssumeRole, no EKS access entries.

Authorisation is gated twice, as it would have been under an IAM design: the tailnet ACL
grant controls which identities may reach a cluster's proxy and which k8s group they are
impersonated as, and the cluster's RBAC controls what that group may read.

### Consequence: there are no per-cluster credentials

This settles the card's credential-storage and rotation question. A registered cluster is
a MagicDNS hostname and nothing else. There is no secret to encrypt at rest, and no
rotation schedule to own: credentials reduce to Canopy's own tailnet node key, whose
rotation Tailscale already manages.

Testing a cluster's connection when it is added is a tailnet dial followed by a
`SelfSubjectAccessReview`, which verifies reachability and permissions together.

### RBAC: a `canopy-reader` ClusterRole, following the existing precedent

`k8s-essentials/roles/data-engineer.ts` is the pattern to copy: a ClusterRole bound to a
tailnet-derived group (`tailscale:data-engineer`). Canopy gets `canopy-reader`, bound to
Group `tailscale:canopy-reader`, granting `get`/`list`/`watch` only over J1's read set:
`pods`, `services`, `endpoints`, `namespaces`, `persistentvolumeclaims`, `secrets`,
`configmaps`, `apps/deployments`, `batch/jobs`, `postgresql.cnpg.io/clusters`,
`gateway.networking.k8s.io/gateways` and `httproutes`, `networking.k8s.io/ingresses`
while the Envoy migration lasts, and pod metrics.

It carries neither `pods/exec` nor `pods/portforward`, which `data-engineer` holds for
humans but a service should not.

### The cluster Canopy runs in

Registered and read like any other cluster, through its own operator's API server proxy.
This keeps one code path and one auth model, and means Canopy's own cluster needs no
special-casing to satisfy the card's requirement for an in-cluster policy reaching beyond
Canopy's namespace: the same `canopy-reader` ClusterRole covers it.

The alternative, an in-cluster ServiceAccount with a ClusterRoleBinding read directly via
`kube::Client::try_default()` (as `commons-servers/src/backup_secrets.rs` already does),
is more robust for the local cluster because it does not depend on the tailnet. It is
worth taking only if the uniform path proves awkward.

## Open: Canopy cannot currently dial out to the tailnet

Canopy has tailnet *ingress* only, via an Ingress with `ingressClassName: 'tailscale'`
(`canopy/src/servers.ts`). There is no sidecar, no Connector, and no `tailnet-fqdn`
egress Service anywhere in the ops Pulumi.

Both halves of this design need Canopy to be a first-class tailnet node with its own tag,
so it presents an identity the API server proxy can impersonate and can reach exposed
database nodes. This is one-time infrastructure work in `pulumi/canopy` and is a
prerequisite for the implementation cards.

An operator egress proxy (an `ExternalName` Service annotated `tailscale.com/tailnet-fqdn`
per target) is the wrong shape here: it needs one Service per target, and targets grow
with every facility.

## Open: facility databases are not exposed on the tailnet

`on-k8s/src/central/CentralServer.ts` exposes central's CNPG primary as a
`loadBalancerClass: 'tailscale'` Service named `central-db-tailscale`, hostname
`k8s-pg-<normalizedStack>`, selecting on `cnpg.io/cluster: central-db` and
`cnpg.io/instanceRole: primary`. `dbExpose` defaults to true, so central's database is
already reachable for every deploy.

Facilities have no equivalent, and the `alertd` harvest needs every facility's own
Postgres (J1 §5). Two candidate resolutions:

- **Mirror the pattern per facility** (`facility-<N>-db-tailscale`). Same shape, no new
  RBAC, keeps the read-only story intact. Costs one tailnet node per facility per deploy.
- **Use `pods/portforward` through the API server proxy.** Adds no tailnet nodes and
  keeps one mechanism, but `create` on `pods/portforward` grants reach to any port on any
  pod, which defeats least-privilege for a service identity.

## Failure domain

Routing all cluster access over the tailnet does not introduce a new single point of
failure. Canopy's admin authentication is already tailnet-gated
(`commons-servers/src/tailscale_auth.rs` trusts `Tailscale-User-Login`), so a tailnet
outage already makes Canopy unreachable to operators. The per-cluster connectivity
self-alert specified in K8S covers the case where one cluster's proxy is down.
