---
status: draft
---

# Split the core model: machines, application servers, and identities

Separate Canopy's `server` into a machine and an application server, and retire `device` into an identity and a machine, so machine-level facts have somewhere of their own to live.

## Terminology

Direction agreed: rename rather than qualify.
"Server" means too many things, and "application server" carries the confusion forward.

- **Application** — what a `server` is today, minus its machine-ish columns. What the status page presents.
- **Machine** — the host an application runs on. New concept, though bestool's existing server ID is really this.
- **Identity** — what a `device` is today: a set of keys, with an optional Tailscale identity. Not a machine.

A machine usually has an identity, and one or more applications on it.

## Behaviour

### Cardinality

- An application has zero or one machine, never two.
- A machine has one or more applications.
  Whether "one or more" is enforced is an open question (see below) — the delete-then-recreate case wants a temporary zero.
- On Kubernetes an application has no machine at all; machine-less is a first-class case, not a degenerate one.
- A Kubernetes cluster is a separate entity beside machines, not a machine of a special class.

### How a machine comes into being

There is no standalone "create a machine" flow, at least not initially.
Machines are created through creating applications, which keeps the invariant that a machine has applications on it reachable by construction.

- The migration creates one machine per existing server, 1:1.
- The application creation form gains a machine section.
  By default it creates a new machine for that application.
  Unticking that offers a search dropdown to select an existing machine instead.

### Identities and the machine link

An identity does gain a machine association, but only routes that need it reach for it.

Extraction is per-route, following the role-gated extractor pattern already in `crates/commons-servers/src/device_auth/mod.rs` (`ServerDevice`, `AdminDevice`, `ReleaserDevice`, `BackupRestoreDevice`).
A route gated to machines resolves the machine from the authenticated identity.
A route gated to something else — an admin, a releaser — resolves no machine, so the association stays invisible to everything that has no business with it.

This drops an existing awkwardness.
`/servers/self` (see [DID](../../specs/public-server/device-identity.md)) refuses when the resolved device is attached to more than one server, and `Server::live_by_device_id` returns a `Vec` for the same reason.
An identity resolves to exactly one machine, and applications hang off the machine, so the ambiguity has nowhere left to arise.

### What sits on which

The dividing principle is not "machine-ish sounding".
Most of what a machine is gets *reported*, so it arrives as sourced detail and presents as a figure; only a few things are operator-set columns.

Stays with the application, and `host` is renamed **`url`** while we are here, since it is a URL and calling it a host was part of the confusion:

- `url` (today `host`), and the name-management fields that work from it: `may_manage_dns`, `may_manage_tls`, `certificate_profile`, the name-management pause fields.
- `product`, `kind`, `rank`, `name`, `public_name`, `group_id`, `notes`, `tags`.
- `registered_at`, `deleted_at`, the restore-window fields.

Moves to the machine:

- The identity link (today `device_id`).
- `cloud` and `geolocation`, both operator-set today (`detect_cloud` only seeds `cloud` from an enrollment hint).
- `alert_when_down_for`, since reachability is a machine fact.

Carried by both, rather than moving:

- **Tags, and resolved billing tags.** A machine-subject check is graded by policy rules against its target's tags, so a machine without tags could not be graded the way an application is.
  Both grains carry both, and a check decides which it reads.
  `product` and `kind` stay reserved read-only tags on applications only, since neither is a property of a box.

Billing is why both need billing tags rather than one.
On a VM the cost is incurred by the machine, so its billing tags belong there.
On Kubernetes there is no machine and the cost attaches to the application server, so the reverse holds.

### Groups

The group lives on the application and only on the application, because a machine-less application still has to belong to one.
A machine's group is never independently set and can never disagree with its applications'.

A machine still needs a group in hand: `Scope::resolve_incident_target` (`crates/database/src/issues.rs:834`) resolves through it, so a machine-targeted check with no group has no incident path, and `disk_free` and `memory` are exactly the checks that should page.

Taking the call left open: the machine carries a **derived** group column, maintained from its applications rather than edited.
It is a denormalisation for query performance, with the application's column the source of truth, so the two cannot drift by construction.

This holds because a shared machine is always shared within one deployment, so every application on a machine is in the same group and the derived value is never ambiguous.
That is a real constraint, not just an observation: it makes the machine dropdown in the application creation form a choice among the machines already in that application's group.

Moving an application to a different group while it shares a machine with applications staying behind is refused.
It has no meaning in the model, and forbidding it stops it happening by accident rather than leaving a machine with two groups to derive from.

### Billing attribution for a machine

A machine's billing attribution is assembled from the applications on it, since `product` and `rank` stay there.

