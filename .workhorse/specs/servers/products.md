---
id: APP
---

# Server products

Canopy monitors servers running more than one application.
A server's product names the application it runs, and its kind names the server's role within that product's topology.
The product is the axis that decides which of Canopy's per-server features apply to a server at all.

## Product and kind

Every server has a product and a kind, both set by an operator rather than reported by the server.
The products are Tamanu, SENAITE, and Canopy itself, and the set is closed and defined by Canopy, since each product's handling is built in rather than configured.
A server's product is Tamanu unless an operator classifies it otherwise.

A kind is a server's role relative to the other servers of its product.
A Tamanu server is a central server, which facility servers sync to, or a facility server, an on-site instance that syncs to a central server.
A product whose servers hold no role relative to each other has standalone servers, so SENAITE servers and Canopy instances are standalone.
Standalone is a single kind that several products offer, rather than a separate kind per product.

An operator sets both when creating a server and can change either afterwards, and the kinds offered are those the chosen product defines.
Changing a server's product to one that does not define that server's current kind moves the server to the new product's default kind along with it.

Group membership carries no product constraint, so a deployment's servers stay in one group whatever application each of them runs.

A server's product is presented wherever its kind is, so a server is classified the same way in a listing as on its own page.
The interfaces that filter a server listing by kind or rank filter by product on the same footing (see [MCP](../private-server/mcp.md)).
Product and kind both appear among the reserved read-only tags in the effective tags Canopy returns to a server's sources (see [STA](../public-server/statuses.md)), so an agent can read the classification Canopy holds for the server it reports on.

## Capabilities

A product determines how Canopy treats its servers' application versions, and whether its servers are eligible for public listing.

Canopy tracks Tamanu's releases, so a Tamanu server's version is graded against the releases Canopy knows.
A Canopy instance reports a version that Canopy does not track as a release train, so its version stands as reported and is graded against nothing.
A SENAITE server has no application version at all.

Tamanu servers are eligible for public listing, and SENAITE servers and Canopy instances are not.

Reachability and health monitoring apply to every server whatever its product, since a server's checks are graded by the source that reports them rather than by the application beneath them (see [CHK](../monitoring/checks.md)).
Backups likewise apply to every product: a server backs up the types its agent advertises, and which types those are is a property of the agent rather than of the product (see [BAK](../public-server/backup.md)).
Managed restore replicas are eligible by intent and backup type rather than by product, so a group-scoped declaration expands over the group's members whatever each member's product (see [RST](../public-server/restore-replicas.md)).

## Versions

Canopy records whatever application version a source reports for a server, whatever that server's product.
What varies by product is what Canopy presents, and how it grades what it presents.

A server whose product has a tracked release train presents its application version together with how far behind the latest release it is, the updates available from it, and the known issues affecting it (see [FIG](../private-server/server-figures.md)).
A server whose product reports a version Canopy does not track presents that version alone, with no distance, no available update, and no known-issue list, there being no release train to measure it against.
A server whose product has no application version presents no version at all.

A server whose product has a tracked release train but which has not reported a version presents its version as unknown, because there is a version to learn and Canopy has not learnt it.
A server whose product has no application version presents nothing rather than an unknown, because there is nothing to learn.

A group's headline version is the version of its canonical member — its highest-ranked live member, with kind breaking a tie in the order central, then facility, then standalone — chosen among only those live members whose product has a tracked release train.
A group whose live members all belong to products without a tracked release train has no headline version.

The fleet's active-version summary, and the fleet spread and crossings of the application version, cover only servers whose product has a tracked release train.

## Public listing

A server is offered to end-user-facing clients only when its product is eligible for public listing, its kind is central, and an operator has given it a public name.
An operator is offered the public name field only for a server whose product and kind make it eligible.
A public name already set is kept when a server stops being eligible, and takes effect again if the server becomes eligible once more.

## Billing attribution

Cloud cost attributes to the product that incurred it, so the product in a server's billing attribution is that server's own product.
A server's stage comes from its own rank and its deployment from its group, so each label describing the server carries the server's own value rather than its group's.
Attribution needs a deployment to attribute to, so an ungrouped server carries no billing attribution.

A group's own attribution names a product only when its live members agree on one.
A group whose members span products attributes no product at all, since naming one product of several would attribute the group's shared cost to the wrong place.

A billing label an operator sets explicitly on a group is honoured as given, so an operator can attribute a mixed group by hand.
The resources Canopy owns on a group's behalf are the exception: a group's backup storage attributes to Canopy's backup product whatever product label the group carries, so backup spend is never charged to a deployment's application.
