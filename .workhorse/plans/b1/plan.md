# Kubernetes health checks in Canopy — brainstorm

Working notes for shaping k8s monitoring in Canopy. Not a spec yet; brainstorm in progress.

## Problem

Tamanu deployments on Kubernetes run no bestool (`alertd`), so they push no statuses and are invisible to Canopy's monitoring. Instead of porting bestool into k8s as an agent per server, Canopy reaches into the cluster: it runs the same alertd check suite against each server from inside the cluster, and determines its own checks about the substrate those servers run on.

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
- **Two sources, divided by subject** (revised by G2; this originally read "infra checks under `kubernetes`, database checks under `alertd`"). A check belongs to a source by **what it asserts something about**, not by how Canopy comes to observe it. `alertd`'s subject is one server: the thing with a database, a version, an API, sync state, and duties that ought to be running — the same subject on Kubernetes as on a VM, so the same check. `kubernetes`' subject is the substrate: what the cluster does with those workloads, at grains other than one server. The useful consequence is that **`alertd` can hold Kubernetes-only checks**: a check qualifies on its subject, not on whether it has a VM counterpart.
- **Reachability for a k8s server is Canopy-determined, not source-driven.** It's always green by default — a same-region cloud server is always reachable to Canopy, so reachability never alerts on harvest cadence (it's a coarse legacy signal; the full check suite tells you whether a server is serving). The one genuine failure is when the server's configured namespace no longer exists in the cluster: the server is gone, reported as unreachable. "Is this server up?" in the serving sense is carried by the server's own harvested checks, not reachability.
- **Harvested server checks are filed under `alertd`** (the source a VM Tamanu server uses), so a k8s server and a VM server share one catalog entry and one policy per check — a server condition means the same thing regardless of substrate. `alertd` is therefore a source with both pushed and Canopy-injected origins. Per G2 this covers the whole server-subject suite, not only the database checks.
- **Harvest failure is a single Canopy-wide failure, not per-server noise.** A Canopy-wide check per cluster covers connection + permissions (modelled like the backup-storage-identity self-alert). When Canopy can't reach a cluster, that check is the actionable failure; the affected servers' pulled/harvested checks go **broken** (unconfirmed) but are graded so they don't fail, so the fleet isn't polluted.

## Check suite

Two families per k8s server, split by subject rather than by how Canopy observes them (rebalanced by G2 — the original split put the infra list under `kubernetes` and only the database checks under `alertd`).

1. **Server checks — harvested via embedded bestool, filed under `alertd`.** The whole server-subject suite: the database conditions (sync, FHIR, migrations and the rest), plus whether duties are running and on the expected version, whether the API answers, storage headroom, and HTTP error rate. Approach: run the published `bestool-alertd` crate as a **Rust library** once per instance and file the results directly — no device-API push, no bestool binary running in-cluster. **The harvest runs inside the in-cluster relay (see Access to clusters), not a Canopy worker** — the relay connects to each instance's local `<prefix>-db-rw` (database `app`, credentials from the CNPG `<prefix>-db-app` secret, per J1) and reports only check results upward, so database credentials and query traffic never leave the cluster.
2. **Substrate checks — determined in the relay, filed under `kubernetes`.** What the cluster does with those workloads: a pod that cannot be scheduled, a volume that will not bind, and the coarser grains (a namespace, a cluster). Much smaller than originally planned, because server live, workloads ready, restarts, database up, storage and resource pressure are all server-subject and moved to the harvest.

So **M1 shrinks a long way and N1 grows to own most of what M1 was going to build**; both cards are rescoped accordingly.

**K2 then settled both families as check-shaped and pushed by the relay** (see Access to clusters), so the two no longer differ in how they reach Canopy — only in what they assert something about and which source they file under. M1's remaining Canopy-side work is registering the source and ingesting what the relay files, not deriving checks from objects it reads.

### The substrate abstraction, and why it is a bestool change

