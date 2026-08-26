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
- A machine has one or more applications, always. A machine with none does not exist.
- On Kubernetes an application has no machine at all; machine-less is a first-class case, not a degenerate one.
- A Kubernetes cluster is a separate entity beside machines, not a machine of a special class.

### How a machine comes into being

There is no standalone "create a machine" flow, at least not initially.
Machines are created through creating applications, which keeps the invariant that a machine has applications on it reachable by construction.

- The migration creates one machine per existing server, 1:1.
- The application creation form gains a machine section.
  By default it creates a new machine for that application.
  Unticking that offers a search dropdown to select an existing machine instead, among the machines in that application's group.

A machine ends with its last application, which is what keeps "at least one application" true rather than merely encouraged.
It is archived rather than deleted, as an application is, so it leaves the live fleet without the record going away.
Removing the last application off a machine warns first, saying that the machine goes with it, and that if the box is still alive the new applications should be put on before the old one is removed.
So the delete-then-recreate case is answered by doing it in the other order, rather than by admitting an empty machine to the model.

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
- its type (what `product` was), `kind`, `rank`, `name`, `public_name`, `group_id`, `notes`, `tags`.
- `registered_at`, `deleted_at`, the restore-window fields.

Moves to the machine:

- The identity link (today `device_id`).
- `cloud` and `geolocation`, both operator-set today (`detect_cloud` only seeds `cloud` from an enrollment hint).
- `alert_when_down_for`, since reachability is a machine fact.

Carried by both, rather than moving:

- **Tags, and resolved billing tags.** A machine-subject check is graded by policy rules against its target's tags, so a machine without tags could not be graded the way an application is.
  Both grains carry both, and a check decides which it reads.
  An application's type and kind stay reserved read-only tags on applications only, since neither is a property of a box.

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

A machine's billing attribution carries a stage and a deployment, and no product.

Its **stage** is the highest rank across its applications, on the existing ordering (`ServerRank` derives `Ord` production-first, `crates/commons-types/src/server/rank.rs:30`), so a box shared by a production and a test workload bills as production.
Its **deployment** is the group, which is unambiguous given the rule above.

Only an application knows what it is, so only an application's billing labels name it.
A machine's labels and a group's labels carry no product, because neither a box nor a deployment is one.
Each grain's labels carry what that grain definitely knows and nothing inferred from what sits inside it.

This removes the rule that had a group name a product only when its members agreed.
That rule was working around a field which should not have been at that grain, and the field goes rather than the workaround.

The agent is where the grains are recombined, not the data model.
The billing-tags check reads the labels of the machine and of every application on it, and works out from both what the instance should be tagged as.
Canopy's records stay correct at each grain, and the joining happens where the joined answer is needed.

Reported rather than stored as a column:

- The machine's hostname.
  bestool will report it, and it attaches to the machine as sourced data — a figure like any other, not a dedicated field.
  This matters because it is the machine's own name, which is a different thing from an application's `url`, and two applications on one machine have one hostname between them and a `url` each.

### Navigation

The hierarchy stays Fleet, then groups, then a group's detail; nothing new is added above it.
There is no machines tab, because a machine is not a way into the fleet, only something a group contains.
The ungrouped listing goes, having nothing left to list.

A group's detail lists by rank as it does now, and within a rank by machine, and within a machine its applications.
Machines are visually grouped rather than merely labelled, so a shared box reads as one thing at every level the fleet is browsed.

### Two detail pages

Editing is machine-first: one form per machine, holding a machine section and one section per application on it, so a shared machine is edited where everything sharing it is visible and a change to its fields is visibly a change to all of them.

Reading is two pages, because the two grains hold genuinely different material.

The **application** page is the one an operator lands on, and is close to the server page as it stands.
It presents the application's checks and its machine's checks amalgamated into one list, its own notes, tags and billing labels, and its `url`.
It carries no backups and no identity, both of which belong to the box.

The **machine** page presents the checks that are about the box and not those of the applications on it, the identity, the machine's own notes and tags, and its backups.
It carries no `url`, that being an application's.

