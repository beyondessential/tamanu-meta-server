---
id: RPT
---

# Reporting schemas

A reporting schema is the set of database views a group's reports read from.
Canopy decides which groups owe a reporting schema for which version, supplies what a build needs to produce one, records the artefacts that result, and grades whether what a group runs is the schema it should have.

## Why it exists

A reporting schema is derived from two things no single actor holds.
Half of it follows from a Tamanu version's database schema and is the same for every group on that version; the other half follows from the group's own configuration, and can only be produced from a database carrying it.

Canopy already holds both sides.
It tracks Tamanu's releases and which version each server runs (see [APP](../servers/products.md)), it knows which version a group is planning to move to (see [UPG](../private-server/upgrade-plans.md)), and it can have a replica of that group's data restored and migrated to that version (see [RST](restore-replicas.md)).
Nothing joins them, so each group's artefact is produced against a database nobody records, on a cadence nobody can see, and a group can run for a year on a schema built for a version it has long left.

## Actors

A **schema builder** is a restore consumer that produces a reporting schema from the replica it restores and publishes the result (see [RST](restore-replicas.md)).
It holds no list of what to build: it advertises an intent carrying `build`, is dispatched the replicas that intent is owed, and reports each outcome as that replica's restore report.
It is the only actor that needs to understand what the schema contains.

An **operator** declares which groups are covered, reads the currency of each, and asks for the builds the derivation does not produce on its own.

Canopy owns which group owes a schema for which version, the database that build is entitled to, the artefact that results, and the grading of what a group actually runs.
The builder owns how the schema is produced.

The builder holds the database rather than asking another actor for one, so no database is handed between actors and a build is dispatched, credentialled, and reported over the paths every other replica already uses.
What runs the build inside that consumer is the consumer's own business: the contract is the same whether it builds the schema itself or drives something else that does.

## What a build is entitled to

A build requires a database at the version being built for, carrying the group's own configuration.
Neither half stands alone: the right version without the configuration produces only the part of the schema every group on that version shares, and the configuration at the wrong version produces a schema for the version the group is leaving.

The configuration a build reads is a group's configured surveys, the screens that compose them, the data elements they collect, and the price lists and insurance plans its invoicing reports pivot into columns.
It is held centrally and synced down, so a schema is built from a central server, and a group holding more than one owes a schema for each, since each carries its own configuration.
It is reference data, present in every copy of the database and carrying no patient content, so nothing a build reads is what a de-identified source exists to protect.

A build's source is not de-identified, because the product's masking manifest masks columns the configuration is read through, and a masked visibility flag or reference identifier produces a schema that is wrong in ways a successful build does not show: the configuration it could not read is absent from the result rather than reported as missing.
Redaction protects a replica someone is given, and a build's replica is given to nobody: it serves one run and is torn down.

A build has inputs the database does not hold: the group's own model definitions, and the formatting and language settings its reports are built with.
Those are the builder's to keep and Canopy neither supplies nor records them, so the database a build is entitled to is one necessary input rather than the whole of one.

Canopy holds the requirement rather than the database, and already has the means to satisfy it: a managed restore replica, restored from a recent snapshot and migrated to the version being built for, meets both halves (see [RST](restore-replicas.md)).
The requirement is the version and the configuration rather than the group's data, so a database freshly migrated to that version meets it too where the configuration is supplied to the build on its own, and a build satisfied that way is the same build and settles the same pair.

A restore that carries a build is its own intent rather than a second purpose bolted onto the one that tests migrations.
The two restore the same snapshot separately, as a verifying intent and a migrating one already do, because their outcomes are independent: a version whose migrations fail against a group's data has no schema to build, and a build that fails says nothing about whether the version is safe to take.
Restoring twice is affordable precisely because neither replica is kept: each exists for one run and is torn down.

Such a replica is held up for the length of the build rather than discarded as soon as it is healthy, since the build reads it after the migrations land.
It is not offered to operators while it stands, being a database at a version its group is not yet running.

## What is owed

Canopy derives what is owed rather than an operator naming each pair of central server and version, and an operator may request a pair the derivation does not reach.

A central server owes a reporting schema for the version its group's open plan moves it to.
A schema that does not exist by the time an upgrade lands is an outage of every report the group has.
The plan is also what has a replica restored and migrated to that version at all, so what is owed and what can be produced arrive together.

Nothing is derived for the version a group already runs, because that version was a candidate once and its schema was built then.
The steady state therefore needs no trigger of its own.

Only a published version is buildable, for the same reason it is testable: a version's schema reaches a builder as its published artefacts, and an unpublished version has none to fetch.

Only a group with an enabled build replica declared for it is owed anything, so a group whose reports are maintained elsewhere accrues no findings for a schema nobody wants.
That declaration is what covers a group rather than a coverage of its own beside it: it already names the group, expands over the group's servers, is enabled or disabled, and is audited (see [RST](restore-replicas.md)).
Only a central server draws an entry from it, since a schema is built from the configuration a central server holds.

A build is owed once per pair of central server and version, and is settled by a successful build for that pair.
A pair is reinstated when the version's own artefacts change, since a schema built from a superseded release of the same version is not the schema that version now describes.
A change to a group's configuration does not reinstate a settled pair: the schema a group has is the one it asked for when it was built, and refreshing it is an operator's decision.

