# Split the core model: machines, applications, and identities

Implementation notes for the split. Behaviour lives in the specs: [FLT](../../specs/servers/overview.md), [APP](../../specs/servers/application-types.md), [CHK](../../specs/monitoring/checks.md), [STA](../../specs/public-server/statuses.md), [FIG](../../specs/private-server/figures.md), [DID](../../specs/public-server/machine-identity.md).

## Steps

Each item is a section below; the section carries the detail and the traps.

- [x] **Rename `servers` to `applications`** — storage, the workspace sweep, and the e2e suite
- [x] **Machine grain: table and model** — `machines`, the 1:1 backfill, `applications.machine_id`
- [x] **Extending scope** — `Scope::Machine`, `machine_id` on `issues` and `scoped_check_policies`
- [x] **The machine becomes the operator and enrolment surface** — create/update a machine, enrolment writes `machines.device_id` and `machines.registered_at`, `Application` gains its `machine_id` field, scaffolding default removed. **Unblocks the two below**
- [x] **Group denormalisation** — the trigger propagating a machine's group onto its applications
- [x] **Dropping `device_server_associations`** — rehome the backup-staleness anchor first
- [x] **Ingest: the machine-subject check rule** — what first files at machine scope
- [x] **Ingest: the detail-field split** — `server_reported_detail` splits by grain; carries the figure reads with it
- [x] **Declared names and certificate routing** — identity to machine to application, and the plural entitlement answer
- [ ] **Restore replicas** — declarations move grain; `migration-test` stays application-scoped
- [ ] **Retiring the graded reachability states** — `short_status`'s hardcoded thresholds
- [ ] **Fleet query interface** — MCP gains `Get machine` and `Find machines`
- [ ] **Migration** — `{product, kind}` becomes `{type}`
- [ ] **Frontend** — two detail pages, the group tree, the status-page bands
- [ ] **Routes** — deprecation aliases for every renamed path

Carried deferrals, each gated on a step above rather than on a vague later:

- [x] Remove the `application_default_machine()` scaffolding default — done early, with the operator/enrolment surface
- [x] Add `machine_id` to the `Application` struct — done with the operator/enrolment surface
- [ ] Carry the machine on `IssueData` — with **Fleet query interface** / **Frontend**, whichever presents machine checks first

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

**A regeneration papercut, fixed rather than worked around.** `crates/database/src/schema.rs` as committed disagreed with what `diesel print_schema` generates: it placed `server_group_backup_config.maintenance_role_arn` beside `target_role_arn`, where the column is physically last-but-five (it was added late). `ServerGroupBackupConfig` derives positional `Queryable`, so the file had been massaged to match the struct, and every `diesel migration run` regenerated the truthful order and broke the build until someone patched it back.

Never a runtime problem: diesel emits an explicit column list in `schema.rs` order and never `SELECT *`, so the physical ordinal is irrelevant. The only invariant is `schema.rs` order == struct field order. Proof it was fine: a fixture inserts a config with a NULL `region`, and physical position 5 is `region` while the struct's fifth field was the non-null `maintenance_role_arn: String` — reading positionally by physical order would have failed on the null, and the test passed.

Fixed by moving the struct field to its true position, so `schema.rs` regenerates cleanly and nobody hand-patches it. The field carries a comment saying why it sits away from the other ARN. `source_policies` in `allow_tables_to_appear_in_same_query` was likewise a stale omission and is now simply generated.

### Machine grain: table and model landed

Migration `2026-08-26-124937-0000_add_machines`. The `machines` table, a 1:1 backfill from `applications`, and `applications.machine_id NOT NULL`. Model at `crates/database/src/machines.rs`.

The backfill keeps `machine.id == application.id` for pre-split rows, so the two are trivially correlatable while the split is half-landed; they diverge the moment a second application joins a machine.

**Additive on `applications` by design.** `device_id`, `cloud`, `geolocation` and `registered_at` are copied onto the machine and left in place, so nothing that reads them breaks while the grain is wired up. Dropping them is a later step, once every reader has moved. `group_id` stays for good as the denormalisation the trigger keeps honest.

**`notes` and `tags` are not copied.** An operator wrote them against the thing they were managing, which becomes the application. Duplicating them would mean two copies of one note drifting apart and a policy rule matching a tag twice. A machine starts with neither.

**Two deliberate deferrals, both with a trigger condition rather than a vague "later".**

