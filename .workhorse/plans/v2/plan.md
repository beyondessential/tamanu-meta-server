# Split the core model: machines, applications, and identities

Implementation notes for the split. Behaviour lives in the specs: [FLT](../../specs/servers/overview.md), [APP](../../specs/servers/application-types.md), [CHK](../../specs/monitoring/checks.md), [STA](../../specs/public-server/statuses.md), [FIG](../../specs/private-server/figures.md), [DID](../../specs/public-server/machine-identity.md).

## Sequencing

**Rename first, then split.** The `servers` → `applications` rename lands before the machine grain, so the machine work is written against names that already read correctly and the affected tables are touched once rather than twice.

Blast radius at time of writing: 19 of 55 tables carry a `server_id`, and 103 Rust files reference `Server`. The API surface regenerates on top of that — `private-web/openapi.json` and `src/api-types.ts` come from the handler annotations via `just gen-openapi`, and `src/types.ts` re-exports them by hand.

How deep the rename goes, and which grain each table lands on:

- `server_certificates` and `server_names` are about the application and become `application_*`.
- `server_backup_capabilities` becomes `machine_backup_capabilities`. Backups are a machine's, not an application's (see [BAK](../../specs/public-server/backup.md)), so its primary key becomes `(machine_id, type)` and participation is toggled per machine.
- `server_enrollment_tokens` becomes `machine_enrollment_tokens`. Enrolment is what admits a box, and the identity it mints holds the machine role (see [DTR](../../specs/private-server/device-trust.md)). `server_enrollment_challenges` follows it.
- `server_reported_detail` **splits rather than renames**, because figures split by grain (see [FIG](../../specs/private-server/figures.md)). Its `version` column is an application's and its platform, hardware and address fields are a machine's, so one table keyed `(server_id, source)` becomes two keyed `(machine_id, source)` and `(application_id, source)`. This is the one rename-step table that is not a rename.
- `server_groups` and its satellites keep their names. Renaming to `deployments` would pre-empt card W1, which exists to settle exactly this contested word.
- `device_server_associations` is dropped rather than renamed (see below).

**Trap.** The wire's `server_id` and the database's `servers.id` stop meaning the same thing. bestool's `server_id` is the *machine* ID and keeps that meaning through the transition, while `servers.id` becomes `applications.id`. A mechanical sweep of `server_id` → `application_id` is wrong at exactly the places touching the device API; each of those has to be read rather than swept.

### Rename step: done

Migration `2026-08-26-101808-0000_rename_servers_to_applications`. `servers` → `applications`, `server_certificates` → `application_certificates`, `server_names` → `application_names`, and the `server_id` column on the tables that stay application-grain (`issues`, `scoped_check_policies`, `incident_reeval_queue`, `version_known_issues`, plus `server_groups.version_server_id`). `Scope::Server` is now `Scope::Application`.

The machine-grain tables were **deliberately left alone**, so each is touched once rather than twice: `statuses`, `server_reported_detail`, `server_backup_capabilities`, `server_enrollment_tokens`, `server_enrollment_challenges`, `backup_*`, `restore_replicas`, `device_server_associations`. Their `server_id` still points at `applications.id`; the FK followed the table rename by itself.

What the rename step deliberately did **not** move, because each belongs to the split:

- **Routes.** Every path is still `/servers/...` on both APIs. Moving them is the split's job, alongside the deprecation aliases.
- **`DeviceRole::Server`.** An identity concept, and the variant name drives the serialised value. Renaming it to `Application` silently changed the wire value from `server` to `application` and broke the enrolment role. It becomes the machine role in the split, with `server` as an input alias (see [DTR](../../specs/private-server/device-trust.md)).
- **Product-facing copy.** The public versions page still reads "Production Server Versions"; `ServerKind`, `ServerRank` and `Product` keep their names and their prose.

**Three traps this step actually hit**, all of them silent rather than loud:

