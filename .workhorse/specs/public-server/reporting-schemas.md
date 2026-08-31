---
id: RPT
---

# Reporting schemas

A reporting schema is the set of database views a group's reports read from.
Canopy decides which groups owe a reporting schema for which version, supplies what a build needs to produce one, holds the artefact that results, and grades whether what a group runs is the schema it should have.

## Why it exists

A reporting schema is derived from two things no single actor holds.
Half of it follows from a Tamanu version's database schema and is the same for every group on that version; the other half follows from the group's own configuration, and can only be produced from a database carrying it.

Canopy already holds both sides.
It tracks Tamanu's releases and which version each server runs (see [APP](../servers/products.md)), it knows which version a group is planning to move to (see [UPG](../private-server/upgrade-plans.md)), and it can have a replica of that group's data restored and migrated to that version (see [RST](restore-replicas.md)).
Nothing joins them, so each group's artefact is produced against a database nobody records, on a cadence nobody can see, and a group can run for a year on a schema built for a version it has long left.

## Actors

A **schema builder** is first-party infrastructure that produces a reporting schema from a database and publishes the result.
It holds no list of what to build: it asks Canopy what schemas are owed, builds them, and reports back.
It is the only actor that needs to understand what the schema contains.

An **operator** declares which groups are covered, and reads the currency of each.

Canopy owns which group owes a schema for which version, the database that build is entitled to, the artefact that results, and the grading of what a group actually runs.
The builder owns how the schema is produced.

Whether the schema builder is also the restore consumer that produced its source database, or a separate actor that asks for one, is an operational choice rather than a contract Canopy holds either way.

## What a build is entitled to

A build requires a database at the version being built for, carrying the group's own configuration.
Neither half stands alone: the right version without the configuration produces only the part of the schema every group on that version shares, and the configuration at the wrong version produces a schema for the version the group is leaving.

The configuration a build reads is a group's configured surveys, the screens that compose them, and the data elements they collect.
It is held centrally and synced down, so a schema is built from a central server, and a group holding more than one owes a schema for each, since each carries its own configuration.
It is reference data, present in every copy of the database and carrying no patient content, so a source whose patient data has been de-identified is sufficient and is what Canopy asks for where one can be had.
That holds only while the product's masking manifest leaves configuration legible: a manifest masking a survey's identifiers rather than its answers would defeat the build while appearing to succeed.

Canopy holds the requirement rather than the database, and already has the means to satisfy it: a managed restore replica, restored from a recent snapshot and migrated to the version being built for, is a database that meets both halves (see [RST](restore-replicas.md)).

A restore that carries a build is its own intent rather than a second purpose bolted onto the one that tests migrations.
The two restore the same snapshot separately, as a verifying intent and a migrating one already do, because their outcomes are independent: a version whose migrations fail against a group's data has no schema to build, and a build that fails says nothing about whether the version is safe to take.
Restoring twice is affordable precisely because neither replica is kept: each exists for one run and is torn down.

Such a replica is held up for the length of the build rather than discarded as soon as it is healthy, since the build reads it after the migrations land.
It is not offered to operators while it stands, being a database at a version its group is not yet running.

## What is owed

Canopy derives what is owed rather than an operator naming each pair of central server and version.

A central server owes a reporting schema for the version its group's open plan moves it to.
A schema that does not exist by the time an upgrade lands is an outage of every report the group has.
The plan is also what makes the build possible, since it is what has a replica restored and migrated to that version at all, so what is owed and what can be produced arrive together.

Nothing is owed for the version a group already runs, because that version was a candidate once and its schema was built then.
The steady state therefore needs no trigger of its own; a group whose current version predates the pipeline has none on file until its next upgrade produces one.

Only a published version is buildable, for the same reason it is testable: a version's schema reaches a builder as its published artefacts, and an unpublished version has none to fetch.

Only a group an operator has declared as covered is owed anything, so a group whose reports are maintained elsewhere accrues no findings for a schema nobody wants.
Coverage is declared per group, expands over that group's central servers, is enabled or disabled, and is audited (see [ADM](../private-server/admin-access.md)).

A build is owed once per pair of central server and version, and is settled by a successful build for that pair.
A pair is reinstated when the version's own artefacts change, since a schema built from a superseded release of the same version is not the schema that version now describes.
A change to a group's configuration does not reinstate a settled pair: the schema a group has is the one it asked for when it was built, and refreshing it is an operator's decision.

## The worklist

A schema builder fetches what it is currently owed in one request, scoped to the calling builder.
Canopy returns one entry per unsettled pair:

- the **group** the schema is for, and the **central server** whose configuration it is built from;
- the **version** to build for;
- where the source database is to be found, or what the builder must ask for to obtain one;
- whether the source is required to be de-identified.

The worklist carries no credentials.
Entries are the latest state rather than a queue to drain, and a builder converges on them over time.

## What a build reports

A builder reports the outcome of each build back to Canopy.
A report carries:

- the **group**, **server**, and **version** it concerns;
- the **outcome** — built, or failed — and, on failure, a description of what went wrong;
- the **snapshot** the source database was restored from, where it was a restored replica, joining the schema to the data it came from;
- a reference to the **published artefact**, where the build produced one;
- **how much of the configuration was covered**: the number of configured surveys the schema addresses, and the number it could not;
- when the build was observed.

Reports are retained indefinitely as an audit trail.

## The artefact

A reporting schema is published as an artefact of the version it was built for, scoped to the group it was built from (see [ART](../platform/artifacts.md)).
A reporting schema always names a group, so two groups on the same version have two of them and neither stands in for the other.

A group's schema for a version supersedes any earlier one for the same pair, and the earlier ones remain addressable, since a group that has not applied the newest is running an older one and its currency has to be gradeable against something.

## Currency

Canopy grades a central server's reporting schema as current, behind, or unknown.
It is current when the schema the server reports running is the artefact Canopy holds for the version it reports running, behind when the two differ, and unknown when the server reports no schema at all.

The schema a server is running is reported by the server as the version it was built for, alongside the other facts its sources report about it (see [STA](statuses.md)).
Canopy does not read it out of the database, so a server that reports nothing is unknown rather than assumed bare.

Currency is presented per group, so whether a group's reports are running against the right schema is answered in one place.

## Alerting

A central server whose schema is behind, or which owes a build nothing has produced, raises a reporting-schema check on itself (see [CHK](../monitoring/checks.md)).
Both leave its reports mismatched to the version it runs, and neither carries a time bound of its own: the plan that made a version a candidate already carries the date it is wanted by.

The check is a warning rather than a failure, and does not escalate: the servers are up and their reports return rows, and a schema written for the wrong version is for whoever maintains the reports rather than whoever is on call.

A failed build raises the same check with its failure description, and settles the pair rather than retrying, since a build against a fixed version and configuration fails the same way every time.

## Out of scope

- What a reporting schema contains, and what each view in it means.
- How a build is run, where it runs, and what it costs.
- The report definitions, documentation, and translations a build may produce alongside the schema. Canopy tracks them as published artefacts of the same version and group without interpreting them.
- Applying a schema to a database, and the change control around doing so.
- Deciding when a group upgrades: an owed schema informs that decision without making it.