1. **`applications.machine_id` has a volatile default that creates a machine** (`application_default_machine()`). An application inserted without a machine gets one of its own, which is exactly the 1:1 the backfill performed and exactly what the pre-split model meant. It exists so the column can be `NOT NULL` from the outset instead of nullable-and-tightened-later, which would push `Option<Uuid>` through every reader and invite a bad default at each. **The hazard it carries:** a caller that should attach to an *existing* machine but omits `machine_id` silently gets a second machine rather than an error — wrong for exactly the two-workload host this card exists for. **Remove it in the step that makes reports create applications against a named machine.** Until then no such caller exists.
2. **`Application` does not yet carry a `machine_id` field.** 49 sites construct that struct and nothing reads the column yet, so the field goes in with the step that reads it (machine detail, scope resolution) and those sites get updated once, with purpose.

## Extending scope

The storage pattern already takes another grain and has taken two. Each scope is a nullable FK column, with a CHECK that at most one is set and a partial unique index keying find-or-create for that grain. `issues` (the check-state table) and `scoped_check_policies` both carry `server_id` and `server_group_id`; `incidents` carries `server_group_id` as its target.

Adding the machine grain follows `migrations/2026-06-15-064431-0000_backup_group_scoped_issues` almost line for line:

- `machine_id UUID REFERENCES machines (id) ON DELETE CASCADE ON UPDATE CASCADE` on `issues` and `scoped_check_policies`.
- Widen `issues_scope_at_most_one` to cover three columns.
- `CREATE UNIQUE INDEX issues_machine_source_ref ON issues (machine_id, source, ref) WHERE machine_id IS NOT NULL`.
- `Scope` ends as `{ Application(Uuid), Machine(Uuid), Group(Uuid), Global }` — `Server` renamed to `Application` by the rename step, `Machine` genuinely new, the other two untouched. `from_columns`/`to_columns` take and return the third column.

**Trap.** The global-scope partial unique index is `WHERE server_id IS NULL AND server_group_id IS NULL` (`migrations/2026-07-08-085731-0000_issues_global_scope`). A machine-scoped row has both null, so it falls inside the global index and collides with a canopy-wide issue on the same `(source, ref)`. The migration must add `AND machine_id IS NULL` there and to its counterpart on `scoped_check_policies`, or machine checks silently clash with self-alerts.

Only `Machine` is added. Clusters are K1's to model, and nothing here should presuppose that a cluster will be a scope at all — including a note saying one is coming, which would railroad K1 into a shape convenient to this card. The hazard worth carrying forward is not a design: whoever adds the next grain must remember the global partial index matches on *all* other scope columns being null.

### Done

Migration `2026-08-26-232015-0000_machine_scoped_issues`. `machine_id` on `issues` and `scoped_check_policies`, both scope CHECKs widened to at-most-one-of-three, machine find-or-create indexes, and **both** global partial indexes recreated with `AND machine_id IS NULL`. The trap has a test that files a canopy-wide and a machine-scoped issue on the same `(source, ref)` and asserts two rows.

`Scope` is now `{ Application, Machine, Group, Global }`, with `to_columns`/`from_columns` on the triple. `Scope::Machine` resolves through the machine's group carrying the *machine's* own `is_monitored`, so excusing a box from monitoring does not quiet the applications on it. `raise_machine_event_with_state` is the machine-grain filing path, mirroring the group one; the startup incident sweep batches machines alongside applications.

Policy chains take the machine dimension: `scoped_to`, `chain_for`, `chains_for_scope` and `apply_scoped` all carry it, and `order_chain` treats machine and application scope as equally specific — a filing is at one grain or the other, so the two never share a chain.

**Nothing files at machine scope yet.** The two call sites that grade a push pass `None` for the machine, because separating machine-subject checks out of a unified push is the ingest step's job. The plumbing is in place and unused, which is deliberate: it lands before the thing that needs it rather than alongside it.

**`IssueData` does not carry the machine.** The private API's issue DTO still exposes only `application_id`, so a machine-scoped issue would present with a null application and no indication of which box it belongs to. Nothing files one yet, and the field goes in with the step that presents machine checks (MCP and the detail pages).

## The machine becomes the operator and enrolment surface

A step the plan originally did not name. Scouting the two steps that were meant to come next found both blocked on the same gap: **a machine is a table nobody writes.** `Machine` has `create`, `archive` and readers, no `update`, and no handlers at all under `crates/private-server/src/fns/`. Nothing in Rust ever writes `machines.registered_at` or `machines.device_id` — the only writer of any `registered_at` is `Application::mark_registered`.

