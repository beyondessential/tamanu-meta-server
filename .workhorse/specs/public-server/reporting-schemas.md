---
id: RPT
---

# Reporting schemas

A deployment's reporting schema is the set of database views its reports read from.
Canopy decides which deployments owe a reporting schema for which version, supplies the database a build needs, holds the artefact a build produces, and grades whether what a deployment is running is the schema it should have.

## Scope

This spec covers the reporting schema as an artefact Canopy tracks per deployment and version: what obliges one to exist, what a build is given, what it reports, and how a deployment's own schema is graded against it.

It does not cover what the schema contains.
Which views exist, what each one selects, and how they are derived from a database are the build's business, and a re-implementation would be free to produce different views from the same inputs.

It does not cover applying a schema to a deployment.
Applying one is a change to a live database, made by whoever is upgrading the deployment, under the deployment's own change control.
Canopy's part is to hold the artefact that should be applied and to say when what is applied is not it.

## Why it exists

A reporting schema is derived from two things that no single actor holds.
Half of it follows from a Tamanu version's database schema and is the same for every deployment on that version.
The other half follows from the deployment's own configuration, and can only be produced from a database carrying that configuration.

Canopy already holds both sides.
It tracks Tamanu's releases and which version each deployment runs (see [APP](../servers/products.md)), it knows which version a deployment is planning to move to (see [UPG](../private-server/upgrade-plans.md)), and it can have a replica of that deployment's data restored and migrated to that version (see [RST](restore-replicas.md)).
Nothing joins them, so the artefact each deployment needs is produced against a database nobody records, on a cadence nobody can see, and a deployment can run for a year on a schema built for a version it has long left.

The gap is visible in the fleet: deployments carry reporting schemas several minor versions behind the release they run, and Canopy has no way to say so.

## Actors

A **schema builder** is first-party infrastructure that produces a reporting schema from a database and publishes the result.
It holds no list of what to build: it asks Canopy what schemas are owed, builds them, and reports back.
It owns the mechanics of the build, and it is the only actor that needs to understand what the schema contains.

An **operator** declares which deployments are covered, and reads the currency of each.

Canopy owns which deployment owes a schema for which version, the database that build is entitled to, the artefact that results, and the grading of what a deployment actually runs.
The builder owns how the schema is produced.
This boundary is the same one the restore path draws, and for the same reason: Canopy is the only actor with fleet-wide knowledge, and the only one that must not need to know how a build works.

Whether the schema builder is also the restore consumer that produced its source database, or a separate actor that asks for one, is a deployment choice and not a contract Canopy holds either way.

## What a build is entitled to

A build requires a database at the version being built for, carrying the deployment's own configuration.

Both halves are load-bearing.
A database at the right version with no configuration in it produces only the version-generic part of the schema, which is the same for every deployment and answers nobody's question.
A database carrying the configuration but sitting at the deployment's current version produces a schema for a version the deployment is leaving.

The configuration a build reads is a deployment's configured surveys, the screens that compose them, and the data elements they collect.
It is reference data, present in every copy of the deployment's database and carrying no patient content.

A build therefore needs no patient data, and a source database whose patient data has been de-identified is sufficient.
Canopy prefers a de-identified source where one can be had, since a build that never sees patient data is a build whose infrastructure need not be trusted with it.
A de-identified source is sufficient only while the product's masking manifest leaves configuration legible: a manifest that masked a survey's identifiers rather than its answers would defeat the build while appearing to succeed.

Canopy does not run the build and does not hold the database.
What it holds is the requirement, and the means to have a conforming database produced: a managed restore replica of the deployment, restored from a recent snapshot, migrated to the version being built for, and de-identified.

## What is owed

Canopy derives what is owed rather than an operator naming each pair of deployment and version.

A deployment owes a reporting schema for its current version, and for the version its group's open plan moves it to.
The current version is what its reports are running against now.
The planned version is what they will run against, and a schema that does not exist by the time the upgrade lands is an outage of every report the deployment has.

Only a published version is buildable, for the same reason it is testable: a version's schema reaches a builder as its published artefacts, and an unpublished version has none to fetch.