Each page ends with the group's whole tree: rank, then machine, then the applications on that machine.
It is the same arrangement the group's own page uses, so an operator learns one shape and reads it everywhere, and the page they are on is marked in place rather than left out.
Any application or machine in the group is one hop away, and the application page links up to its machine at the top.

A machine sits under the rank of its highest-ranked application, the same rule its billing stage follows.
A box shared by a production and a test workload files under production with its test application nested inside it, which is the useful reading: the box is production-grade because something production is on it.

`/servers/{id}` redirects to the application, so every deep link that exists today keeps working and lands on the page that replaced what it pointed at.
An incident that named a server names an application.

The status dot goes from the top of both pages.
The health badge already says how the thing is, at a size the dot was never trying to compete with, and the group's own listing says how everything around it is.

### Operator presence

Operators attribute to machines, never to applications, since `external_users` is a machine check and a person is on a box.

A group's operator count counts people rather than sightings: someone logged into two machines in a group counts once.
Its tooltip names each of them and the machines they are on.

### Status page
That keeps the application as the unit an operator reads while making a shared box visible as one thing, so a machine going down reads as one failure with its applications under it rather than as several unrelated ones.

The group card restructures to carry this.
It becomes narrower and taller, in three bands: the group's name and version, then its applications, then its incident and operator marks.
Each rank is a row within the applications band rather than a run within one wrapping strip, separated by a rule, which retires the hollow triangle that marked a rank break before.
A row carries its rank spelled out, right-aligned behind its applications, so the rank is readable without hovering anything.
Initials were considered and rejected: production, clone, demo, test and dev collide on their first letters, and a two-letter abbreviation stops being an initial without becoming the word.
A machine is drawn as an enclosure around the applications on it, and every machine is enclosed whether it carries one application or several.

Enclosing every machine is what keeps the common case quiet.
An enclosure means nothing by being present, only by what it holds, so an operator reads how many applications are on a box without first checking whether the box is shared.

The enclosure also carries the machine's own state, and the application indicator inside it carries only the application's.
This retires the two-level indicator the status page uses today, where a fill carries reachability and a ring carries health.
That indicator was doing two jobs because a server was two things; once the machine owns reachability, the application indicator has one subject and spends its whole colourway on it.

Severity reads from the colour and subject from the shape.
Red means down on either grain: a red application on a plain machine is one application failing on a healthy box, while a machine that goes red takes every application on it red with it.
A machine that is merely degraded is distinct from an application that is, so the two never compete for the same hue.
Yellow is not part of this palette at all, being spoken for by the striped presentation of a check that could not run.

A warning stays a near neighbour of healthy rather than a step towards down.
It means a check is failing while the application is overall fine, so it reads as a shade of fine and does not compete with a failure for attention.

A machine is reachable or it is not, with a further state for one that has never reported at all.
There are no intermediate degrees of quiet: the graded "recently quiet" and "quiet a while" states are removed.
They ran on fixed thresholds of two, ten and thirty minutes (`short_status`, `crates/database/src/statuses.rs:646`), which is a second definition of reachability alongside the per-machine threshold the reachability check uses, and the two never agreed.
Only the configurable one survives, so the fleet presents reachability on the same terms it alerts on.

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
The split holds for now: one filing, one issue, one incident.
Worth revisiting if a second application-subject reporter ever appears.

### A machine's checks present on its applications

Reachability is the case that raised this, but the rule is general: **every machine check appears on every application on that machine**, marked as a machine check.

Take a server presenting checks A, B and C today, where the split makes A and B application checks and C a machine check.
It still presents A, B and C.
The MCP interface still returns A, B and C.
The only difference is that C is marked as belonging to the machine.

So the machine is absorbed into the application as a shared part, from an operator's point of view.
The health rollup rule in [CHK](../../specs/monitoring/checks.md) does not change: an application's health still derives from the checks contributing to it, and its machine's checks are among them.

There is still one filing per machine check, so incidents are unaffected — one incident from the machine's scope, however many applications present the check.
A silence on a machine check is machine-scoped and quiets it everywhere it appears, which is right: it is one check, seen from several places.

This is the strongest continuity property the split has.
Nothing an operator or an MCP consumer currently sees disappears or moves, so the refactor lands without a relearning cost.

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