1. **A PL/pgSQL body does not follow a column rename.** Views and CHECK expressions store parsed references and follow it; a function body is text and does not. `update_server_group_effective_version` reads `version_application_id`, so the migration restates it. Missed, every status push fails at the trigger. Worth re-checking on the split: `upsert_device_server_association` is the other trigger function, untouched here only because both columns it names are unrenamed.
2. **Raw SQL in tests fails soft.** `fetch_issue` ends `.ok()`, so a query naming a column that no longer exists returns `None` and reads as "nothing filed" rather than as an error. Renaming a column means sweeping SQL string literals, not just the diesel DSL — and a literal that joins a renamed table to a kept one has to be split by hand.
3. **A blanket per-file sweep is wrong wherever one file spans both grains.** `stability.rs` queries `issues` *and* `statuses`; sweeping the file rewrote `statuses.server_id`, which the schema still has. The compiler cannot catch this — it is inside a SQL string.

**Pre-existing drift, not this card's to fix.** `diesel migration run` regenerates `crates/database/src/schema.rs`, and a fresh migrate puts `server_group_backup_config.maintenance_role_arn` at ordinal 12 where the committed schema has it at 5 (and adds `source_policies` to `allow_tables_to_appear_in_same_query`). `ServerGroupBackupConfig` derives positional `Queryable`, so taking the regenerated order fails to compile. Both were restored to the committed values to keep this diff purely the rename. **Anyone regenerating the schema has to restore them again**, and the underlying disagreement is worth its own card.

## Extending scope

The storage pattern already takes another grain and has taken two. Each scope is a nullable FK column, with a CHECK that at most one is set and a partial unique index keying find-or-create for that grain. `issues` (the check-state table) and `scoped_check_policies` both carry `server_id` and `server_group_id`; `incidents` carries `server_group_id` as its target.

Adding the machine grain follows `migrations/2026-06-15-064431-0000_backup_group_scoped_issues` almost line for line:

- `machine_id UUID REFERENCES machines (id) ON DELETE CASCADE ON UPDATE CASCADE` on `issues` and `scoped_check_policies`.
- Widen `issues_scope_at_most_one` to cover three columns.
- `CREATE UNIQUE INDEX issues_machine_source_ref ON issues (machine_id, source, ref) WHERE machine_id IS NOT NULL`.
- `Scope` ends as `{ Application(Uuid), Machine(Uuid), Group(Uuid), Global }` — `Server` renamed to `Application` by the rename step, `Machine` genuinely new, the other two untouched. `from_columns`/`to_columns` take and return the third column.

**Trap.** The global-scope partial unique index is `WHERE server_id IS NULL AND server_group_id IS NULL` (`migrations/2026-07-08-085731-0000_issues_global_scope`). A machine-scoped row has both null, so it falls inside the global index and collides with a canopy-wide issue on the same `(source, ref)`. The migration must add `AND machine_id IS NULL` there and to its counterpart on `scoped_check_policies`, or machine checks silently clash with self-alerts.

Only `Machine` is added. Clusters are K1's to model, and nothing here should presuppose that a cluster will be a scope at all — including a note saying one is coming, which would railroad K1 into a shape convenient to this card. The hazard worth carrying forward is not a design: whoever adds the next grain must remember the global partial index matches on *all* other scope columns being null.

## Group denormalisation

A trigger propagates a machine's group onto its applications, so the denormalisation cannot drift however either is written. Triggers-for-denormalisation is established here — the table being dropped below was itself trigger-maintained off `statuses`.

An application could read its machine's group through the join, but it carries the column anyway so every group query reads one column rather than joining through the machine, and so the trigger has somewhere to write. An application always has a machine (see [FLT](../../specs/servers/overview.md), "Cardinality"), so the column is never the only source of an application's group — it is a denormalisation, and the trigger is what keeps it honest.

## Dropping `device_server_associations`

The identity ↔ machine link is a single column on the machine. The association table goes: a many-to-many the model has no use for, unconsulted in months.

Three things read it. Two fall away with it: the lookup at `crates/database/src/servers.rs:621` (fed by a trigger on `statuses`, which goes too) and the merge fix-up at `crates/database/src/devices.rs:395`.

