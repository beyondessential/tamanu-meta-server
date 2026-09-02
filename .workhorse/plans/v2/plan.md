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
- [x] **Maintenance windows take the machine** — arrived from main mid-split; moved onto the machine grain
- [x] **Restore replicas** — declarations move grain; `migration-test` stays application-scoped
- [x] **Retiring the graded reachability states** — `short_status`'s hardcoded thresholds
- [x] **Fleet query interface** — MCP gains `Get machine` and `Find machines`
- [x] **Migration** — `{product, kind}` becomes `{type}`
- [x] **Frontend** — two detail pages, the group tree, the status-page bands
- [x] **Routes** — deprecation aliases for every renamed path
- [x] **The application type is an open set** — `Other(String)`, no default, no type precedence
- [ ] **Check identity: the namespace** — a check is identified by namespace and name, not name alone. Design settled; the checklist is in that section

Carried deferrals, each gated on a step above rather than on a vague later:

- [x] Remove the `application_default_machine()` scaffolding default — done early, with the operator/enrolment surface
- [x] Add `machine_id` to the `Application` struct — done with the operator/enrolment surface
- [~] **Backup tables take the machine grain** — storage, models, handlers and checks moved; 7 e2e tests in `backups.spec.ts` still point at the old location. See the section below
- [ ] Separate "never reported" from "reported long ago" — `Status::latest_for_servers` looks back seven days for performance, so a target silent for longer returns no row and reads as never reported (grey) rather than unreachable (red). Pre-existing, and sharper now the states are three: the fix is a cheap "has this ever reported" fact rather than widening the scan -- verify that it does remain cheap, if the query is "somewhat more expensive" it will slow every single view that displays a status, and the /status page (which has all of them at once) will slow to a crawl.
- [x] Carry the machine on `IssueData` — done with the fleet query interface, which is what presented machine checks first
- [x] Link a machine's maintenance window to its detail page — done with the machine page; the fleet maintenance view links a machine target rather than rendering it as plain text

## Backups take the machine grain

BAK has said since it was written that a backup is a machine's: a device request resolves identity → machine → group and never reaches the applications on the box, and a box shared by two workloads backs up once. The storage lagged that, keying every backup table on whichever application reported the run.

**Nothing on the wire moved.** A device never names a target — it is resolved from the authenticated identity — so `server_id` on these tables was internal throughout. The whole change is invisible to `bestool-canopy`.

**Five tables**, each column RENAMEd rather than added-and-dropped so it keeps its position: the models load positionally, and a column moved to the end silently misaligns the ones after it. `backup_requests` and `server_backup_capabilities` carry the moved column in their primary key, so two applications on one box collapse to one row — the capability is OR'd and dated from the first advertisement, the request keeps the operator's latest intent. `server_backup_capabilities` becomes `machine_backup_capabilities`.

**What was actually wrong, not just misnamed:**

- `resolve_server` in the public backup handler picked the device's *single live application* — precisely what BAK says must not happen. It resolves the machine now.
- The staleness scan was rooted at applications joined to capabilities, so a box with two workloads would have produced two staleness rows for its one backup, and alerted twice. It is rooted at machines.
- The staleness and reconcile checks filed at `Scope::Application`. They file at `Scope::Machine`, which is what makes a shared box's late backup one finding.
- `latest_success_by_machine_type_for_group` reached the machine by joining applications. That join is gone.

**Outstanding: 7 e2e tests in `backups.spec.ts`.** The backup panel moved off the application page onto the machine's, per FLT ("the application page carries no backups"), and the group page's cross-link plus a few assertions still point at `/servers/{id}#backups`. Rust is green at 1288; typecheck, clippy and biome are clean.

## Terminology: W1's group / environment / instance