- **Product** is the list of its applications' products, rather than one picked among them.
- **Stage** is the highest rank across its applications, on the existing ordering (`ServerRank` derives `Ord` production-first, `crates/commons-types/src/server/rank.rs:30`), so a box shared by a production and a test workload bills as production.
- **Deployment** is the group, which is unambiguous given the rule above.

A group's attribution is not the precedent here.
[APP](../../specs/servers/products.md) has a group attribute no product when its live members span products, justified as avoiding charging the shared cost to the wrong place.
That justification does not hold up: product is not a group-level fact at all, so there is nothing to get right or wrong.
Arguably a group should carry no product in its attribution in the first place, and the "only when they agree" rule is working around a field that should not be there.

A machine is different in kind, not a special case of the same rule.
A machine spanning products is one box genuinely running both, so the cost really is shared and listing both products is the truthful answer.

Reported rather than stored as a column:

- The machine's hostname.
  bestool will report it, and it attaches to the machine as sourced data — a figure like any other, not a dedicated field.
  This matters because it is the machine's own name, which is a different thing from an application's `url`, and two applications on one machine have one hostname between them and a `url` each.

### The model splits; the UI mostly does not

The separation is a modelling one, and an operator should not have to navigate it.

The edit form is **machine-first**: one form per machine, holding a machine section and one section per application on it.
So a shared machine is edited in the one place that shows everything sharing it, and a change to the machine's fields is visibly a change to all of them.
A one-application machine reads as a single form with two sections, which is close to the server form as it stands.

The same holds for controls that follow a grain.
The unreachability silence lives mechanically on whatever carries reachability, which is the machine, but ergonomically it sits wherever the thing it silences is reported.
So an operator quiets a host that is expected to be down without first working out which record owns the switch.

### Identity roles

The `server` identity role becomes `machine`, so a role names what the identity is installed on.
Inputs accept the old name as an alias for compatibility, so a fielded agent enrolling as `server` keeps working.

### Monitoring and reachability

Reachability is a machine-level concept, and a cluster-level one on Kubernetes.
Applications carry a monitoring on/off toggle; machines carry one too.

Every application on an unreachable machine is itself unreachable.
That is a derivation and not a second filing: one `reachability` check exists, on the machine, and it fails once.
So a dead host is one check, one issue, and one incident, with every application on it presenting as unreachable — the "one fact with N consequences" the split is for.

An application presenting as unreachable is how an operator reads its checks correctly.
The check states keep their last observed results, which would otherwise read as current, and the unreachable presentation is what says they are not.

An unreachable application is also **unhealthy**, not merely marked.
If the machine is down, so is everything on it, and a rollup reading healthy off stale checks would be a lie.

That does pull towards applications having a liveness signal of their own, since unreachability now behaves per-application for health while being filed once.
The split holds for now: one filing, one issue, one incident, and health derived onto each application.
Worth revisiting if a second application-subject reporter ever appears.

Two ways to land the health part, and the choice is not settled:

- **Extra clause in the rollup.** Health is derived from the checks contributing, *plus* the reachability of the application's machine.
  Changes the rollup rule in [CHK](../../specs/monitoring/checks.md).
- **Inherited check.** The machine's reachability check presents in each application's check list, so the existing rollup rule reaches it without changing.
  Costs one check appearing against several targets, which nothing in the model does today.

Either way there is one filing, so incidents are unaffected: still one, from the machine's scope.

## Reachability as it stands today

Three distinct things wear the name, and they do not split the same way.

**1. `ShortStatus`** (`crates/commons-types/src/status.rs:49`) — up / blip / away / down / gone, derived purely from the age of the latest status row for a server against fixed thresholds (2, 10, 30 minutes).
Presentational only: status dots, the MCP fleet summary's `unreachable` count.
Nothing alerts off it.

**2. The `canopy`/`reachability` check** (`Status::sweep_staleness`, `crates/database/src/statuses.rs:275`) — one check per server, computed from per-source freshness against that server's own `alert_when_down_for` interval, each source graded by its reachability mode (`on`/`quiet`/`off`).
This is the one that alerts, is silenceable (the "Alert when this server is unreachable" switch in `ServerEdit.tsx` *is* the server-scoped silence on it), and feeds incidents and the health rollup.

Pingtask, the third, is being removed as this card is written, so an active HTTP probe of the application is no longer part of the picture.
The backstop it justified in the sweep — "servers with no counted source fall back to the latest status row, any source" — goes with it.

### What that implies for the split

Both are "did a reporter reach us recently".
The reporter is bestool, and bestool runs on the machine, so both are machine facts.
That supports machine-primary.

