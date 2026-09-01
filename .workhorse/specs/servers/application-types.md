---
id: APP
---

# Application types

Every application has a type, which says what it is.
The type is the axis that decides which of Canopy's per-application features apply to an application at all.

A type names the software and the role it plays together, so a Tamanu central and a Tamanu facility are two types rather than one type in two configurations.
They are different types because they are different things: a large set of checks exists only on centrals and another only on facilities, which is not how two instances of one thing behave.

The set of types is closed and defined by Canopy, since each type's handling is built in rather than configured.
The types are `tamanu-central`, `tamanu-facility`, `senaite` and `canopy`.
Software whose instances hold no role relative to each other has a single type named for the software alone.

Types are a flat set with no ordering among them.

## Where a type comes from

An application's type is reported, never entered.
The software running on a machine is what an application is, and the report that creates an application is what tells Canopy its type (see [FLT](overview.md), "Applications come from reports").

Canopy adopts a reported type silently.

An application does not change type.
A reporter that reports a different type for an application it was already reporting has, as far as Canopy can tell, stopped reporting one application and started reporting another: the first becomes unreachable and the second is created beside it (see [STA](../public-server/statuses.md), "Identifying an application").
An operator resolves that by archiving whichever of the two is wrong.

A type appears among the reserved read-only tags in the effective tags Canopy returns to a reporter, so an agent can read the classification Canopy holds for the application it reports on (see [STA](../public-server/statuses.md)).

The software the type is an instance of, and the role it plays within that software, are each served as a reserved tag of their own beside the type.
Both are derived from the type rather than stored, so the three always agree.
They are served because a tag key is not part of the API's schema and a consumer reading one cannot tell a withdrawn key from an absent value (see [API](../platform/api-compatibility.md)).

## Capabilities

A type determines how Canopy treats an application's version, and whether the application is eligible for public listing.

Canopy tracks Tamanu's releases, so a Tamanu application's version is graded against the releases Canopy knows.
A Canopy instance reports a version that Canopy does not track as a release train, so its version stands as reported and is graded against nothing.
A SENAITE application has no version at all.

Applications of type `tamanu-central` are eligible for public listing, and no other type is.

Reachability and health monitoring apply to every application whatever its type, since an application's checks are graded by the source that reports them rather than by the software beneath them (see [CHK](../monitoring/checks.md)).
Backups apply to every machine whatever its applications: a machine backs up the types its agent advertises, and which types those are is a property of the agent (see [BAK](../public-server/backup.md)).
Managed restore replicas are eligible by intent and backup type rather than by application type, so a group-scoped declaration expands over the group's members whatever each member runs (see [RST](../public-server/restore-replicas.md)).

Group membership carries no type constraint, so a group's applications stay in one group whatever each of them runs.

An application's type is presented wherever the application is, so it is classified the same way in a listing as on its own page.
The interfaces that filter a listing by rank filter by type on the same footing (see [MCP](../private-server/mcp.md)).

## Versions

Canopy records whatever version a source reports for an application, whatever its type.
What varies by type is what Canopy presents, and how it grades what it presents.

An application whose type has a tracked release train presents its version together with how far behind the latest release it is, the updates available from it, and the known issues affecting it (see [FIG](../private-server/figures.md)).
An application whose type reports a version Canopy does not track presents that version alone, with no distance, no available update, and no known-issue list, there being no release train to measure it against.
An application whose type has no version presents no version at all.

An application whose type has a tracked release train but which has not reported a version presents its version as unknown, because there is a version to learn and Canopy has not learnt it.
An application whose type has no version presents nothing rather than an unknown, because there is nothing to learn.

A group's headline version is the version of its canonical member: its highest-ranked live application, with type breaking a tie in the order `tamanu-central`, then `tamanu-facility`, chosen among only those live applications whose type has a tracked release train.
A group whose live applications all belong to types without a tracked release train has no headline version.

That headline is what an upgrade plan measures a group's current version from, as well as what a group presents (see [UPG](../private-server/upgrade-plans.md)).

The fleet's active-version summary, and the fleet spread and crossings of the application version, cover only applications whose type has a tracked release train.

## Public listing

An application is offered to end-user-facing clients only when its type is eligible for public listing and an operator has given it a public name.
An operator is offered the public name field only for an application whose type makes it eligible.
A public name already set is kept when an application stops being eligible, and takes effect again if the application becomes eligible once more.

## Billing attribution

Each grain's billing labels carry what that grain knows, and nothing inferred from what sits inside it.

An application's labels name the software its type is an instance of, its stage from its own rank, and `billing.deployment` from its group's name.
Cost allocation groups by software rather than by software-in-a-role, so a central and a facility of one deployment attribute to the same product.
That label keeps its spelling because cloud cost allocation reads it, and every device reads its own effective tags.

A machine's labels carry a stage and a group and no type, a box not being a piece of software.
Its stage is the highest rank among the applications on it, so a box shared by a production and a test workload bills as production.
Its group is its own.

A group's labels carry a stage, its own name, and a product only when its live applications all run one software.
A group whose applications span two attributes no product at all, since naming one of several would attribute the group's shared cost to the wrong place.
A group holding a central and a facility names Tamanu, the pair being one software in two roles.

A billing label an operator sets explicitly is honoured as given.
The resources Canopy owns on a group's behalf are the exception: a group's backup storage attributes to Canopy's own backup product whatever labels the group carries, so backup spend is never charged to an application of that group.

Recombining the grains is the agent's work rather than the data model's.
The agent's billing-tags check reads the labels of the machine and of every application on it, and derives from both what the underlying instance should be tagged as.