The fleet view stays one view, but each figure spreads over the population it belongs to: machine figures over machines, application figures over applications.
Two denominators in one view, because the alternative — everything counted over applications — reports one two-application box as two Ubuntu machines, and a spread whose numbers are wrong is worse than one that needs reading carefully.

An operator crosses a machine figure against an application figure the same way they cross two of either, so "which platforms is this Tamanu version running on" is one crossing and not a join done in the operator's head.

The unit of a crossing follows from cardinality rather than being chosen: an application has at most one machine, so every application has a well-defined value for any machine figure, while a machine has many applications and no single value for an application figure.
So a crossing involving any application figure counts applications, and a crossing of two machine figures counts machines.

A machine-less application has no value for a machine figure and is absent from such a crossing rather than counted as unreported, following the precedent [APP](../../specs/servers/products.md) already sets for a server excluded from the application-version spread.
It is absent from a machine-figure spread automatically, being no part of that population.

## Implementation notes

### Extending scope is a well-worn path

The storage pattern is already set up to take another grain, and has taken two.
Each scope is a nullable FK column, with a CHECK that at most one is set and a partial unique index keying find-or-create for that grain.
`issues` (which is the check-state table) and `scoped_check_policies` both carry `server_id` and `server_group_id`; `incidents` carries `server_group_id` as its target.

Adding a machine grain follows the group migration (`migrations/2026-06-15-064431-0000_backup_group_scoped_issues`) almost line for line:

- `machine_id UUID REFERENCES machines (id) ON DELETE CASCADE ON UPDATE CASCADE` on `issues` and `scoped_check_policies`.
- Widen `issues_scope_at_most_one` to cover three columns.
- `CREATE UNIQUE INDEX issues_machine_source_ref ON issues (machine_id, source, ref) WHERE machine_id IS NOT NULL`.
- `Scope` ends as `{ Application(Uuid), Machine(Uuid), Group(Uuid), Global }` — `Server` renamed to `Application` by the rename step, `Machine` genuinely new, the other two untouched.
  `from_columns`/`to_columns` take and return the third column.

**Trap to avoid.** The global-scope partial unique index is `WHERE server_id IS NULL AND server_group_id IS NULL` (`migrations/2026-07-08-085731-0000_issues_global_scope`).
A machine-scoped row has both of those null, so it would fall inside the global index and collide with a genuine canopy-wide issue on the same `(source, ref)`.
The migration has to add `AND machine_id IS NULL` to that index, and to its counterpart on `scoped_check_policies`, or machine checks will silently clash with self-alerts.

Only `Machine(Uuid)` is added here, and nothing anticipates a cluster.

Clusters are K1's to model, and this card should not presuppose that a cluster will be a scope at all.
Adding a `Cluster` variant now, or even leaving a note that one is coming, railroads K1 into the shape this card happens to find convenient.
K1 may want a scope variant, or something else entirely; it should reach that on its own evidence.

The one thing worth carrying forward is not a design but a hazard: whoever adds the next grain has to remember that the global partial index matches on *all* other scope columns being null.

### Sequencing: rename first, then split

The `servers` → `applications` rename lands before the machine grain, so the interesting work is written against names that already read correctly and the 19 affected tables are touched once rather than twice.

Blast radius: 19 of 55 tables carry a `server_id`, and 103 Rust files reference `Server`.
The API surface regenerates on top of that — `private-web/openapi.json` and `src/api-types.ts` come from the handler annotations via `just gen-openapi`, and `src/types.ts` re-exports them by hand.

**How deep it goes.** `server_backup_capabilities`, `server_reported_detail` and `server_enrollment_tokens` are about the application and become `application_*`.

`server_groups` stays as it is.
Renaming it to `deployments` would be wrong — "deployment" is already contested language, meaning a group in one place and a single rank within a group in another, which is exactly what card W1 exists to settle.
Picking a name here would pre-empt that, so the group tables keep their names and W1 decides.

`device_server_associations` is dropped rather than renamed; see below.