A host-subject check does not skip in a relay: the relay is a Linux process with a filesystem, memory and load of its own, so the check runs and reports **the relay pod's facts as the server's**, identically for every instance that relay serves. That passes, which is worse than failing. The fix is to give the check a *substrate* to ask instead of the local machine, keeping its graded logic and changing only the acquisition. That is a change to `bestool-alertd` itself (its Kubernetes implementation behind a feature, so a check's two behaviours cannot drift apart on separate release cycles), carried as a card in the **bestool** workspace and a prerequisite for N1.

The substrate answers more than a reading: also whether the subject is expected to be running at all (which separates a skip from a failure), the scope at which a reading is shared (per workload, per server, per cluster), and where a check's persistent state lives.

### Out of scope now

- **Backups.** K8s backups run at two layers (AWS-level, and Postgres via the CNPG Barman plugin), covered externally and not integrated in Canopy. Bringing them into Canopy is a separate future effort.
- **Cluster-infrastructure checks** (node pools, Karpenter and the like). Out of scope to build, but nothing may foreclose reporting a condition that touches a whole cluster; the Canopy-wide target with each cluster an instance is that pathway.
- **`memory` and `load` as gauges.** The Tamanu containers declare requests and no limits, so there is no per-container ceiling to take a percentage of. The condition worth alerting on is the OOM kill or eviction, an event rather than a gauge, covered by resource pressure.

## Access to clusters — resolved by H1: an in-cluster relay

The auth question is settled (H1). Rather than Canopy reaching into each cluster directly, a **small Canopy-authored relay runs in each cluster**: it holds the read permissions, connects to each local Postgres, and opens an outbound long-lived connection to Canopy. Canopy asks the relay for what it needs; the relay never accepts inbound and Canopy never talks to an external cluster's kube API directly.

- **Why the relay wins.** The capability surface becomes the relay's method set, not an RBAC surface — RBAC can't express "read this secret only to connect to that database", so any direct design hands Canopy `secrets: get/list/watch` fleet-wide. The relay also gives per-server Postgres reachability for free (dials `<prefix>-db-rw` on a ClusterIP), needs no outbound path from Canopy, and its dropped connection is a direct per-cluster connectivity signal — exactly the self-alert K8S already specifies.
- **The `alertd` harvest runs in the relay** (not a Canopy worker — revises the earlier plan): the relay embeds `bestool-alertd` as a Rust library, runs the checks against local Postgres, and reports only results up.
- **Transport:** QUIC (`quinn`) over Tailscale, TLS carrying throwaway certs (WireGuard already provides confidentiality/peer auth). Both ends need a **kernel-mode** Tailscale sidecar (`TS_USERSPACE=false`, `NET_ADMIN`, TUN) — userspace mode is TCP-only and QUIC won't pass. We already run QUIC over Tailscale elsewhere, so this is a known-good path, not a risk to retire. Canopy's sidecar goes on the singleton worker owning relay connections; the relay gets its own namespace.
- **Identity:** a relay is a **device with a new `role`**, authenticated by its tailnet peer tag (as `device_auth/tailnet.rs` already does for HTTP), taking the address from the QUIC connection. Optional cheap hardening: pin the relay's SPKI fingerprint at enrollment. Fits the existing device/association model without a new principal type.
- **Canopy stores no cluster credentials.** A registered cluster is a relay identity and nothing else — no secret at rest, no rotation to own.
- **Canopy's own cluster** (co-resident Tamanu test/dev): still open — run a relay there like any other (one code path), or read the local cluster directly with a widened ClusterRole (more robust, doesn't depend on the relay being up).
- **RBAC** moves to the relay's ServiceAccount (mostly `get`/`list`/`watch`; never `pods/exec` or `pods/portforward`). **G2 widens this beyond J1's list, and past per-namespace:** container memory needs `metrics.k8s.io`, PVC usage needs kubelet or CSI stats, and the HTTP error-rate check must list pods in `envoy-gateway-system` and scrape a port on them — outside the Tamanu namespaces entirely. So the assumption that a relay reads only the namespaces of the servers it serves does not hold, and exposing that surface resource-shaped would export a metrics proxy to Canopy.
- **The relay's authority is not read-only** (K2, departing from every prior document here). It gains the verbs that put a deployment to sleep and wake it. Still no `exec`, still no `portforward`.
- **Method set: both families check-shaped and pushed** (K2, revising H1's candidate middle that G2 had confirmed). The relay holds the check logic for the substrate as well as the harvest, computes in-cluster, and files upward; no method returns a Kubernetes object, so the relay is better described as Canopy's agent in the cluster than as a relay. The resource-shaped half collapsed on **release cadence, not on check logic**: its supposed advantage was iterating on substrate checks without redeploying relays, but the relay already redeploys on the fleet's cadence because it embeds `bestool-alertd` and SELF's skew check exists to keep that tracking the shipped bestool. Worth remembering that if the relay ever stopped tracking bestool, the argument would need revisiting.
- **Beyond filings, three named queries and two commands.** Queries: the namespace roster for L1's picker, the connected-and-answering handshake for K1's registration test, and the embedded suite version for SELF's skew alert. Commands: hibernate and wake. There is no operator-triggered re-run — the relay owns its own cadence.
- **Cadence.** The relay holds current state from list and watch, files a check when its result changes, and refiles everything periodically so state survives a missed event, a restart, or a dropped connection. Events are an accelerant, not the trigger of record, because a check's result is level-triggered and would otherwise leave Canopy holding a stale pass. The harvest cannot be event-driven, its readings being database queries, so it stays on a sweep cadence: two cadences in one relay.
- **Cost, accepted:** a second deployable with its own release cycle in every cluster, so version skew and protocol versioning from the start. H1 left open whether that cost is warranted at a fleet of two clusters; **decided that it is**, so the relay goes ahead at the current fleet size rather than waiting for the fleet to grow to justify it. The DB harvest forces either per-facility tailnet nodes or `portforward` under any direct design, and neither is acceptable, so the relay is the design.

The **Tailscale operator's API-server proxy** (auth mode, impersonating the tailnet identity) was the leading alternative and is now a rejected one, not a fallback held in reserve: it answers only the object-read half, still needs a second mechanism for the harvest, and requires Canopy to gain tailnet egress it does not have today. Revisit only if the relay's own foundations fail (see the transport spike), not on fleet-size grounds.

## Config surface

- **Cluster registration is relay enrollment** (reshaped by H1). A Canopy settings page still owns it and it's managed in-app, but a registered cluster is a relay identity, not a set of connection details and credentials Canopy stores. "Test the connection on add" becomes "is the relay connected and answering". The per-server k8s form's cluster picker draws from this registry. (K8S spec impact — see below.)
- **DB harvest credentials never reach Canopy.** Tamanu's k8s setup uses CNPG, which stores each instance's Postgres credentials as the `<prefix>-db-app` secret in the instance's namespace (J1). The **relay** reads that secret in-cluster and connects to `<prefix>-db-rw`; Canopy receives only check results.

## Putting a deployment to sleep (new capability, from K2)

The method set is the security boundary, so a mutation belongs in it deliberately rather than bolted on later. K2 designs in two commands, hibernate and wake, not immediately used.

- **A command targets a deployment**, which is a namespace, which is a server group at a rank. So hibernation acts on a whole group at once and there is no hibernating a single facility within a namespace. That follows the substrate rather than being a limitation to work around: CNPG hibernation and scaling to zero are what a deploy does to itself when its TTL expires, and the deploy is the unit that has a TTL.
- **The guardrail starts at has-a-TTL and is enforced at the relay.** A deployment carrying no TTL cannot be hibernated, and the relay is what refuses it, keeping the precondition where the fact lives (a TTL is cluster-side and Canopy may not hold it) and keeping the method set the boundary rather than trusting Canopy to ask correctly. Expected to widen later, so it is a policy on the command rather than a property of the deployment. Canopy gates production actions on top of that as a standing principle; no production deployment runs on Kubernetes today, so the relay-side precondition carries the weight in the meantime.
- **Whether a deployment is asleep is a fact, not a check** (following G2's hibernation-as-eligibility). Canopy presents it on the group, and each affected server's checks carry it as their skip reason. Registering a check for it would grade a deliberate act as a condition, and there is no result it could sensibly take.
- **Sleeping and waking are admin actions and are audited**, in the same vocabulary upgrade plans and restore-replica declarations already use.

## Specs written on this card

This card is the tracking issue/PR; it holds the specs, and the implementation sub-cards merge into it. Specs so far:

- **New:** `monitoring/kubernetes.md` (id `K8S`) — the umbrella: deployment shape, Kubernetes servers and identity picker, the in-cluster relay and what crosses its connection, cluster registry, the subject demarcation between the two sources, harvested server checks (`alertd`), substrate checks (`kubernetes`), putting a deployment to sleep, reachability for k8s servers, and cluster-read failure handling.
- **Fold** into `private-server/self-alerts.md` — the per-cluster connectivity self-alert (escalating), now expressed as relays disconnected or not answering; plus (G2) a second per-cluster alert for a relay harvesting with a check-suite version out of step with the fleet.
- **Fold** into `monitoring/checks.md` — `kubernetes` added to the reserved sources, and Canopy-determined reachability for servers monitored through a relay. G2 restated the `kubernetes` source as substrate checks at server, namespace, or cluster grain; K2 then replaced "Canopy-populated sources" with sources **filled by a cluster's relay filing what it determines**, since nothing is pulled.
- **Fold** into `public-server/statuses.md` — `alertd` has two origins: a device push, and a cluster's relay filing for the Kubernetes servers it harvests.
- **Fold** into `private-server/device-trust.md` — `relay` added to the device roles, associated with no server.
- **No fold needed** in `private-server/server-figures.md` (`FIG`), which G2 checked: its existing rules already handle a reporter that is not on the server, provided the harvest omits rather than synthesises. A k8s server presents no bestool version because that figure is never reported for it, and a server reporting no operating system already falls back to what its database engine gives away.

### Interaction with planned upgrades (`UPG`, landed upstream)

Noticed while rebasing onto main. UPG decides a plan is met once **the group's reported version** has reached its target, so for a Kubernetes group that judgement rests entirely on the harvest reporting the Tamanu application version — there is no agent on the server to report it. G2's choice already satisfies this (the version comes from the database's recorded `currentVersion`, which the check suite falls back to when there is no install, with the container image tag as a cross-check rather than the source), so nothing needs changing. Worth recording because the coupling is not obvious from either side: if the harvest were later trimmed to omit the application version along with the host-shaped fields it must omit, Kubernetes groups would silently never meet an upgrade plan.

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
- ~~**TTL hibernation**~~ — **resolved by G2.** Deploys with a TTL are scaled to zero and their CNPG clusters hibernated after the window. Under the subject demarcation this is server-subject, so it belongs to the harvest, and it is not a check result at all but an **eligibility fact**: a hibernated server is deliberately asleep, so its checks report **skipped** rather than failed or broken. A hibernated namespace is still present, so the server is not gone. Specified in K8S.

## Open questions

- ~~Cluster auth mechanism~~ — **resolved by H1: in-cluster relay dialling Canopy over QUIC/tailnet.** See Access to clusters.
- ~~Exact Tamanu k8s namespace layout~~ — **resolved by J1.** See `plans/j1/plan.md`; findings carried into the cards above.
- ~~**The relay's method set**~~ — **resolved by K2: both families check-shaped and pushed**, with three named queries and two commands alongside. This overturned H1's candidate middle, which G2 had confirmed, on release-cadence grounds. See Access to clusters and `plans/k2/plan.md`.
- When to bring backups into Canopy (AWS-level + CNPG Barman) — deliberately deferred.

### Spun out by K2, with no card yet

- **A general log of operator actions.** Wanted, and wider than hibernate/wake. Canopy has no such facility today: three specs say an action is audited (upgrade plans, restore-replica declarations and credential issuances, the backup recovery ceremony) and each is realised on its own terms, with no log covering operator actions across Canopy. K2 says only that sleeping and waking are audited and leaves the facility to a card of its own. That card does not exist, and the work is Canopy-wide rather than Kubernetes, so it does not belong in this breakdown.

### Left open by G2, for the implementation

- **Whether each threshold constant suits both substrates.** They were tuned against a VM. A per-check pass during N1; not a spec matter, since specs carry no thresholds.
- **Which grain the substrate reports at, per check.** The HTTP error-rate check needs a cluster-scoped reading fanned out per server where the others are per-workload, so the interface must admit both.
- **Where a check's persistent state lives.** Two alertd checks persist to one fixed path, so several instances driven in one relay process would read and write each other's state. The substrate has to carry a state location scoped to the subject.

### Settled without a spike

Two of H1's to-confirm items needed no investigation:

- **QUIC over the tailnet works.** Established practice elsewhere in our infrastructure, so the kernel-mode sidecar path is known-good statically. H1's fallbacks (tsnet at the relay end, HTTP/2 over TCP) are not on the table; note the canopy workspace itself gains `quinn` as a new dependency.
- **`bestool-alertd` is embeddable.** Published, and deliberately built as a library, so embedding it is the crate's expected use rather than an assumption to test.

What remained open was the harvest's **contract with Canopy's filing**, which G2 carried and has now landed (see `plans/g2/plan.md`). Its outcomes:

- **Parity is by construction, unless Canopy re-derives it.** The crate already builds its payload through the same serialisation a pushed bestool uses, so check names and the detail fields policy reaches as `check.<field>` match provided the relay produces the payload the same way and Canopy ingests it through the same path a device push takes. The one real risk is Canopy re-modelling the filing on its side.
- **Thresholds are compile-time constants** in each check, read from no config, so the two substrates cannot drift apart; what remains is whether a given number suits both.
- **Eligibility, not a curated subset.** Each check decides for itself whether it can run and reports skipped with a reason. Skips file normally — Canopy already carries a large proportion of skips and handles the volume in the UI rather than by withholding data.
- **The harvest reports on the server, never on the harvester.** The crate's server-wide detail is host-shaped by default and several fields are not optional, so they must be deliberately omitted rather than left to serialise the relay's hostname, OS, uptime, memory and networking as the server's. Synthesising a plausible value is worse than a gap, because it grades a real server against a fiction.
- **Version skew is relay metadata, not a server figure.** The relay's embedded suite version describes the relay, so it belongs to the cluster registry and the relay's device record, and skew becomes one comparison per cluster — a Canopy-wide check with each cluster an instance, sitting beside the connectivity check. Filing it as a server figure would put an identical row on every server a relay harvests and have an operator chase an upgrade on something that does not exist.
- **Aggregate by worst, never by sum,** where a check folds several subjects into one number. A sum hides the failure: one full volume beside an empty one of the same size reads as half full.

### Decided

- **The extra deployable is warranted** at a fleet of two clusters. The relay proceeds now; the API-server-proxy alternative is rejected rather than held as a fleet-size fallback (see Access to clusters).
- **The substrate lives in alertd behind a feature**, not as a trait the relay implements: a check's two behaviours belong in one crate so they cannot drift on separate release cycles, which is the property the reuse exists for. Cost is a `kube` dependency behind a feature and the discipline to keep it out of the default build.

### Decisions to make, not research

These are judgment calls with the tradeoffs already laid out; they need deciding rather than investigating.

- **Canopy's own cluster** — relay like any other (one code path), or direct in-cluster reads with a widened ClusterRole (robust to the relay being down). K8S is deliberately silent on this.
- **Whether to pin the relay's SPKI fingerprint** at enrollment, or rely on the tailnet ACL and tag check alone.
- **How a relay is enrolled**, which the DTR fold leaves unstated. Follows from the pinning decision and from who deploys the relay.
- **Who deploys the relay and how it is versioned** against Canopy, with protocol versioning needed from the start.
Hibernation is no longer among these: G2 settled it as an eligibility fact (see the J1 findings above).

## Resolved: identity stability

The namespace is the stable identity. A namespace changing means the server is gone, not that its identity drifted, so Canopy doesn't reconcile or protect against reassignment — that's an intentional operator act. The only guard needed is the reachability failure when the configured namespace is absent. A namespace disappearing and reappearing under the same name is picked back up, which is fine and intended.