What that blocks, concretely:

- **Group denormalisation.** An operator moving a machine between groups is the trigger's only real driver, and that write path does not exist. Worse, the scaffolding default creates the machine *inside* the application's INSERT, before the application row exists and with `group_id` NULL — so a machine-to-application trigger either never fires for the create path or blanks the group the operator just chose.
- **The backup-staleness anchor.** Rehoming it to `machines.registered_at` collapses it to `config_created_at` for every machine, because nothing sets that column. The anchor would silently stop suppressing `backup-never` on newly-onboarded boxes — the exact false alert it exists to prevent.

So this lands first: operator create/update on the machine, enrolment writing the machine's identity and registration, `Application` gaining its `machine_id` field, and the `application_default_machine()` default removed.

### Model half done

`Application.machine_id` is a real field, so every struct-literal insert names its machine and the compiler enforces it. `Machine::update` takes a `MachineUpdate` changeset and **owns the group write**: it propagates the new group onto the applications on the machine, re-evaluates their open issues on an ungrouped-to-grouped transition, and recomputes both groups' cached effective version. That is deliberately a model method rather than a trigger, because a trigger does the column and none of the three consequences. `MachineUpdate` has no `device_id` or `registered_at`: an identity is bound by enrolment, not by editing a form. `Machine::mark_registered` does that, and `COALESCE`s `registered_at` so a re-enrolment does not restart the clock a backup deadline counts from.

The operator create flow now creates the machine first and hangs the application off it, carrying name, group, cloud and geolocation to the machine. Still 1:1 — a second workload on a box arrives by report, not through that form.

Removed `NewServer` and its `From<Application>` conversion: dead since the crate split, in neither OpenAPI spec, and the only thing that would have needed a machine invented for it.

### Complete

`/api/machines` exists — list, get (with the applications on the box), create, update, archive — all admin-gated except the two reads, with `machines` registered as an OpenAPI tag.

Enrolment now marks the machine as well as the application. The application is still marked too, because enrolment is keyed by application until that flow moves to the machine outright.

**The `application_default_machine()` default is gone.** Omitting a machine is now a NOT NULL violation, which is what it should always have been: the hazard was a caller that should attach to an *existing* machine silently getting a second one, wrong for exactly the two-workload host this card serves.

That cost 137 raw-SQL fixture rewrites across 40 files. The uniform shape is a data-modifying CTE, which keeps it a single statement (`sql_query` cannot run two) and preserves bind numbering:

    WITH m AS (INSERT INTO machines (id) VALUES (…) RETURNING id)
    INSERT INTO applications (…, machine_id) VALUES (…, <same id>)

Where the application had an explicit id, the machine reuses it — the same 1:1 the real backfill produced. Where it had none, the machine is minted anonymously and selected from the CTE.

Two things worth knowing about that sweep. Fixture machines carry no group even where their application does; nothing reads `machines.group_id` on those paths, so it is inert, but it is a disagreement a future machine-scoped assertion would trip over. And each fixture application gets its own machine, so no existing test exercises two applications sharing a box — the cases that do are in `machines.rs` and `scope.rs`, written deliberately.

**Traps found while scouting, worth carrying in:**

- A trigger-driven group change fires no Rust side effects. `reevaluate_open_issues_for_server` and `recompute_groups` both hang off `Application::assign_to_group`; a SQL `UPDATE` from a trigger bypasses both, so open issues never get promoted to incidents and a group's cached effective version goes stale. `assign_to_group` has no production callers today, so it is cheap to move — but its semantics have to be relocated, not dropped.
- `ON DELETE SET NULL` on both `applications.group_id` and `machines.group_id` means a hard group delete nulls each independently, at the DB level, without the trigger. They happen to agree; the trigger must not fight that.
- Group archival cascades over `applications.group_id` and leaves machines untouched, so a restored group's machines and applications can disagree about liveness.
- The test surface dwarfs the production surface: **188 raw `INSERT INTO applications` across 69 files** carry no `machine_id`, and none is compiler-checked. They break at runtime the moment the default goes. The 52 `Application { … }` struct literals *are* compiler-checked.

## Group denormalisation

A trigger propagates a machine's group onto its applications, so the denormalisation cannot drift however either is written. Triggers-for-denormalisation is established here — the table being dropped below was itself trigger-maintained off `statuses`.

