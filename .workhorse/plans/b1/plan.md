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
2. **Tamanu database checks — harvested via embedded bestool.** The valuable part of bestool (sync system metrics/checks, FHIR processing, and the rest of the Tamanu DB-level checks). Approach: integrate the published `bestool-alertd` crate as a **Rust library**, run the checks against each instance's own Postgres, and inject the results directly into Canopy — no device-API push, no bestool binary running in-cluster. **The harvest runs inside the in-cluster relay (see Access to clusters), not a Canopy worker** — the relay connects to each instance's local `<prefix>-db-rw` (database `app`, credentials from the CNPG `<prefix>-db-app` secret, per J1) and reports only check results upward, so database credentials and query traffic never leave the cluster.

### Out of scope now

- **Backups.** K8s backups run at two layers (AWS-level, and Postgres via the CNPG Barman plugin), covered externally and not integrated in Canopy. Bringing them into Canopy is a separate future effort.
- **bestool systemd service checks** — superseded by k8s liveness/health.
- **Container-composition checks** — covered by crashloop/restart + workloads-ready.

## Access to clusters — resolved by H1: an in-cluster relay

The auth question is settled (H1). Rather than Canopy reaching into each cluster directly, a **small Canopy-authored relay runs in each cluster**: it holds the read permissions, connects to each local Postgres, and opens an outbound long-lived connection to Canopy. Canopy asks the relay for what it needs; the relay never accepts inbound and Canopy never talks to an external cluster's kube API directly.

- **Why the relay wins.** The capability surface becomes the relay's method set, not an RBAC surface — RBAC can't express "read this secret only to connect to that database", so any direct design hands Canopy `secrets: get/list/watch` fleet-wide. The relay also gives per-server Postgres reachability for free (dials `<prefix>-db-rw` on a ClusterIP), needs no outbound path from Canopy, and its dropped connection is a direct per-cluster connectivity signal — exactly the self-alert K8S already specifies.
- **The `alertd` harvest runs in the relay** (not a Canopy worker — revises the earlier plan): the relay embeds `bestool-alertd` as a Rust library, runs the checks against local Postgres, and reports only results up.
- **Transport:** QUIC (`quinn`) over Tailscale, TLS carrying throwaway certs (WireGuard already provides confidentiality/peer auth). Both ends need a **kernel-mode** Tailscale sidecar (`TS_USERSPACE=false`, `NET_ADMIN`, TUN) — userspace mode is TCP-only and QUIC won't pass. Canopy's sidecar goes on the singleton worker owning relay connections; the relay gets its own namespace.
- **Identity:** a relay is a **device with a new `role`**, authenticated by its tailnet peer tag (as `device_auth/tailnet.rs` already does for HTTP), taking the address from the QUIC connection. Optional cheap hardening: pin the relay's SPKI fingerprint at enrollment. Fits the existing device/association model without a new principal type.
- **Canopy stores no cluster credentials.** A registered cluster is a relay identity and nothing else — no secret at rest, no rotation to own.
- **Canopy's own cluster** (co-resident Tamanu test/dev): still open — run a relay there like any other (one code path), or read the local cluster directly with a widened ClusterRole (more robust, doesn't depend on the relay being up).
- **RBAC** moves to the relay's ServiceAccount (`get`/`list`/`watch` only over the read set J1 enumerates; no `pods/exec` or `pods/portforward`).
- **Cost, accepted:** a second deployable with its own release cycle in every cluster, so version skew and protocol versioning from the start. H1 left open whether that cost is warranted at a fleet of two clusters; **decided that it is**, so the relay goes ahead at the current fleet size rather than waiting for the fleet to grow to justify it. The DB harvest forces either per-facility tailnet nodes or `portforward` under any direct design, and neither is acceptable, so the relay is the design.

The **Tailscale operator's API-server proxy** (auth mode, impersonating the tailnet identity) was the leading alternative and is now a rejected one, not a fallback held in reserve: it answers only the object-read half, still needs a second mechanism for the harvest, and requires Canopy to gain tailnet egress it does not have today. Revisit only if the relay's own foundations fail (see the transport spike), not on fleet-size grounds.

## Config surface

- **Cluster registration is relay enrollment** (reshaped by H1). A Canopy settings page still owns it and it's managed in-app, but a registered cluster is a relay identity, not a set of connection details and credentials Canopy stores. "Test the connection on add" becomes "is the relay connected and answering". The per-server k8s form's cluster picker draws from this registry. (K8S spec impact — see below.)
- **DB harvest credentials never reach Canopy.** Tamanu's k8s setup uses CNPG, which stores each instance's Postgres credentials as the `<prefix>-db-app` secret in the instance's namespace (J1). The **relay** reads that secret in-cluster and connects to `<prefix>-db-rw`; Canopy receives only check results.

## Specs written on this card

This card is the tracking issue/PR; it holds the specs, and the implementation sub-cards merge into it. Specs so far:

- **New:** `monitoring/kubernetes.md` (id `K8S`) — the umbrella: deployment shape, Kubernetes servers and identity picker, the in-cluster relay, cluster registry, pulled infra checks (`kubernetes` source), harvested DB checks (`alertd` source), reachability for k8s servers, and cluster-read failure handling.
- **Fold** into `private-server/self-alerts.md` — the per-cluster connectivity self-alert (escalating), now expressed as relays disconnected or not answering.
- **Fold** into `monitoring/checks.md` — `kubernetes` added to the reserved sources, the notion of Canopy-populated sources, and Canopy-determined reachability for pulled servers.
- **Fold** into `public-server/statuses.md` — `alertd` has two origins (device push and Canopy harvest).
- **Fold** into `private-server/device-trust.md` — `relay` added to the device roles, associated with no server.