An operator may ask for a build of the version a central server currently runs, which owes that pair and is satisfied like any other.
It is what a group whose version predates the pipeline gets its first schema from, what rebuilds one after a configuration change, and what clears a server graded behind with nothing else owed.
The version a request names is one its server already runs, so the replica reaches it with no migrations to apply.

## Dispatch

A build is dispatched as a restore replica rather than through a worklist of its own (see [RST](restore-replicas.md)).
An intent carrying `build` contributes an entry per unsettled pair among the covered groups' central servers, naming:

- the **group** the schema is for, and the **central server** whose configuration it is built from;
- the **version** to build for, which the replica is migrated to before the build reads it.

The snapshot to restore, the repo coordinates, and the intent's parameter values are the ones every replica's entry carries, and credentials are obtained per run as they are for any other restore.
Entries are the latest state rather than a queue to drain, and a builder converges on them over time.

## What a build publishes

A build publishes what it produced as artefacts of the version it built for, scoped to the group it built from (see [ART](../platform/artifacts.md)): the **reporting schema** itself, the **report definitions** that read from it, the **documentation** describing its views, and the **analytics metadata** derived beside them.
Canopy records each and interprets only the schema, which is the one a server applies and the one currency is graded on.

Publishing is part of the build rather than a step taken afterwards: the builder publishes and registers what it produced in the run it reports, so a schema that exists is one a server can already fetch.

An artefact is published for the exact version it was built for and never for a range of versions.
A schema follows from the migrations a version applies, so one built against a patch is not the schema another patch of the same minor describes, and a range would offer the fleet a schema for a version it does not run.

A reporting schema names a group, so two groups on the same version have two of them and neither stands in for the other.
The version half on its own is the exception: a schema built from no group's configuration belongs to the version alone and is published unscoped, with the version's other artefacts rather than derived per group, so it exists for every version whether any group is covered or not.
It is what a group with no schema of its own is offered, and a group's own schema takes precedence over it, carrying that group's configuration as well as the version's shape.

A group-scoped artefact is published into the group's own object storage, under a prefix distinct from that group's backup repo, over a short-lived credential Canopy issues the builder for the run (see [BAK](backup.md)).
A schema derived from a group's configuration therefore rests in that group's storage, and the credential is what confines a build to writing its own group's artefacts.
Canopy records where each artefact is and holds no copy of the file.

A group's schema for a version supersedes any earlier one for the same pair, and the earlier ones remain addressable, since a group that has not applied the newest is running an older one and its currency has to be gradeable against something.
Each carries a digest, so which of them a server has applied is a fact rather than an inference from a version string.

## What a build reports

A build reports as its replica's restore report, which already names the group, the server, the snapshot it restored, and when it was observed.
Beyond those it carries:

- the **version** it was built for;
- the **outcome** — built, or failed — and, on failure, a description of what went wrong;
- a reference to each **artefact** the build published, of which the schema is one;
- **how much of the configuration was covered**: the number of configured surveys, price lists, and insurance plans the schema addresses, and the number it could not.

The restore's health and the build's outcome stay separate signals from the one report, as a migration test's already do: a healthy replica whose build failed reports a healthy restore and a failed build.

Reports are retained indefinitely as an audit trail.

## Applying

A central server applies the newest schema Canopy offers it for the version it runs, with no operator moving a file: it obtains the artefact over its own credential, applies it, and stamps the schema with what it applied.
It applies one when its stamp and the offered artefact differ, and does nothing when they match, so an upgrade that emptied the schema is repaired by the server itself.
A schema is applied by replacing it whole, which cannot proceed under an open report, so a server applies before its reporting connections are serving rather than beneath them.

A server provisions the reporting role and the schema's privileges for itself, so a schema arriving after the server started is readable as soon as it is applied, with no grant to run after it.

A server that cannot obtain or apply one keeps what it has and reports that, which grades it behind or unknown rather than silently current.

## Currency

Canopy grades a central server's reporting schema as current, behind, or unknown.
It is current when the artefact the server reports having applied is the newest Canopy offers it for the version it reports running, behind when it is an earlier artefact or one built for another version, and unknown when the server reports no schema at all.

The schema a server is running is reported by the server as the artefact it applied, alongside the other facts its sources report about it (see [STA](statuses.md)).
Canopy does not read it out of the database, so a server that reports nothing is unknown rather than assumed bare, and a schema applied by hand carries no artefact to report.

Currency is presented per group, so whether a group's reports are running against the right schema is answered in one place.

## Alerting

A central server whose schema is behind, or which owes a build nothing has produced, raises a reporting-schema check on itself (see [CHK](../monitoring/checks.md)).
Both leave its reports mismatched to the version it runs, and neither carries a time bound of its own: the plan that made a version a candidate already carries the date it is wanted by.

The check is a warning rather than a failure, and does not escalate: the servers are up and their reports return rows, and a schema written for the wrong version is for whoever maintains the reports rather than whoever is on call.

A failed build raises the same check with its failure description, and settles the pair rather than retrying, since a build against a fixed version and configuration fails the same way every time.
A settled pair is dispatched again by an operator asking for a build, which is what these checks exist to prompt.

## Out of scope

- What a reporting schema contains, and what each view in it means.
- How a build is run, where it runs, and what it costs.
- What the report definitions, documentation, and analytics metadata a build publishes contain, and what reads them.
- The mechanics of applying a schema to a database, and the change control around doing so.
- Deciding when a group upgrades: an owed schema informs that decision without making it.