The **legacy `tamanu` source** (see [STA](../../specs/public-server/statuses.md), "Legacy pushes"; `crates/public-server/src/statuses.rs:128`) was the other application-subject reporter, but it is going: one server still sends it, and that stops within about a month of this card.
So every remaining reporter is an agent installed on something.

### Reachability attaches to whatever hosts the reporter

That is the rule the split needs, and it holds in both contexts.

On a VM the reporter is bestool, installed on the machine, so bestool going quiet is a machine fact — one fact, with every application on that machine as its consequence.

On Kubernetes the reporter is the relay, and there is one relay per cluster holding one outbound connection (see card J2).
The relay going quiet is a cluster fact, with every application on the cluster as its consequence.
Liveness is not per-application on Kubernetes, because the namespace is a *filing grain* for checks and never a reporter.

So reachability is carried by machines and by clusters, and by neither applications nor namespaces.
This keeps "machine" an honest name: it does not have to stretch to cover clusters, because a cluster is its own entity carrying its own reachability.

An application never goes quiet in its own right.
A dead application on a live machine is not silence — bestool keeps reporting and grades the application's own checks as failed or broken.
Silence only ever means the agent stopped, and the agent belongs to the machine or the cluster.

Health is already documented as independent of reachability (`status.rs:406`), so the two axes are established; this adds a grain to one of them.

Residual edge: a machine hosting two applications reported by two different agents, where one agent goes quiet.
Machine reachability counts sources, so the quiet one raises the machine's reachability warning without saying which application lost its reporter.
Probably acceptable, and the source detail names the quiet source anyway.

### Figures

Figures split by which grain the fact belongs to, but present in one view rather than two.

Machine figures: the platform and operating system version, the timezone, the hostname, and the bestool version.
Also the machine facts bestool does not report yet — uptime, CPU, memory, filesystems, addresses.

Application figures: the application version and its release-train grading, the database engine version, and the runtime version.
Postgres and Node are per-application even though they sit on the machine, because each application has its own.

The fleet view stays one view.
An operator crosses a machine figure against an application figure the same way they cross two of either, so "which platforms is this Tamanu version running on" is one crossing and not a join the operator has to do in their head.

## Open questions

- [ ] Does `Scope` gain one variant or two — `Machine(id)` alone, or `Machine(id)` and `Cluster(id)` — given reachability files at both?
- [ ] Which way does unreachability reach an application's health — an extra clause in the rollup rule, or the machine's reachability check inherited into the application's check list?
- [ ] The edit form is machine-first, so is the *detail* view too, or does an application keep a page of its own with its machine's facts presented on it?
- [ ] Is there a machine list, or is the fleet still listed as applications?
- [ ] How does Canopy tell a unified push from a split one — a marker field, or the presence of the new machine section?
- [ ] Which check names and which server-wide fields does Canopy's unified split rule treat as machine-subject? Needs to be written down and to match bestool.
- [ ] Is "a machine has at least one application" enforced, or is a temporary zero allowed for the delete-then-recreate case?
- [ ] Does `/servers/self` become a machine-self route returning the machine plus the applications on it?
- [ ] Should a group carry a product in its billing attribution at all? Product is not a group-level fact, and dropping it would remove the "only when members agree" rule rather than working around it. In scope for this card, or its own?
- [ ] What does the status page present once applications and machines are separate?

## Transition

### The identifier

bestool's existing server ID is really the machine ID.

- bestool keeps calling it the server ID for now, and Canopy keeps accepting it as such.
- Canopy introduces a machine ID; bestool moves across to it later.

### Two push formats

Canopy accepts both, and tells them apart on arrival.

- A **unified** push is the format bestool sends today: machine facts and application facts mixed in one `health` set and one server-wide detail.
  Canopy splits it across the machine and the application itself.
- A **split** push is the new format, carrying machine-subject and application-subject material already separated.
  Canopy takes it verbatim and splits nothing.

bestool moves from unified to split over whatever period it takes, one release at a time, and the unified machinery is retired once nothing sends it.

The split rule for unified pushes is knowledge Canopy has to hold — which check names are machine-subject, and which server-wide fields belong to the machine.
It duplicates what bestool will know once it sends split format, which is the price of the transition rather than a design choice, and it goes away with the unified path.

During the transition a check name can file against a machine from one reporter and an application from another, depending on which format that host is sending.
The catalog is keyed by (source, check) fleet-wide while scope is per-filing (see [CHK](../../specs/monitoring/checks.md)), so this costs nothing as long as Canopy's split rule agrees with bestool's.

### Migration

Every existing server becomes one application and one machine, 1:1, so the backfill is mechanical.
`alert_when_down_for` moves to the machine, and with one application per machine at migration time there is nothing to reconcile.