An application could read its machine's group through the join, but it carries the column anyway so every group query reads one column rather than joining through the machine, and so the trigger has somewhere to write. An application always has a machine (see [FLT](../../specs/servers/overview.md), "Cardinality"), so the column is never the only source of an application's group — it is a denormalisation, and the trigger is what keeps it honest.

### Done

Migration `2026-08-27-050328-0000_applications_take_machine_group`. Two triggers: a machine's group change propagates to the applications on it, and an application's own group write is corrected back to its machine's. Existing rows were brought into agreement, a no-op on anything the 1:1 backfill produced.

The trigger complements `Machine::update` rather than replacing it. The model method does the three things a column write cannot — re-evaluating open issues for anything that gains a group, and recomputing both groups' cached effective version — while the trigger covers every other writer so the denormalisation cannot drift even when the consequences are someone else's job.

**The application update endpoint keeps its contract but changes meaning.** A group change on an application is applied to its *machine*, which propagates back down. Moving "the server" to a group moves the box, which is the model's semantics, and the frontend needs no change.

**The trap: `BEFORE INSERT` was wrong and had to come out.** A data-modifying CTE's rows are not visible to the rest of the same statement, so `WITH m AS (INSERT INTO machines …) INSERT INTO applications …` had the trigger look the machine up, find nothing, and blank the group it was just handed. The foreign key still passed — constraint checks use a later snapshot than the trigger's SELECT — so the failure was silent: a correct-looking insert with a null group. It cost 124 test failures before the cause was clear. The trigger is `UPDATE`-only now, with the reason written into the migration so nobody adds `INSERT` back. Insert-time agreement is the caller's, which the operator flow gets right by creating the machine in its own statement.

Two smaller consequences. Fixture machines now carry their application's group, which closes the disagreement flagged in the previous step. And an empty changeset is no longer an error in `Application::update`: it became reachable when the group moved to the machine, since a group-only edit leaves nothing to write. That turned `update` on a missing server from a 500 into a 404 — the 500 was an accident of diesel refusing the empty changeset before anything checked existence, so the test asserting it was updated rather than worked around.

## Dropping `device_server_associations`

The identity ↔ machine link is a single column on the machine. The association table goes: a many-to-many the model has no use for, unconsulted in months.

Three things read it. Two fall away with it: the lookup at `crates/database/src/servers.rs:621` (fed by a trigger on `statuses`, which goes too) and the merge fix-up at `crates/database/src/devices.rs:395`.

The third has to be rehomed. Backup staleness anchors "never backed up" on `max(min_first_seen, config_created_at)`, where `min_first_seen` is the earliest association (`crates/database/src/backup/staleness.rs:80`). It stops a newly-onboarded box alerting immediately against a backup config that predates it. Dropping the table degrades the anchor to `config_created_at` alone — handled by the code, but it reintroduces that false alert. The anchor moves to **when the machine was enrolled** (see [BKJ](../../specs/jobs/backup.md), the staleness signal).

The machine rather than the application, because the thing being backed up is the box. Anchoring on an application's registration would restart a machine's backup deadline every time a workload was added to it, so a box that has been failing to back up for a month would read as freshly onboarded the moment someone deployed a second application onto it.

That is a correction rather than a substitution: anchoring a backup deadline on when a device was first associated with a server reads as an accident of what was available, and "has this been backed up in time" has nothing to do with certificates.

**Scouted. Four corrections to the three-readers claim above:**

1. **The trigger will outlive the table and break every status push.** `DROP TABLE device_server_associations` succeeds without complaint — a PL/pgSQL body is text, not a parsed dependency, so Postgres neither blocks the drop nor cascades to the function. `upsert_device_server_association` then fires on the next status insert and fails on a missing relation. Drop in order: trigger on `statuses`, then function, then table.
2. **`get_past_associations_for_device` is a vertical slice, not a lookup.** It has a private-server route (`POST /api/devices/get_past_server_associations`), an OpenAPI path, generated TS types, and a rendered "Past server associations" panel on the device detail page. All of it goes.
3. **The e2e suite will not catch getting that wrong.** The device-detail specs mount the panel, and the component renders an error state rather than throwing. There is no console-error or `pageerror` guard in the Playwright fixture, so dropping the table while leaving the endpoint gives a silently 500-ing panel that CI reports green.
4. **`seed.rs` truncates the table by name**, so `just seed` breaks at startup rather than at any reader.