**Trap in the rename.** The wire's `server_id` and the database's `servers.id` stop meaning the same thing.
bestool's `server_id` is the *machine* ID and keeps that meaning through the transition, while `servers.id` becomes `applications.id`.
So a mechanical rename of `server_id` to `application_id` is wrong at exactly the places where it touches the device API, and each of those has to be read rather than swept.

### The machine's group column is trigger-maintained

A trigger recomputes the machine's group from its applications, so the denormalisation cannot drift however the applications are written.
Triggers-for-denormalisation is established here already — the table being dropped below was itself trigger-maintained off `statuses`.

This puts the invariant and its guard in the right order rather than in two places.
The trigger is where "a machine's group is its applications' group" is *true*, and it raises if a write would leave a machine's applications disagreeing.
The application-code refusal to move an application's group off a shared machine then sits in front of that as an operator-facing error, explaining the problem in the operator's terms instead of surfacing a constraint violation — but it is not the only thing standing between the model and an inconsistent row.

### The identity link, and dropping the association table

The identity ↔ machine link is a single column on the machine.
`device_server_associations` goes: it is a many-to-many that the new model has no use for, and it has not been consulted in months.

Three things read it today, and two of them fall away with it — the lookup in `crates/database/src/servers.rs:621` (fed by a trigger on `statuses`, which goes too) and the merge fix-up in `crates/database/src/devices.rs:395`.

The third has to be rehomed.
Backup staleness anchors "never backed up" on `max(min_first_seen, config_created_at)`, where `min_first_seen` is the earliest association for the server (`crates/database/src/backup/staleness.rs:80`).
It is what stops a newly-onboarded server alerting immediately against a backup config that predates it: taking the later of the two starts the grace from when the server actually showed up.

Dropping the table degrades the anchor to `config_created_at` alone, which the code already handles but which reintroduces exactly that false alert.
So the anchor moves to the application's `registered_at`.

This is a correction rather than a substitution.
Anchoring a backup deadline on when a device was first associated with a server is a confusing thing for the system to do, and reads as an accident of what happened to be available rather than a decision — the question "has this been backed up in time" has nothing to do with certificates or associations.
`registered_at` says when the thing started existing as far as Canopy is concerned, which is what the anchor was always reaching for.

### Mockups

Three mockups put the open presentation and wire questions side by side as options, under `.workhorse/design/mockups/v2/`.

- **Status page: banded group cards** — the settled direction, not options.
  Cards become narrower and taller, split into three bands: name and version, the dots, then incident and operator marks, with the status band omitted when there is nothing in it.
  Ranks become rows separated by a rule lighter than the band borders, replacing the hollow triangle.
  Every machine is a pill enclosure, one application or several, so a one-application machine is a single dot in a pill.
  That last part is what makes it work: the enclosure carries no meaning on its own, only its contents, so an operator never has to notice whether a pill is there, only how many dots are inside.
- **Application and machine detail pages** — the two pages side by side, with what sits on each, the amalgamated check list, and the backlinks between them.
  Indicators follow the status page: a dot is an application's health, an enclosure is a machine, and an enclosure appears only where a machine is the subject.
- **Machine navigation** — superseded by the two above, and kept only as the record of the options considered.
  Its flat-listing option is the one worth remembering as rejected: it had nowhere to show machine state, so a dead box read as a coincidence of failing applications.
- **Status push wire shapes** — unified against split, the discriminator, and the unified split rule as a table.

## Open questions

