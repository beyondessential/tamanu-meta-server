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

- **A group.** A machine belongs to a group as an application does, so a machine-targeted check has an incident target to resolve through (`Scope::resolve_incident_target`, `crates/database/src/issues.rs:834`).
- **Tags, and resolved billing tags.** A machine-subject check is graded by policy rules against its target's tags, so a machine without tags could not be graded the way an application is.
  Both grains carry both, and a check decides which it reads.
  `product` and `kind` stay reserved read-only tags on applications only, since neither is a property of a box.

Billing is why both need billing tags rather than one.
On a VM the cost is incurred by the machine, so its billing tags belong there.
On Kubernetes there is no machine and the cost attaches to the application server, so the reverse holds.

Reported rather than stored as a column:

- The machine's hostname.
  bestool will report it, and it attaches to the machine as sourced data — a figure like any other, not a dedicated field.
  This matters because it is the machine's own name, which is a different thing from an application's `url`, and two applications on one machine have one hostname between them and a `url` each.

### Identity roles

The `server` identity role becomes `machine`, so a role names what the identity is installed on.
Inputs accept the old name as an alias for compatibility, so a fielded agent enrolling as `server` keeps working.

### Monitoring and reachability

Working position: reachability is a machine-level concept only, and applications have nothing called reachability.
Applications carry a monitoring on/off toggle; machines carry one too.
See the reachability findings below — pingtask and the legacy `tamanu` heartbeat complicate this and it is not settled.

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
- [ ] How does an application present while its machine or cluster is unreachable? It has no reachability of its own to fail, so something has to say why nothing is arriving.
- [ ] `alert_when_down_for` moves to the machine, so what does the migration do with a machine whose applications disagreed on the threshold? (Moot at migration, where every machine has one application, but not for a machine that later gains a second.)
- [ ] Is "a machine has at least one application" enforced, or is a temporary zero allowed for the delete-then-recreate case?
- [ ] Does `/servers/self` become a machine-self route returning the machine plus the applications on it?
- [ ] Can a machine's group disagree with its applications' groups, and if so which wins where? A machine in one group hosting an application in another is representable once both carry a group.
- [ ] What does a VM's billing attribution name as its product and stage, given `product` and `rank` stay on applications and a two-workload machine has two of each? The group rule (span products, attribute none — see [APP](../../specs/servers/products.md)) is the nearest precedent.
- [ ] What does the status page present once applications and machines are separate?

## Transition

bestool's existing server ID is really the machine ID.

- bestool keeps calling it the server ID for now, and Canopy keeps accepting it as such.
- Canopy introduces a machine ID; bestool moves across to it later.
- bestool will grow separate application-subject and machine-subject reporting.
- Canopy needs to recognise a bestool that is not yet sending split data, and split the current coalesced push across the two grains itself.