The anchor itself: `min_first_seen` aggregates over every device ever associated with the application, so it is effectively "first status this application ever pushed". Moving to `machines.registered_at` shifts it *earlier* (enrolment precedes first push), which makes the `config_created_at` branch of the `max` win more often. That is the intended direction but it is a behaviour change, not a like-for-like swap — and it is inert until something actually writes `machines.registered_at`.

### Done

Migration `2026-08-27-061117-0000_drop_device_server_associations`, in the order the scouting warned about: trigger, then function, then table. `statuses` is partitioned and the trigger lived on the parent, so one DROP covered every partition and any created later.

The vertical slice went with it: `Application::get_past_associations_for_device`, the `/api/devices/get_past_server_associations` endpoint and its OpenAPI path, the generated TS, and the "Past server associations" panel on the device detail page. The device-merge fix-up and the seeder's truncate entry went too.

**The anchor moved to `machines.registered_at`**, and became a join rather than a second query — the scan already reaches `applications`, so the machine comes with it. Anchored on the box because the box is what gets backed up: anchoring on an application's registration would restart a machine's backup deadline every time a workload was added to it, so a box failing to back up for a month would read as freshly onboarded the moment someone deployed a second application onto it. Both cases have tests.

That is a behaviour change rather than a like-for-like swap. `min_first_seen` was effectively "first status this application ever pushed"; enrolment precedes first push, so the anchor shifts earlier and the `config_created_at` branch of the `max` wins more often. Intended direction, but worth knowing when reading a `backup-never` that fires sooner than it used to.

### The check rule: done

`CheckSubject` in commons-types names the 18 machine-subject checks and decides a check's grain. Whole names, never prefixes, with tests pinning the two cases that make prefixes wrong: `caddy_version` is the box's while `caddy_certs` is the workload's, and `ips` is the box's addresses while `ips_errors` is a Tamanu error stream. Anything unrecognised is the application's — a new check is far likelier to be a product's own than a new fact about the box.

Ingest grades each check at its own grain and files it there, from one unified payload. Two end-to-end tests push both grains at once and assert where each lands.

**All five scouted traps were real and are closed:**

1. `raise_machine_event_with_state` took the source as a parameter. Its group and canopy-wide siblings assume `canopy` because they file conditions canopy determines for itself; a machine's checks come from `alertd`. Recording them under `canopy` would have broken per-source silences, the `check_severities` a push answers with, source staleness, and the rule that a source's push only recovers its own checks. There is a test asserting the reporter's source survives.
2. The debug assertion rejecting non-`canopy` filings outside application scope now admits a machine's.
3. `silenced_health_checks_for_server` covers the machine grain, so an operator's silence on a machine check reaches the agent instead of holding only on canopy's side.
4. `enqueue_incident_reeval` keys on the application, so the machine path evaluates its incident inline instead — which it already did.
5. Recovery bookkeeping is per grain: two previously-active sets. One shared set would make a check that moves grain read as unmentioned on the grain it left, closing and reopening it on every push.

The machine filing path also carries `device_id`, so a machine issue records which reporter filed it.

### The detail-field split: done

Migration `2026-08-28-001655-0000_split_reported_detail_by_grain`. `server_reported_detail` becomes `application_reported_detail`, and `machine_reported_detail` joins it. Existing rows are split in place: the box's fields move to its machine and come out of the application's body, so each fact is stored once.

**Reads did not change, which is what made this tractable.** `for_server` returns an application's own detail merged with its machine's, so every figure consumer sees exactly the view it saw before. The storage is what moved. The hard half the plan warned about — every figure read going through one table, with `osName`, `osVersion`, `munin` and `bestoolVersion` machine-subject but read from the application's row — is answered by merging on read rather than by moving the readers.

`version` stays with the application and has no machine counterpart: a version is what the workload runs. The machine's own agent version (`bestoolVersion`) is a detail field like any other and goes to the box.

The field list lives in `commons_types::subject` beside the check-subject list, since both answer the same question; the module was renamed from `check_subject` to `subject` to say so. The migration repeats the list once, to move the rows that already exist.

**One regression this introduced and fixed.** Merging on read meant `for_server` looked up the application's machine, which made it *error* for an application that no longer exists where it used to return nothing. A deleted application has no detail rather than being an error, so the lookup is optional. Caught by an existing test, which is the argument for having had one.

## Declared names and certificate routing