- [ ] Does an application's type ride in its identifier on the wire, or sit beside it as its own field?
- [ ] Do check names carry their application type (all of Tamanu's being `tamanu_*`), or does the wire qualify each check by application and Canopy key on the pair? The first needs no per-application qualification, if the names can bear it.
- [ ] What becomes of `kind` now that product is the application's type? Central, facility and standalone are kinds within a type, so the two are a pair rather than one replacing the other, but the vocabulary needs settling.
- [ ] Does Canopy adopt a reported type or kind silently, or surface the change for an operator to see? Adoption is settled; whether it is announced is not.
- [ ] Confirm the crossing unit: a crossing involving any application figure counts applications, a crossing of two machine figures counts machines. Derived from cardinality rather than chosen, so it should hold, but the view has to label which it is showing.
- [ ] Is `services`, the reported service inventory, a ninth application detail field? bestool's count of 30 is 22 plus 8, which leaves it outside the total.

## Transition

### The identifier

bestool's existing server ID is really the machine ID.

- bestool keeps calling it the server ID for now, and Canopy keeps accepting it as such.
- Canopy introduces a machine ID; bestool moves across to it later.

### Routes are deprecated, never removed

No route is deleted, because fielded clients call the names that exist today and would break.
A renamed route keeps its old path, marked deprecated, and routes internally to the new name, so there is one implementation and two ways in.

`/servers/self` becomes a machine-self route under that rule: it answers with the machine and the applications on it (see [DID](../../specs/public-server/device-identity.md)), reached at both the old path and the new one.

### Two push formats

Canopy accepts both, and tells them apart on arrival.

- A **unified** push is the format bestool sends today: machine facts and application facts mixed in one `health` set and one server-wide detail.
  Canopy splits it across the machine and the application itself.
- A **split** push is the new format, carrying machine-subject and application-subject material already separated.
  Canopy takes it verbatim and splits nothing.

The split format also stops flattening free-form fields into the envelope.
A reporter's own fields sit under a `detail` object, on the machine, on each application, and on each health entry.
Today they are spread across the top level and across each health entry, so a reporter cannot send a field named `source`, `health`, `check` or `result` without colliding with the envelope; a container removes the reserved words entirely, leaving everything inside `detail` as data and everything outside it as structure.

`detail` is the name because it is already Canopy's word for this, in the status contract, the check-state model and the figures.
Policy rules and the fleet spread reach a check's fields as `check.<field>`, which is exactly `health[].detail.<field>`, so the wire and the vocabulary need no translation between them.

Nesting also makes the envelope extensible, which is why this card does not need to answer everything a reporter might one day send.
Because the envelope's keys are a closed set rather than whatever a reporter did not happen to send, a new sibling of `health` and `detail` can be added later without ambiguity and without a format break.
Reported metrics are the case in point: they raise questions about sampling, units, reporter clocks and retention that this card deliberately leaves alone, and the shape can take them when those are answered.

### The unified split rule

Splitting a unified push is knowledge Canopy has to hold: which check names are machine-subject, and which top-level fields belong to the machine.
It duplicates what bestool knows once it sends split format, which is the price of the transition rather than a design choice, and it goes away with the unified path.

bestool's registry splits 45 checks into 18 machine-subject and 27 application-subject.
Application is the default, so Canopy holds only the machine list: an 18-name allowlist rather than a 45-name mapping, and a check absent from it files against the application, as an unknown check always did.

The machine-subject checks are `disk_free`, `inodes`, `btrfs`, `held_captures`, `memory`, `load`, `uptime`, `time_sync`, `external_users`, `ips`, `munin`, `billing_tags`, `tailscale`, `tailscale_config`, `canopy_registration`, `caddy_version`, `caddy_resolvers` and `caddyfile_version`.

The rule matches whole names rather than patterns, and that is load-bearing rather than incidental.
Caddy straddles the split: its version, resolvers and Caddyfile describe the install on the box, while its certificates are what it serves for one application.
`ips` and `ips_errors` share a prefix and nothing else, one being the machine's addresses and the other a Tamanu error stream.
A prefix rule would file both wrongly and silently.

Two entries bear on decisions made elsewhere in this card.

`external_users` is machine-subject, which moves operator presence onto the machine.
The status page reads that check to show who is logged in and currently attributes them to a server.
People are on a box rather than in an application, so the machine is the truer home, but what the operator marks count changes with it.

`billing_tags` is machine-subject, which agrees from bestool's side with both grains carrying billing tags: on a VM the cost is incurred by the box.

The detail fields split 30 ways: 22 machine-subject and 8 application-subject.
Machine takes identity and OS (`hostname`, `osKind`, `osName`, `osVersion`, `kernel`, `arch`, `osTimezone`), hardware and capacity (`cpuCores`, `totalMemoryBytes`, `filesystems`, `uptimeSecs`), virtualisation (`virtualised`, `virtualisation`), addressing (`ipv4`, `ipv6`, `nat64`, `lanIps`, `wanIpv4`, `wanIpv6`), and the agent and platform (`bestoolVersion`, `instanceTags`, `munin`).
The application takes `tamanuVersion`, `tamanuRoot`, `tamanuServerKind`, `nodeVersion`, `canonicalUrl`, `currentSyncTick`, `timezone`, `pgVersion`, and the service inventory.

Two of these agree with decisions this card reached from the other direction.
`instanceTags` is machine-subject, matching the machine carrying billing tags, and `canonicalUrl` is application-subject, matching `host` becoming the application's `url`.

### Product is what an application is

`product` was a poorly-typed application.
It exists because Canopy had one record per box and needed somewhere to say which software that box ran; with applications as records in their own right, the thing a product named *is* the application.

So product is not a separate field an operator maintains alongside the application. An application is a Tamanu, or a SENAITE, or a Canopy, and that is its type.
An operator creating one says which type it is and which kind within that type, and once the machine reports, Canopy learns the type, kind and version from what arrives and keeps its record in step.

This is why the reported detail carries a kind but no product: there was never anything to report beyond what the application already is.

### An application needs a type on the wire

A random identifier alone does not say what an application is, and Canopy cannot treat it as a Tamanu without being told.
Either the identifier carries the type, or the entry carries a type field beside its identifier.

The type is what makes reported material addressable.
A detail field is `timezone` on some application, and an operator writing a policy rule or reading the fleet figures needs to distinguish a Tamanu's timezone from a SENAITE's rather than seeing them merged into one column.

The same holds for checks, which are keyed by `(source, check)` fleet-wide.
Two applications of different types reporting the same check name would collide into one catalog entry today.
Either check names carry their application type, so all of Tamanu's are `tamanu_*`, or the wire qualifies them per application and Canopy keys on the pair.
Names carrying the type would mean the wire needs no per-application qualification at all, which is the simpler shape if the names can bear it.

### The application is the source of truth for what it is

`tamanuServerKind` arriving as reported detail is not a rival to the operator-set kind, and reading it as one gets the model backwards.

An application knows its own kind and product, because they follow from the software it is running.
An operator sets them in Canopy only because the Canopy record is created before the real thing connects, so something has to fill the gap in the meantime.
That is a bootstrap value for the window before first contact, not a standing claim that Canopy knows better than the application does.

Once the application reports, its own answer is the authoritative one and Canopy's record follows it.

[APP](../../specs/servers/products.md) states this more strongly than intended: it says product and kind are "set by an operator rather than reported by the server", which reads as a permanent property of the model rather than a description of a fleet that could not report yet.
The split is the moment to correct that wording, since reporting is exactly what is arriving.

### Two timezones

The detail split turns one figure into two that can disagree.
`osTimezone` is the machine's clock and `timezone` is the application's own setting, and a Tamanu configured for one zone on a box set to another is a real configuration rather than a contradiction.

The figure is the machine's, and is named for what it is: the operating system's timezone.
That is the one an operator reads a log line or a timestamp against, and naming it removes the question of which timezone an unqualified figure meant.

Both remain available as fields, so an operator who wants an application's configured timezone can spread or cross on it like any other reported field.
The figure row leads with the machine's; the application's is there for whoever needs it.

### Figure fallbacks now cross grains

Two figure rules reach for a value that has moved to the other grain, and both still work while an application has at most one machine.
Neither is broken; both are worth stating rather than inheriting, because a reader will otherwise assume a figure is sourced from its own grain.

The platform falls back to the family the database engine gives away when no operating system is reported, so a machine with no OS reads its applications' `pgVersion` to guess.
The runtime version falls back to the runtime named by the reporting identity's connection metadata, which the application reaches through its machine.

During the transition a check name can file against a machine from one reporter and an application from another, depending on which format that host is sending.
The catalog is keyed by (source, check) fleet-wide while scope is per-filing (see [CHK](../../specs/monitoring/checks.md)), so this costs nothing as long as Canopy's split rule agrees with bestool's.

### Migration

Every existing server becomes one application and one machine, 1:1, so the backfill is mechanical.
`alert_when_down_for` moves to the machine, and with one application per machine at migration time there is nothing to reconcile.