The spec was deliberately written behavioural-only, leaving auth mechanism and exact namespace resource names to the spikes. J1 changes nothing behavioural (all implementation detail — see `plans/j1/plan.md`).

**H1's relay is now folded into the specs**, at the architectural level (the relay exists, it dials outward, Canopy holds no cluster credential, credentials and query traffic stay in the cluster) and without the transport and deployment mechanics (QUIC/quinn, tailnet sidecars, kernel-mode networking, SPKI pinning), which stay in `plans/h1/plan.md` as implementation. What landed:

- K8S gained a **The relay in each cluster** section, reshaped the cluster registry around relay enrolment (no stored credential, connection test = relay answering), pointed the infra checks and the DB harvest at the relay, and renamed the failure section to **When Canopy cannot read a cluster** with the relay-versus-cluster diagnosis carried in the check's detail.
- SELF's Kubernetes bullet now reads as relays disconnected or not answering.
- DTR gained `relay` as a device role, associated with no server.

Deliberately **not** specified, because it is not settled: whether Canopy's own cluster is read through a relay or directly (K8S stays silent on the mechanism there, saying only that Canopy can read instances in its own cluster), and how a relay is enrolled in the first place (DTR's "how a device comes to exist" list is untouched, so the relay's creation path is a spec gap once the mechanism is chosen).

## New findings from J1 to carry into the implementation cards

- **Positional facility prefix.** A facility's resource prefix is `facility-<N>` where N is a positional index, **not** the facility id or name. The id lives only in the `app.kubernetes.io/instance` label on app workloads and in the Gateway listener hostname. L1's picker must join across resource kinds (CNPG cluster ↔ app workloads ↔ Gateway) and **persist the prefix↔id/host binding**, because neither is derivable from the other.
- **Gateway API, not Ingress.** Query `Gateway`/`HTTPRoute`; tolerate an un-migrated namespace still on `Ingress` rather than reading the missing Gateway as "server gone".
- **Zero-replica duties are valid.** Read expected counts from each Deployment's own `.spec.replicas`; never assume a fixed count and don't alarm on a duty deliberately scaled to zero.
- **TTL hibernation — new behavioural question.** Deploys with a TTL are scaled to zero and their CNPG clusters hibernated after the window. A hibernated-but-present namespace is not a deleted one; M1/N1 should likely treat it as broken/unconfirmed rather than failing. Needs a decision on the source-worker cards (M1/N1/P1) and may warrant a spec line.

## Open questions

- ~~Cluster auth mechanism~~ — **resolved by H1: in-cluster relay dialling Canopy over QUIC/tailnet.** See Access to clusters.
- ~~Exact Tamanu k8s namespace layout~~ — **resolved by J1.** See `plans/j1/plan.md`; findings carried into the cards above.
- **The relay's method set** — check-shaped (`give me results for namespace X`, tightest surface, relay release per new check) vs resource-shaped (`list deployments in X`, Canopy keeps check logic but drifts toward an RBAC proxy). Candidate middle: resource-shaped for the `kubernetes` infra checks, check-shaped for the `alertd` harvest. H1 defers this to its own design card; it also settles the relay's implementation language.
- **Canopy's own cluster** — relay like any other, or direct in-cluster reads with a widened ClusterRole.
- When to bring backups into Canopy (AWS-level + CNPG Barman) — deliberately deferred.

### Raised to spikes

- **Does QUIC pass a kernel-mode Tailscale sidecar** against a real deployment? Gates the relay's transport; fallbacks are tsnet at the relay end or HTTP/2 over TCP at both.
- **Is `bestool-alertd` consumable as a Rust library** against a caller-supplied Postgres connection? Assumed by this plan and H1, examined by neither. Gates the harvest and settles the relay's language.

### Decided

- **The extra deployable is warranted** at a fleet of two clusters. The relay proceeds now; the API-server-proxy alternative is rejected rather than held as a fleet-size fallback (see Access to clusters).

### Decisions to make, not research

These are judgment calls with the tradeoffs already laid out; they need deciding rather than investigating.

- **Canopy's own cluster** — relay like any other (one code path), or direct in-cluster reads with a widened ClusterRole (robust to the relay being down). K8S is deliberately silent on this.
- **Whether to pin the relay's SPKI fingerprint** at enrollment, or rely on the tailnet ACL and tag check alone.
- **How a relay is enrolled**, which the DTR fold leaves unstated. Follows from the pinning decision and from who deploys the relay.
- **Who deploys the relay and how it is versioned** against Canopy, with protocol versioning needed from the start.
- **How M1/N1 treat a hibernated deploy** (see J1 findings above) — likely broken/unconfirmed rather than failing, but it wants a decision, and probably a spec line.

## Resolved: identity stability

The namespace is the stable identity. A namespace changing means the server is gone, not that its identity drifted, so Canopy doesn't reconcile or protect against reassignment — that's an intentional operator act. The only guard needed is the reachability failure when the configured namespace is absent. A namespace disappearing and reappearing under the same name is picked back up, which is fine and intended.