`server_names` becomes `application_names` and keeps its existing fleet-wide unique index on `name` (`migrations/2026-07-29-093724-0000_server_names_and_certificates`). That index already implements the exclusivity the specs now require, so no constraint changes — a name has been tied to one row since the table was created. What changes is the comment above it, which explains the index as stopping two group members fighting over a name, and the behaviour that was written against a weaker reading of it.

The routing is what actually moves. Today a certificate or address request resolves through the device to its single server, and that server's grants apply. With an identity belonging to a machine rather than to an application, the resolution becomes: authenticate the identity, resolve its machine, then find the application on that machine declaring the requested name (see [CRT](../../specs/public-server/certificates.md), "Identity and authorisation"). The grant and pause checks then read from that application.

**Trap.** The refusal for "no application on this machine declares that name" must be indistinguishable from "no application anywhere declares it". The unique index makes the second case cheap to detect, which is exactly why it is tempting to report it, and reporting it turns the endpoint into a directory of what other machines serve.

The entitlement answer becomes plural. `What an application may act on` is asked by an agent holding a machine identity, so its response carries one entry per application on the machine instead of a single application's grants. That is a breaking shape change on both the standalone endpoint and the status-push response, so it needs the same treatment as the other renamed routes: the old path keeps answering with a single entry for a single-application machine, and the new shape is what a two-application machine gets.

### Declared names and certificate routing: done

Routing moved as planned: `authorise` resolves identity → live machine → the application on that machine declaring the name, and the entitlement answer gained an `applications` array with one entry per workload, the flat fields still answering for the single-application machine every machine is today.

Two things the step turned up that the section above did not anticipate:

- **The operator declaration surface did not exist.** Names were only ever declared as a side effect of an agent registering addresses, so there was nowhere for an operator to say which workload serves what — the thing the whole routing now reads. Added `certificates/declare` and `certificates/release`, with `ApplicationName::declare`/`release` behind them. The operator refusal names the holder; the device-facing one never does, and `ApplicationCertificate::request` maps the former to the latter for exactly that reason.
- **A certificate order left no declaration behind.** Registering an address happened to create the row; ordering a certificate did not, so a single-application machine could hold a certificate for a name nothing declared. `request` now declares the name it orders for, which makes "an order exists only for a declared name" an invariant of the model rather than of one call path.

That invariant is what lets renewal and the expiry alert both filter on `still_declared()`: releasing a name stops Canopy renewing its certificates and stops raising them as running out, since renewing past a release would order for a name another application now serves. What was already issued stays issued and collectable.

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

**Scouted.** The seam is a single `None` — the machine argument `apply_scoped` already takes in the grading loop. Nothing in the codebase does prefix or pattern matching on check names today, so a whole-name set is a clean fit with nothing existing to tempt a prefix rule. The status push resolves its target from the URL path rather than from the authenticating identity; `Machine::get_by_device_id` is the substitution point and is already unused outside tests.

Five things that have to move with it, none of them obvious from the filing seam:

- **`raise_machine_event_with_state` hardcodes `CANOPY_SOURCE`**, matching its group and global siblings. Machine-subject checks arrive from `alertd`, so reusing it as-is would record them under `canopy` and quietly break per-source silences, the `check_severities` response, source staleness, and the rule that a source's push only recovers its own checks. It needs the source as a parameter.
- **A debug assertion rejects the filings this step creates.** `file_check` asserts a filing is application-scoped or from `canopy`; a machine-scope `alertd` filing trips it in debug builds.
- **Silences do not know about machines.** `silenced_health_checks_for_server` filters application and group only, even though `scoped_check_policies.machine_id` exists and the policy chain already takes a machine. The `check_severities` block of the push response is built from it, so a machine-check silence would be invisible to the reporting agent.
- **`enqueue_incident_reeval` keys on `application_id`**, column included, so machine filings have an incident path but no way to queue re-evaluation.
- **Recovery bookkeeping is application-scoped.** `active_refs_with_prefix` filters on `application_id`, so the two grains need independent previously-active sets or a check that changes scope reads as unmentioned on the old one and close-then-reopens.

Also: `server_reported_detail` has no machine column and replaces the whole body per `(server_id, source)`, so stripping machine fields out of an application's push loses them outright — there is no merge to fall back on. And only 8 detail field names appear anywhere in code; the other 23 exist solely in the working doc, seed data and tests, so the split list is written from scratch rather than amended.

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