W1 (PR #524, open) retires the word "deployment" and settles three terms: a **group** is what Canopy attaches shared state to, an **environment** is a group's members at one rank, and **the Canopy instance** is an installation of Canopy itself. `billing.deployment` keeps its spelling, being read by cloud cost allocation and by every device reading its effective tags. It adds an `AGENTS.md` rule with a `grep -rin deployment` expectation.

This branch has been swept to that vocabulary — the ~24 uses it introduced, not main's ~322, which are W1's to remove. Sweeping ours keeps the branch clean under the new rule without duplicating W1's work or multiplying the conflict between two open PRs.

**We settled the ambiguity W1 could not see.** GRP defines an environment as "a group's servers at one rank", which stops meaning one thing once servers split into machines and applications: `rank` lives only on applications, while `group_id` lives on both. So an environment is a set of *applications*, and a machine's stage is derived as the highest rank among the applications on it — which is what `APP` already says about billing. Written into `FLT` under "Groups", since we are the ones introducing the distinction. The cross-reference to GRP goes in when W1 lands; linking a file that does not exist on this branch would dangle.

The first draft overreached, asserting that a box is not in an environment. It is, in practice — nobody puts a production workload and a demo one on one box. What is true is only that Canopy records no relationship, because nothing it does turns on one. A modelling fact, not a claim about how the fleet is run, and the spec now says the narrower thing.

**Merge order matters.** 57 files are touched by both, three of which this branch renamed (`database/src/servers.rs`, `database/src/server_certificates.rs`, `private-server/src/fns/servers.rs`), plus `specs/servers/products.md` → `application-types.md`. W1 also renames `deployment_default_region()` → `instance_default_region()`, which this branch calls from the restore worklist. V2 landing first is much the cheaper order: replaying a vocabulary sweep over a rename is mechanical, where replaying a rename over a sweep means resolving 57 files of wholesale rewrites and hand-applying four renames.

## The wire may not break under it

**The bar is now a standing rule**, in [API](../../specs/platform/api-compatibility.md) and in `AGENTS.md`: the `bestool-canopy` crate generated from the public OpenAPI spec after a change must not be a semver-breaking change against the one generated before it, unless the break has been coordinated. It came out of this card, but it governs every change, so it lives in the specs rather than dying with this plan.

**It reaches the public spec only.** That is what the agent-facing crate is generated from. The private API is the admin SPA's, versioned with the bundle it ships in — a distinction worth stating, since `product` and `kind` live there and nowhere on the public wire.

**`product` and `kind` never appeared on the public wire at all.** `PublicServer` and `WorklistEntry` carry neither, and no schema named `Product` or `ServerKind` is emitted. So merging the pair into one type, which felt like the risky change, costs the agent-facing wire nothing. The one agent-facing surface that does carry them is the reserved tags, which are map keys rather than schema, so an agent or an operator rule reading `canopy:product` or `canopy:kind` would have broken *silently*. Both stay emitted, derived from the type, alongside the new `canopy:type`.

**Two real breaks, both the same rename.** Comparing the generated spec across the branch, `server_id` → `machine_id` on `VerificationArgs` (a request) and on `WorklistEntry` (a response). Everything else the branch does to the public spec is additive.

Both are fixed by keeping the old name beside the new one. The backfill makes this exact rather than approximate: `INSERT INTO machines … SELECT a.id` gave every pre-split machine its application's id, so for every machine that predates the split the two values are equal. On the response the old name is emitted alongside the new. On the request both are optional and the handler requires exactly one — naming both is refused rather than resolved by preference, since a reporter that disagrees with itself about what it restored has not been understood.

**One residue, deliberate.** `VerificationArgs.server_id` widens from required to optional, which reads as a field type change in the generated crate. It is the only way to let a new client send `machine_id` without also sending a legacy field forever; the alternative is a permanently required `server_id`. Runtime compatibility — the actual goal — is unaffected in either direction.

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

## Maintenance windows take the machine

Maintenance windows landed on main (card unrelated to this one) while the split was in flight, written against `servers` before the rename. Rebasing carried them in; this step moved them onto the grain the split creates.

**The window is over the machine.** Taking a box down to patch it stops everything running on it, so a window naming one application left the others on the same host monitored and alerting through work that was always going to stop them. Naming the machine makes that one declaration with N consequences — the same argument the card makes for reachability.

Migration `2026-08-31-091748-0000_maintenance_windows_take_the_machine` renames `server_id` to `machine_id` and repoints the key, backfilling through `applications.machine_id`. Every pre-split window was over a server that is now an application on exactly one machine, so the backfill is that join.

**Trap, and the reason for `FilingScope`.** A filing now carries two different machine ids, and conflating them silently widens what a window does. `machine_id` scopes machine-written *silences*; `covering_machine` is the box whose window covers the filing, which for an application is the host it runs on. Passing the covering machine to `scoped_to` would have made every machine-scoped silence apply to the workloads on that machine — a real change to silence semantics, arriving as a side effect of a maintenance change. They travel as named fields on `check_policies::FilingScope` rather than as four positional `Option<Uuid>`, which is what the near-miss was.

`Scope::Application` is deliberately not a window target: an application is covered through its machine rather than named, so `fleet_columns` maps it to `None` alongside `Global`.

## Fleet query interface

The MCP surface gains a grain rather than being renamed through (see [MCP](../../specs/private-server/mcp.md)):

- `Get server` splits into `Get application` and `Get machine`, with the fields divided along the same axis as `server_reported_detail` above — version and database engine to the application, platform and hardware to the machine.
- `Find machines` is new, alongside the existing application search.
- `Find issues` gains machine and application filters, and filtering by application returns the machine's issues among the application's own, which means it reads through the same amalgamation the detail page uses rather than a second query path.
- `Get incident` reports each issue's scope, so a client can tell a box's failure from its software's.

These regenerate into `private-web/openapi.json` and `src/api-types.ts` via `just gen-openapi` like any other handler change.

### Fleet query interface: done

`find_machines` and `get_machine` are new, with the figures split along the same axis as the reported detail: platform, hostname, processor count, memory, uptime, filesystems and addresses to the machine; version and database engine to the application. Each side names the other, and the client-visible instructions now open by saying machines and applications are different things and which to ask about what.

An issue's target became one tagged `scope` rather than a row of nullable ids, which is what the spec asks for ("the scope it is filed at and what that scope names"). A client cannot read a machine's failure as unattributed because `application_id` was null.

**Three bugs, not one refactor.** Machine checks had been filing since the ingest step, and nothing read them:

- `Issue::list` excluded them from the fleet issue list outright. Its "not canopy-wide" guard was `application_id IS NOT NULL OR server_group_id IS NOT NULL`, and a machine issue has both null — so every maintenance, restore-verification, redaction and machine-subject finding was invisible.
- The group filter collected only a group's applications, so a group's view dropped its machines' issues too.
- `enrich_issues` resolved names from `application_id` alone, so a machine issue rendered with no name, host, or deployment. `enrich_issue` had its own copy of that logic and now delegates, since the two had already drifted.

`IssueRow` renders a machine's issue by its machine instead of falling through to "(group-wide)". No link yet: the machine detail page arrives with the frontend step.

`machine_health_from_check_state` is a sibling of the application rollup rather than a generalisation — the queries differ only in which column names the target, and both end at `HealthState::from_results`, so the grains cannot classify differently.

## Restore replicas

Declarations move grain: a declaration names a machine within a group, and whole-group declarations expand over machines (see [RST](../../specs/public-server/restore-replicas.md)). The worklist, snapshot authority and credential scoping follow, all of which were already `(server, type)` pairs standing in for what is really a machine.

Migration testing does not follow, and this is the one place in the card where the two grains genuinely interleave: a candidate version is an application's, while the snapshot it is tested against is a machine's. A worklist entry for a `migrate` intent therefore names the application whose candidate it carries alongside the machine whose snapshot it restores.

The checks land accordingly — `restore-verification` and `redaction` at machine scope, `migration-test` at application scope. Getting these the wrong way round is silent: both file successfully and both present, just against the wrong grain.

Card B3 covers the related assumption that a backup type's *name* tells Canopy what it holds. Nothing here depends on that being fixed first.

### Restore replicas: done

Migration `2026-08-31-105219-0000_restore_replicas_take_the_machine`. `restore_replicas.server_id` and `backup_restore_checks.server_id` become `machine_id`, backfilled through `applications.machine_id`; `ReplicaKey`'s first element is now a machine. Declarations, the worklist, and the sweep all expand over machines, so a box running two workloads gets one replica of its one snapshot rather than two of the same backup.

The checks landed as the section above called for: `restore-verification` and `redaction` at machine scope, `migration-test` at application scope. Three things this turned up that were not obvious from the outside:

- **The recovery gate is grain-specific.** `open_server_issue_active` reads `issues.application_id`, so a machine-scoped check asking through it would never find its own open issue and would never file the recovery. Added `open_machine_issue_active` and `machines_with_open_checks`, and `file_restore_check` now picks by the scope it is filing at. Silent if missed: the check files fine and simply never clears.
- **The interleaving needed storing, not deriving.** A migration test is a machine's snapshot plus an application's candidate version. `migration_tests` gained an `application_id` so the pair is recorded rather than re-derived; a worklist entry carries both ids and a report echoes the application back. Without it, a two-workload box with different candidates could not have a verdict attributed.
- **Snapshot authority reaches the machine through a join.** `backup_runs` still records the application that reported a run, so `latest_success_by_machine_type_for_group` joins `applications` to key by machine. A box whose two workloads both report a backup of one type has one latest snapshot, the most recent of them. The join goes away when the backup tables take the machine grain.

**Still application-grain, deliberately:** redaction gaps (a manifest is a product's), `migration_tests` itself, and `VersionKnownIssue`.

## Retiring the graded reachability states

`short_status` (`crates/database/src/statuses.rs:646`) grades quiet on hardcoded 2/10/30 minute thresholds, independent of the per-target `alert_when_down_for` the reachability check uses — two definitions that never agreed, with the unconfigurable one driving the dots. The graded intermediate states go; what survives is reachable, unreachable, and never-reported, on the configurable threshold.

### Retiring the graded reachability states: done

`ShortStatus` loses `Away` and `Blip`; `Up`, `Down` and `Gone` survive as reachable, unreachable and never reported, with the wire values unchanged. `short_status` now takes the target's own down threshold instead of the fixed 2/10/30-minute bands, so the indicator and the `reachability` check are graded on the same clock. They could previously disagree outright: a target configured to five minutes showed a healthy dot while its own reachability check had already failed.

`Application::reachability` holds the threshold lookup so the six call sites cannot drift apart on it, which is how the two definitions came to diverge in the first place.

Two things found on the way:

- **The status legend was already wrong.** It claimed `blip` was "missed 2 checks" and `away` "last seen 2-10m ago", where the code made blip 2-10m and away 10-30m. The labels now name the three states with no durations at all, since each target is judged against its own.
- **`away` and `down` never carried a health signal.** `StatusDot`'s reachable set was `{up, blip}`, so a target in either graded state showed no health outline. Collapsing to `{up}` makes that rule legible rather than incidental.

Specs: CHK gained a line separating the check's three results (how much of what should be reporting still is) from the target's reachability (whether anything is), which reads as a contradiction otherwise. MCP's "recent-activity window" and "not recently seen" were a second, undefined vocabulary for the same idea and now point at the target's own threshold.

## Check identity

Two questions that look alike and are not: what a check *result* is filed against, and what distinguishes one *check* from another. The first is the target — an application, a machine, a group, Canopy — and that is `Scope`, which already exists and is untouched by any of this. The second is the check's identity, which is a namespace and a name.

Reachability is the case that separates them. It is one check, filed at every target: same concept, same rules, no ceiling. Two targets, one identity. Reading the target axis and calling it identity is the mistake to avoid here, and it is the one I made twice.

`version` is the opposite case. A box's version, a Tamanu's version and another product's version are unrelated conditions colliding on a name — different meanings, different rules, so different checks. That is what a namespace is for.

**Namespacing exists because of control over names, not because of targets.** Canopy's own names are curated: we choose every one and can guarantee it means a single thing, so `canopy` needs no namespace and its checks are identified by name alone. Names arriving over the device API are not curated, so they need a namespace to keep unrelated conditions apart.

So a source is one of two things:

- **flat** — a curated namespace where names are unique by construction. `canopy` and `manual`.
- **structured** — namespaced by a subject, which is either the machine or an application of a given type.

The subject is `Machine` or `Application(type)` and nothing else, because those are the only things a device push is ever about. A group check and a Canopy-wide check are always canopy's own, so they live in the flat namespace and never need a subject. There is no `Group` subject and no `Global` subject; the flat case *is* the unscoped case.

Resolving a filed check to its catalog entry therefore needs the namespace, and for a structured source's application check that means the application's type. Canopy's own filings need nothing, which is most of the code the earlier attempts were disturbing.

### Storage: the namespace sits beside the source, and is never encoded into it

`source` keeps meaning the reporter, everywhere it already appears. It is a name a device push carries and an operator recognises, and nine tables key on it for reasons that have nothing to do with check identity — reported detail, backup snapshots, statuses, `source_policies`. Overloading it to carry a namespace would drag all of them into this.

`source_policies` is the clearest case. Reachability and ingest are properties of *the reporter* — whether we hear from it at all, and what we do with what it sends. They are not per-namespace and its key stays `(source)` untouched.

So the namespace is its own columns, on the tables that carry a check's identity and nowhere else:

- `subject TEXT NULL` — `NULL` for flat, `'machine'`, or `'application'`
- `application_type TEXT NULL` — set when and only when `subject = 'application'`

with a CHECK enumerating those three shapes, so a half-populated namespace cannot be written. This is the `Scope::from_columns`/`to_columns` idiom AGENTS.md mandates, applied to a second axis: a Rust enum is the only thing that constructs or reads the pair, and no code hand-matches the columns. The enum wants a name of its own rather than reusing `CheckSubject`, which answers a different question — which half of a unified push a reading belongs to — and conflating the two is how this went wrong twice already.

Two tables get the columns. `check_policies` is the catalog, and `scoped_check_policies` overrides and silences entries in it, so both key on identity. `issues` derives.

`check_policies`'s primary key `(source, check_name)` becomes a single unique index on `(source, subject, application_type, check_name)` declared `NULLS NOT DISTINCT`, so the three namespace shapes are all covered by one index and `NULL` collides with `NULL` the way the key needs it to. The table gains a surrogate `id`, because the key it was primary on no longer identifies a row.

Three partial unique indexes, one per shape, were the plan and are not what shipped. `scoped_check_policies` settled it: its key already varies over four scopes, so per-shape partials there would be twelve indexes rather than four, and `NULLS NOT DISTINCT` is the only tractable way to write it. Once the table next door uses it the "no precedent in the repo" argument is spent, and one index reading the same way on both tables beats three on one and four on the other.

### Deriving the namespace costs nothing on the read paths

An application check's namespace needs that application's type, which a check-state row does not carry. Storing the type on the row was the alternative, and it is not needed.

The catalog gate is not a join today. `live_cataloged_pairs` loads the whole catalog once per call into a `HashSet<(String, String)>` and each row does an in-memory `contains`. Adding the namespace widens that key. Same one load, same one lookup per row — the cost is replaced, not added.

The type is already in hand at the callers. `health_from_check_state` takes `(id, group_id)` pairs the caller built from `ServerInfo`s it had loaded anyway, and `ServerInfo` carries `type`. Widening that tuple adds no query at any of them, and the `group_of` map the loop already builds gains a sibling.

`source_freshness` is the exception and the only real cost: it takes bare ids and scans fleet-wide without the applications loaded, so it needs one extra query for a type map. It runs from `reconcile_liveness`, a periodic job rather than a request path, which is the right place to pay it.

A `check_subjects` table with the namespace behind an id was considered and rejected. The subject of an application check is its type slug, and the type is an open set living on `applications.type`; a subjects table becomes a second answer to "what types exist" that can drift from the first. It buys integrity the CHECK already gives, on a value that is two short columns.

### The migration is a fan-out, and it uses the ingest rule

**Only structured sources fan out.** `canopy` and `manual` are flat, so their entries take `subject = NULL` and are otherwise untouched. That is most of the catalog, and none of it moves.

**The fan-out derives the namespace with `CheckSubject::of`** — the same function `statuses.rs` already uses to split a unified push. Any other rule, however sensible on its own, puts a migrated entry in one namespace and the next report of the same check in another, so day one after the migration is a duplicate catalog. The migration and the ingest path have to read from the same list or they disagree by construction.

So, per entry from a structured source:

- **A machine-subject name is a re-key, not a fan-out.** One entry, `subject = 'machine'`. No multiplication, because the machine namespace has nothing to vary over.
- **An application-subject name becomes one entry per application type that has reported it**, read off existing check-states.

**Ceiling, rules, documentation and the review stamp all carry.** The stamp is the one that looked like a question and is not. Pending review hard-caps a check's effective result at warning, so registering the derived entries as unreviewed would, at the moment of migration, stop every vetted check in the fleet from opening an incident — a failed `disk_free` would go quiet. Beyond that being a bad migration, it misreads what the review was: the operator vetted this name from this source across exactly the fleet the fan-out covers, so splitting that decision into the namespaces it was already in force in preserves it rather than claiming a review nobody did. A namespace that *first reports after* the migration is genuinely new, and registers pending review by the ordinary rule.

**`first_seen` and `last_seen` are re-derived per namespace, not inherited.** `last_seen` drives seven-day decommissioning candidacy, so copying the original's would make a namespace that stopped reporting a year ago look fresh and keep a dead check alive forever. `first_seen` copied would misdate when that namespace appeared. Both are in the check-states the fan-out is already reading.

**Decommissioned entries fan out the same way.** Decommissioning resolves states rather than deleting them, so the namespaces are still derivable and the retirement record survives per namespace. A resurrected check re-registers pending review regardless, so nothing rides on getting its old policy back.

**One case is unresolvable**: an application-subject entry with no check-states left, because the only application reporting it was deleted and cascaded its states away. Its type is not recoverable and there is nothing running to serve. Those entries are dropped, which is what already happens semantically — a later report re-registers the check pending review. What is actually lost is operator-authored documentation for a check nothing is running, and the migration should say how many it dropped rather than doing it silently.

**Saying so means filing an issue, not raising a notice.** A `RAISE NOTICE` was the plan; `migrate.rs` prints nothing a Postgres notice arrives on at any verbosity, so in the deployment path it would land nowhere and only ever be seen by whoever ran the file by hand. The migration raises the notice anyway, for that reader, and also files a canopy-wide `manual` issue naming the dropped entries. That puts it where an operator already looks, and resolving it is how they say they have dealt with it.

**`scoped_check_policies` fans out too, and further than the catalog does.** A row scoped to an application takes that application's type and stays one row. A row scoped to a machine, a group, or Canopy-wide, on an application-subject check, covers a target that spans types, so it becomes one row per type present in that target. That multiplication is real and worth counting before running it.

### The spec's phrasing needs correcting with this

CHK already carries the substance — two types reporting the same name are two entries — but says a check is catalogued "as `<type>.<check>`", which reads as the name being concatenated in storage. It is not: the namespace is its own columns and `<type>.<check>` is how an entry presents. Line 99's "keyed by (source, check)" needs the namespace too. Both land with the implementation so spec and code move together.

### Implementation

Ordered so nothing is half-keyed at any point: the type exists before the columns, the columns and the fan-out land together, and the readers move before the writers stop agreeing with them.

- [x] **A `Namespace` type in `commons-types`, beside `CheckSubject`** — `Flat`, `Machine`, `Application(ApplicationType)`, with `from_columns`/`to_columns` mirroring `Scope`'s, and a `Namespace::of(source, check_name, application_type)` that delegates to `CheckSubject::of`. Its own name and its own doc comment saying what it is *not*: `CheckSubject` answers which half of a push a reading belongs to, `Namespace` answers which catalog entry it resolves to. Conflating them is the mistake this card made twice. `commons-types` already carries diesel derives (`ApplicationType`), so the column mapping can live with it. The reserved-source boundary was already written three separate ways (`public-server/src/statuses.rs`'s own `RESERVED_SOURCES`, two hardcoded `== "canopy" || == "manual"` tests in `private-server/src/fns/healthchecks.rs`, and the `CANOPY_SOURCE`/`MANUAL_SOURCE` constants in `database`), so the type consolidates them onto one `is_reserved` rather than adding a fourth. It is case-insensitive, which the ingest gate needed and the other callers do not notice
- [x] **One migration, sequenced internally** — `subject`/`application_type` on `check_policies` and `scoped_check_policies`; the CHECK enumerating the three shapes; the fan-out; then the `NULLS NOT DISTINCT` identity index on `check_policies` (created last, since the pre-fan-out rows would collide) and the reworked `scoped_check_policies` indexes. Backfill flat sources to `subject = NULL`, machine-subject names to `'machine'`, and application-subject names to one row per type observed in `issues`. Announce the dropped undrivable entries rather than dropping them silently. Two sequencing traps, both found by running it against seeded data: the scoped fan-out has to come *after* the old name-only indexes are dropped or the second type of every row is rejected, and the nil application (`00000000-…`, canopy's own filing target) has to be excluded from both fan-outs or the global scope mints an `application_type` of `canopy`. The migration carries its own frozen copy of the machine-subject list, held to `MACHINE_SUBJECT_CHECKS` by a test beside that list until it ships
- [ ] **`CheckPolicy` keyed by the namespace** — `live_cataloged_pairs` returns `HashSet<(String, Namespace, String)>`; `register`/upsert, `gone_quiet`'s ordering, and `decommission`'s fleet-wide sweep all take it. `decommission` is the one to watch: retiring `disk_free` must not retire another namespace's
- [ ] **Ingest registers with the namespace** — `public-server/src/statuses.rs:546` and `:623` already compute `CheckSubject::of`; they now carry it into the catalog registration alongside the reporting application's type. This is where the migration's rule and the ingest rule are locked to the same function
- [ ] **The catalog gates widen** — 7 `live_cataloged_pairs` loads and 8 membership tests across `issues.rs` (1523, 1624, 1741, 1914, 1952, 3758), `check_policies.rs:886`, `private-server/src/fns/statuses.rs:928`. `health_from_check_state` takes `(id, group_id, type)` triples; every caller already holds the type on its `ServerInfo`, so no caller gains a query. `source_freshness` is the exception and gets one `(id, type)` map query, paid in `reconcile_liveness`
- [ ] **Scoped resolution matches on namespace** — fleet catalog → group → target still applies in order, with each step matching the filing's namespace. A group- or machine-scoped row on an application-subject check now covers one type, not all of them
- [ ] **Private API and SPA** — the catalog list and detail carry the namespace; `/healthchecks/:source/:check` gains a segment, with a redirect from the two-segment form so a bookmarked check page still lands; an entry presents as `<type>.<check>` for an application namespace and the bare name otherwise. Regenerate `private-web/openapi.json` and `api-types.ts`
- [ ] **CHK lines 62–63 and 99** — the namespace as data, `<type>.<check>` as presentation, and the catalog key gaining the namespace
- [ ] **Tests** — two types reporting one name are two entries with independent ceilings; a machine-subject name from a structured source is one entry, not one per type; a silence on one namespace leaves the other's state alone; the fan-out preserves the review stamp while re-deriving `last_seen`; a namespace first reporting after migration registers pending review. Playwright coverage for the catalog and check-detail changes

**The invariant the derivation rests on**, verified rather than assumed: every group- and global-scoped filing in the tree uses `CANOPY_SOURCE` (`backup/staleness.rs`, `jobs/backup/rotation.rs`, `jobs/backup/complete.rs`, `self_alerts.rs`), so a structured-source filing is always application- or machine-scoped and its namespace is always derivable. A structured source filing at group scope would break `issues` deriving, so if one ever appears it has to be caught here.

### The wire keeps the split shape

The push shape stays as the split mockup has it: a source, the machine, and the applications. Every push arriving over the device API is structured, so the API carries no flat source today.

That is a statement about what is needed now, not a structural bar. A fourth key alongside `machine` and `applications` — for a source whose checks belong to no subject — is a coherent extension, and the spec's language must leave room for it rather than asserting that API sources are structured by nature. When something needs to push flat data, that is when its shape gets decided.

### The application type is an open set

The working doc settled this at the start and the implementation drifted from it. "No operator ever fills in an application, its type or its version; those are only reported on a report. A report is the only thing that creates an application." A report carrying a type Canopy has never seen creates an application of that type, and no Canopy release is needed for a new product to appear. A closed set would mean no deployment can push a new kind of application until Canopy ships, which is not a constraint worth having.

Canopy keeps built-in handling for the types it knows — Tamanu splitting into central and facility is why the axis exists at all — and a type it does not know simply has none of it. That is the whole of the special-casing: known types carry capabilities, unknown types carry none.

What follows, each of which the code currently gets wrong:

- **No default type.** There was never a call for one. The type comes from a report or the application does not exist.
- **Version tracking for an unknown type is `Reported`.** Canopy holds no release train for it, so its version stands as reported and is graded against nothing.
- **Types are a flat set with no ordering.** Anywhere types are listed or sorted, they sort alphabetically. An invented precedence is surprising to read and is one more thing to maintain as types appear.
- **A group's headline version names the thing directly**: the application version of the `tamanu-central` on the group's highest-ranked machine. Not a type precedence breaking a rank tie — that is the rule the working doc replaced, and `type_priority` reintroduced it.
- **An application presents as the sentence case of its type**, so `tamanu-central` reads as "Tamanu central", with no per-type label and no special cases. An operator's own name for an application overrides it and is optional.

The set being open is cheap here: the type is not on the public wire at all, so the compatibility bar does not reach it, and the column is already text.

### Where the code diverged from the working doc

Recorded because two of these were *restored* during implementation in response to failing tests, where the test was encoding the rule the working doc had already replaced. The lesson is the general one: a failing test is not the specification, and when one disagrees with the working doc it is the test that is stale.

- [x] `ApplicationType` was a closed four-variant enum whose `FromStr` and `FromSql` rejected anything else, and derived `Default`. It now carries an `Other(String)` for a type Canopy has no handling for, and has no default. That costs `Copy`, which is the bulk of the diff.
- [x] `server_groups::type_priority` ranked types against each other to break a rank tie when picking a group's canonical member. Gone: the headline names the `tamanu-central` on the group's highest-ranked machine, and nothing else. There is no fallback — a deployment has a central, and a group without one has no headline version rather than borrowing a facility's. The first pass at this invented a facility fallback, which is a deployment shape that does not exist.
- [x] `APPLICATION_TYPE_ORDER` gave types a display precedence in the SPA. Gone: rank is an ordered set and types are not, so the tiebreak sorts alphabetically.
- [x] `ApplicationType::label` title-cased and special-cased SENAITE. Now the sentence case of the slug for every type, which is what FLT already said.

Two things follow that were not in the original list. The type catalogue endpoint could no longer be a constant — it returns the types Canopy knows *plus* the ones the fleet is running, so a reported type has a label and capabilities everywhere the SPA presents it. And the wire schema for a type becomes a plain string rather than an enum of four, which is free here because the type is not on the public wire.

Open is not unconstrained: a type is a slug, and a reported value that is not one is a reporting error refused at parse rather than adopted as a new type. That gate is the Rust type every write goes through, not a column constraint.

There is no default at either level. The Rust `Default` derive is gone and so is the column default, which recorded a typeless row as a Tamanu central it was not. Roughly two dozen test seeds were leaning on that default and now say what they are seeding.

The closed set is inherited rather than invented here — `Product` on main is a closed enum and the base spec says "the set is closed and defined by Canopy" — but the working doc had already decided otherwise for this card, so carrying the sentence forward was the error.

## Routes

No route is deleted, and the step turned out to be pure addition: nothing on the public wire had actually been renamed, so there was no old path to alias. The whole diff against the base spec is one added path.

**`/machines/self` is new; `/servers/self` is untouched.** The two answer different questions rather than the same one under two names, which is why they are two handlers rather than one with an alias. `/servers/self` asks which *application* the caller is and 409s on a box running two — the case the machine endpoint exists for. Its response keeps the shape it has always had, so a fielded agent is unaffected; the deprecation is in its description. DID said the two "reach the same answer", which was not true once written, and now says what they each do.

That `server_id` had to keep meaning an application settled it: `POST /status/{server_id}` resolves an application, so an agent that read its id from `/servers/self` and pushes to it would break if the id changed meaning.

**The `server` role is renamed rather than aliased through the code.** `DeviceRole` is not on the public wire at all, so the variant became `Machine` outright, a migration rewrites the stored rows, and `FromStr`/serde accept `server` on input. The alias is on the input only, as planned. Two properties are pinned: a row still written as `server` reads as the machine role and authenticates — every device in the fleet was written that way, so that read is what stops the rename locking them out — and what Canopy stores and presents is `machine`.

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

### The type axis landed; two detail pages have not

Done: the type replaces the pair throughout the UI. One `ApplicationTypeChip` replaces `ServerProductChip` and `ServerKindChip`, since one type carries what two chips carried. `useProducts` becomes `useApplicationTypes`, keyed by type and reading the same catalogue endpoint. The sort helper orders by rank then type, a central still beating a facility on a tie. The edit form shows the type rather than offering it, a type being reported and never entered, and gates the public-name field on the type's own capability.

**The create form became the machine form.** Same form minus five fields: no URL, no product, no kind, no rank, no public name. What is left is a box — name, group, location, tailnet identity, monitoring, notes, tags — and `MachineCreateArgs` widened to match `MachineUpdate` so creating and editing cannot disagree about what a field means. Binding a tailnet node at create time sets `device_id` without setting `registered_at`: naming a box is not the box arriving, and a backup deadline counts from arrival.

**The create form's reachability switch is back**, writing a machine-scoped silence rather than the server-scoped one it used to. The two create-time e2e tests return with it, now asserting on `scoped_check_policies.machine_id`.

### Silences follow the event

The gap was not the one first recorded. `ScopedCheckPolicy::silence` takes a `Scope` and has always written `machine_id`; `silenced_health_checks_for_server` already read machine silences, and so did the consolidated view once it went grain-generic. What was missing was an operator surface — and, underneath it, a live defect.

**The defect.** `re_evaluate_incident_membership` resolved a silence by passing `issue.application_id.unwrap_or(Uuid::nil())`, so a machine-scoped issue consulted only its group. A machine silence therefore quieted the check in the consolidated view and in what the agent was told to run, while the same check still opened an incident. Three readers, two answers. `is_silenced` now takes the issue's own `Scope`, so the grain it was filed at is the grain it is silenced at.

**The invariant is now in the spec** (CHK, "Silences follow the event"): a check that can be filed at a scope can be silenced at that scope; the scopes are the ones it applies at, its own target and that target's group; every reader of a silence must agree; and silencing everywhere is the catalog ceiling rather than a silence, so no scope above the group is offered.

`MachineSilencedRef` mirrors its two siblings, `silence_machine` / `unsilence_machine` / `list_for_machine` sit beside theirs, and `ChecksTable` takes a `CheckTarget` discriminated by grain instead of a nullable `serverId` — so the pair of scopes it offers follows from what it is presenting rather than from a special case.

### The machine's own page, and the group tree

`machines.get_detail` assembles what the page needs: the record, the group, the identity, the figures resolved across every source, the machine's own health and checks, the applications on it each with their own dot, and its billing labels. A machine carries no product, a box not being a piece of software, so its labels are a stage and a group.

**Two pieces had to exist first.** Machine reachability was nowhere: `Application::reachability` reads a `statuses` row, and a machine has none. `Machine::reachability` reads when the box last reported against the box's own threshold, so a quiet machine and a quiet workload are two findings rather than one. And `consolidated_checks_latest` was application-only; rather than a second copy that would drift the way `enrich_issue`'s did, it became `consolidated_checks_for(target: Scope, ..)` with two thin wrappers. The grains differ in which column identifies the target and which rollup answers for it; the catalog gate, silence pass, reachability fill-in and ordering are one implementation.

**`ChecksTable` and `HealthIndicator` moved out of `ServerDetail` into a component**, 619 lines, so the two pages share one implementation rather than one copying the other.

**The group now presents machines, and the applications under each.** A machine takes the rank of its highest-ranked application, the same derivation its billing stage uses. A machine carrying nothing appears as awaiting check-in rather than being absent — before this it was invisible, so an operator who had just added a box had nothing to look at.

**A bug found on the way.** `suspended_targets` returns machine ids since maintenance took the machine grain, but the fleet listing still tested them against the *application* id. Every machine that predates the split took its application's id, so the wrong read agreed with the right one on all existing data and only parted company for a machine created since. Fixed, with a test that seeds deliberately unequal ids and fails on the old code.


### The status page, and the dot's second subject

`StatusDot` had two encodings because a server was two things: a fill for reachability and a ring for health. With the machine owning reachability the dot has one subject, so it spends its whole colourway on the application — healthy, warning, failing, never reported — and the ring goes. The unhealthy case stops being a fill-and-ring inversion and becomes a colour.

`MachineEnclosure` is the machine: a pill around that box's dots, neutral, orange when the box's own checks are degraded, red when the box is down. Red on both grains is deliberate — red means down, and which element carries it says what went down. Orange is the enclosure's alone and light green the dot's, so each hue means one thing.

A group card's dots become rank rows of enclosures rather than one flat strip with a triangle at the rank break. The triangle goes: the break is a rule, and the enclosures say which dots share a box, which is what the strip could never say.

`FacilityServerStatus` gained the machine — id, name, its own reachability and health — since the card had no way to group by box. Two batch reads came with it (`Machine::get_many`, `MachineReportedDetail::latest_for_machines`) so a page of cards does not ask per box.

CHK gained "One subject per mark": a mark says one thing about one subject, an enclosure means nothing on its own, and a mark carries no second encoding for a second subject.

**Maintenance postdates the mockup, and the status page had none of it.** Windows arrived from main mid-split, so the mockup's colourway never accounted for them — and the card type carried no maintenance at all, meaning a box being worked on looked exactly like one that was not, on the page an operator watches. `FacilityServerStatus.machine_maintained` comes from the one batch `suspended_targets` read, and the enclosure carries it: a window is declared over a machine or a group and never over an application, so the box is what shows it.

The pill is hatched rather than cut. A mask on the enclosure would clip the dots inside it too, which would say something about the applications, and the window is the box's. The hatch runs the same diagonal as the dot's maintenance cut, so the two read as one idea at either grain. The dot keeps its own cut for the surfaces that draw applications without a box — a sibling strip — where it is the window's consequence for that application rather than a window of its own. The legend follows: maintenance moved from the dot's row to the enclosure's.

**The bands, and two things the mockup had that the first pass did not.** The card is three: name and version, the rank rows, then a status band carrying operators left and the incident right-aligned beside them. The band is omitted when there is neither, so a quiet card is two bands and the eye goes to the ones with a third. `CardContent`'s padding had to go, since bands run edge to edge; the card clips them with `overflow: hidden` and each band carries its own padding.

Going back to the mockup for the band layout caught two deviations in what had already landed. The enclosure was a solid fill where the mockup draws an outline with a wash — the pill is context for the dots inside it, not a competitor to them. And the rank rows had lost the watermark: the mockup spells the rank out behind its own row, faint enough to read only when looked for, which is what replaces the triangle rather than nothing replacing it. Both corrected, and the test case for the watermark restored — I had rewritten it into a weaker one when the watermark went missing.

## Mockups

Under `.workhorse/design/mockups/v2/`:

- `status-page-machine-grouping.html` — banded group cards, rank rows, machine enclosures, the colourway.
- `detail-pages.html` — application and machine pages, the amalgamated check list, the group tree, the pre-enrolment state.
- `push-wire-shapes.html` — unified against split, the discriminator, the split rule.
- `machine-navigation-options.html` — superseded; kept as the record of options considered.

## Adjacent

Card W1 settles deployment/group/rank terminology, which is why the group tables keep their names here. Cards L2 and N1 turn on the same machine/application axis from bestool's side. K1 wants the cluster/identity separation this resolves.