The third has to be rehomed. Backup staleness anchors "never backed up" on `max(min_first_seen, config_created_at)`, where `min_first_seen` is the earliest association (`crates/database/src/backup/staleness.rs:80`). It stops a newly-onboarded box alerting immediately against a backup config that predates it. Dropping the table degrades the anchor to `config_created_at` alone — handled by the code, but it reintroduces that false alert. The anchor moves to **when the machine was enrolled** (see [BKJ](../../specs/jobs/backup.md), the staleness signal).

The machine rather than the application, because the thing being backed up is the box. Anchoring on an application's registration would restart a machine's backup deadline every time a workload was added to it, so a box that has been failing to back up for a month would read as freshly onboarded the moment someone deployed a second application onto it.

That is a correction rather than a substitution: anchoring a backup deadline on when a device was first associated with a server reads as an accident of what was available, and "has this been backed up in time" has nothing to do with certificates.

## Declared names and certificate routing

`server_names` becomes `application_names` and keeps its existing fleet-wide unique index on `name` (`migrations/2026-07-29-093724-0000_server_names_and_certificates`). That index already implements the exclusivity the specs now require, so no constraint changes — a name has been tied to one row since the table was created. What changes is the comment above it, which explains the index as stopping two group members fighting over a name, and the behaviour that was written against a weaker reading of it.

The routing is what actually moves. Today a certificate or address request resolves through the device to its single server, and that server's grants apply. With an identity belonging to a machine rather than to an application, the resolution becomes: authenticate the identity, resolve its machine, then find the application on that machine declaring the requested name (see [CRT](../../specs/public-server/certificates.md), "Identity and authorisation"). The grant and pause checks then read from that application.

**Trap.** The refusal for "no application on this machine declares that name" must be indistinguishable from "no application anywhere declares it". The unique index makes the second case cheap to detect, which is exactly why it is tempting to report it, and reporting it turns the endpoint into a directory of what other machines serve.

The entitlement answer becomes plural. `What an application may act on` is asked by an agent holding a machine identity, so its response carries one entry per application on the machine instead of a single application's grants. That is a breaking shape change on both the standalone endpoint and the status-push response, so it needs the same treatment as the other renamed routes: the old path keeps answering with a single entry for a single-application machine, and the new shape is what a two-application machine gets.

## Fleet query interface

The MCP surface gains a grain rather than being renamed through (see [MCP](../../specs/private-server/mcp.md)):

- `Get server` splits into `Get application` and `Get machine`, with the fields divided along the same axis as `server_reported_detail` above — version and database engine to the application, platform and hardware to the machine.
- `Find machines` is new, alongside the existing application search.
- `Find issues` gains machine and application filters, and filtering by application returns the machine's issues among the application's own, which means it reads through the same amalgamation the detail page uses rather than a second query path.
- `Get incident` reports each issue's scope, so a client can tell a box's failure from its software's.

These regenerate into `private-web/openapi.json` and `src/api-types.ts` via `just gen-openapi` like any other handler change.

## Restore replicas

Declarations move grain: a declaration names a machine within a group, and whole-group declarations expand over machines (see [RST](../../specs/public-server/restore-replicas.md)). The worklist, snapshot authority and credential scoping follow, all of which were already `(server, type)` pairs standing in for what is really a machine.

Migration testing does not follow, and this is the one place in the card where the two grains genuinely interleave: a candidate version is an application's, while the snapshot it is tested against is a machine's. A worklist entry for a `migrate` intent therefore names the application whose candidate it carries alongside the machine whose snapshot it restores.

The checks land accordingly — `restore-verification` and `redaction` at machine scope, `migration-test` at application scope. Getting these the wrong way round is silent: both file successfully and both present, just against the wrong grain.

Card B3 covers the related assumption that a backup type's *name* tells Canopy what it holds. Nothing here depends on that being fixed first.

## Retiring the graded reachability states

`short_status` (`crates/database/src/statuses.rs:646`) grades quiet on hardcoded 2/10/30 minute thresholds, independent of the per-target `alert_when_down_for` the reachability check uses — two definitions that never agreed, with the unconfigurable one driving the dots. The graded intermediate states go; what survives is reachable, unreachable, and never-reported, on the configurable threshold.