Only a deployment an operator has declared as covered is owed anything.
A deployment whose reports are maintained elsewhere, or which has no reporting at all, should not accrue findings for a schema nobody wants.
Coverage is declared per group, is enabled or disabled, and is audited (see [ADM](../private-server/admin-access.md)).

A build is owed once per pair of deployment and version, and is settled by a successful build for that pair.
A pair is reinstated when the version's own artefacts change, since a schema built from a superseded release of the same version is not the schema that version now describes.
Changes to a deployment's configuration do not reinstate a settled pair on their own; the schema a deployment has is the schema it asked for at the time it was built, and refreshing it is an operator's decision.

## The worklist

A schema builder fetches what it is currently owed in one request, scoped to the calling builder.
Canopy returns one entry per unsettled pair:

- the **group** the schema is for, and the **server** whose configuration it is built from;
- the **version** to build for;
- where the source database is to be found, or what the builder must ask for to obtain one;
- whether the source is required to be de-identified.

The worklist carries no credentials.
A builder reconciles the worklist against what it has already produced and converges over time; entries are the latest state rather than a queue to drain.

## What a build reports

A builder reports the outcome of each build back to Canopy.
A report carries:

- the **group**, **server**, and **version** it concerns;
- the **outcome** — built, or failed — and, on failure, a description of what went wrong;
- the **snapshot** the source database was restored from, where the source was a restored replica, joining the schema to the data it was derived from;
- a reference to the **published artefact**, where the build produced one;
- **how much of the deployment's configuration was covered**: the number of configured surveys the schema addresses, and the number it could not;
- when the build was observed.

Coverage is a primary result rather than diagnostic detail.
A build that succeeds while silently skipping half a deployment's surveys produces an artefact that looks complete and leaves reports missing, and the count is what makes that visible without reading the schema.

Reports are retained indefinitely as an audit trail.

## The artefact

A reporting schema is published per version and deployment, alongside the other artefacts a version publishes.
Canopy records where each one is published rather than storing it, as it does for a version's other artefacts, and corroborates a reported artefact against the published artefacts it already holds for that version.

A deployment's artefact for a version supersedes any earlier artefact for the same pair.
Earlier artefacts are retained and remain addressable, because a deployment that has not yet applied the newest one is running an older one and its currency has to be gradeable against something.

## Currency

Canopy grades a deployment's reporting schema as current, behind, or unknown.

A deployment's schema is current when the schema it reports running is the artefact Canopy holds for the version it reports running.
It is behind when the two differ.
It is unknown when the deployment reports no schema at all, which is the state of a deployment whose schema was applied before Canopy tracked any.

The schema a deployment is running is reported by the deployment, alongside the other facts its sources report about it (see [STA](statuses.md)).
Canopy does not read it out of the deployment's database, and treats a deployment that reports no schema as unknown rather than assuming it has none.

Currency is presented per group, so whether a deployment's reports are running against the right schema is answered in one place.

## Alerting

A deployment whose schema is behind raises a reporting-schema check on its central server (see [CHK](../monitoring/checks.md)).
A deployment that owes a build which has not been produced within its bound degrades the same check, because owed-and-unbuilt and built-but-unapplied are both "this deployment's reports do not match the version it runs".

The check is a warning rather than a failure, and does not escalate.
The deployment is serving patients and its reports are returning rows; what is wrong is that some of those rows are computed by a schema written for a different version, and the people who need to know are the ones who maintain the reports, not whoever is on call for outages.

A failed build raises the same check on the affected server, carrying the failure description.
A build failing against a fixed version and a fixed configuration fails the same way every time, so a failure settles the pair rather than leaving it to retry, and a new version or a rebuild is what clears it.

## Out of scope

- What a reporting schema contains, and what each view in it means.
- How a build is run, where it runs, and what it costs to run.
- The report definitions, documentation, and translations a build may produce alongside the schema. Canopy tracks them as published artefacts of the same version and deployment without interpreting them.
- Applying a schema to a deployment's databases, and the change control around doing so.
- Deciding when a deployment upgrades: an owed schema informs that decision without making it.
- Producing a source database. A build's source is a managed restore replica or is not Canopy's concern at all (see [RST](restore-replicas.md)).