## Routes

No route is deleted; fielded clients call the names that exist today. A renamed route keeps its old path, marked deprecated, and routes internally to the new name, so there is one implementation and two ways in. `/servers/{id}` redirects to the application it became, and `/servers/self` keeps answering alongside `/machines/self` (see [DID](../../specs/public-server/machine-identity.md)).

The `server` role name gets the same treatment: enrolment inputs accept it as an alias for the machine role, so an agent deployed before the rename keeps enrolling (see [DTR](../../specs/private-server/device-trust.md)). The alias is on the input only — what Canopy stores and presents is the machine role, so the two do not both need carrying through the code.

## Ingest

Canopy holds the machine-subject allowlist only while unified pushes exist. From bestool's registry: 18 of 45 checks are machine-subject — `disk_free`, `inodes`, `btrfs`, `held_captures`, `memory`, `load`, `uptime`, `time_sync`, `external_users`, `ips`, `munin`, `billing_tags`, `tailscale`, `tailscale_config`, `canopy_registration`, `caddy_version`, `caddy_resolvers`, `caddyfile_version` — and application is the default.

The rule matches whole names, not prefixes, and that is load-bearing. Caddy straddles the split (`caddy_certs` is an application's while the rest describe the install); `ips` and `ips_errors` share a prefix and nothing else, one being addresses and the other a Tamanu error stream. A prefix rule files both wrongly and silently.

Detail fields split 23 machine / 8 application. Machine takes `hostname`, `osKind`, `osName`, `osVersion`, `kernel`, `arch`, `osTimezone`, `cpuCores`, `totalMemoryBytes`, `filesystems`, `uptimeSecs`, `virtualised`, `virtualisation`, `ipv4`, `ipv6`, `nat64`, `lanIps`, `wanIpv4`, `wanIpv6`, `bestoolVersion`, `instanceTags`, `munin`, `services`. Application takes `tamanuVersion`, `tamanuRoot`, `tamanuServerKind`, `nodeVersion`, `canonicalUrl`, `currentSyncTick`, `timezone`, `pgVersion`.

During the transition a check name files against a machine from one reporter and an application from another, depending on which format that host sends. The catalog is keyed fleet-wide while scope is per-filing, so this costs nothing as long as the split rule agrees with bestool's — which is the one part of this card that cannot be verified from inside this repo.

## Migration

Every existing server becomes one application and one machine, 1:1. `alert_when_down_for`, the group and the identity link move to the machine. With one application per machine there is nothing to reconcile.

`{product: tamanu, kind: central}` becomes `{type: tamanu-central}`; kind disappears with the mapping.

The migrated applications are the one population that was operator-entered rather than reported, since they predate the model. Reporting corrects them as each machine's pushes arrive — the same path any application takes, starting from a value rather than from nothing.

## Frontend

Two detail pages replace one, and the group tree (rank → machine → applications) is the same component on the group page and at the foot of both detail pages. The status page's group card restructures into three bands with machine enclosures.

`ServerKindChip` goes. `ServerProductChip` becomes an application-type chip. `StatusDot` loses its two-level encoding: fill carries the application's health alone, and the machine enclosure carries the machine's state.

Playwright coverage goes in with each UI change, per the repo's frontend rules.

## Mockups

Under `.workhorse/design/mockups/v2/`:

- `status-page-machine-grouping.html` — banded group cards, rank rows, machine enclosures, the colourway.
- `detail-pages.html` — application and machine pages, the amalgamated check list, the group tree, the pre-enrolment state.
- `push-wire-shapes.html` — unified against split, the discriminator, the split rule.
- `machine-navigation-options.html` — superseded; kept as the record of options considered.

## Adjacent

Card W1 settles deployment/group/rank terminology, which is why the group tables keep their names here. Cards L2 and N1 turn on the same machine/application axis from bestool's side. K1 wants the cluster/identity separation this resolves.
