---
id: PRD
---

# Server products

Canopy monitors servers running more than one application.
A server's product names the application it runs, and its kind names the server's role within that product's topology.
The product is the axis that decides which of canopy's per-server features apply to a server at all.

## Product and kind

Every server has a product and a kind, both set by an operator rather than reported by the server.

The products canopy monitors are Tamanu, SENAITE, and canopy itself.
A server's product is Tamanu unless an operator classifies it otherwise.

A kind is a role within one product's topology.
A Tamanu server is a central server, which facility servers sync to, or a facility server, an on-site instance that syncs to a central server.
A product whose servers have no role relative to each other has standalone servers, so SENAITE servers and canopy instances are standalone.

An operator sets both when creating a server and can change either afterwards.
The kinds offered are those the chosen product defines.

A server's product and kind both appear among the reserved read-only tags in the effective tags canopy returns to a server's sources (see [STA](../public-server/statuses.md)), so an agent can read the classification canopy holds for the server it reports on.

## Capabilities

A product determines whether canopy tracks application versions for its servers, whether its servers are eligible for public listing, and whether its servers support managed restore replicas.

Tamanu has a tracked release train, is eligible for public listing, and supports managed restore replicas (see [RST](../public-server/restore-replicas.md)).
SENAITE and canopy have none of the three.

Reachability and health monitoring apply to every server whatever its product, since a server's checks are graded by the source that reports them rather than by the application under them (see [CHK](../monitoring/checks.md)).
Backups likewise apply to every product: a server backs up the types its agent advertises, and which types those are is a property of the agent rather than of the product (see [BAK](../public-server/backup.md)).

## Versions

Version figures and the grading canopy puts on them apply to a server only when its product has a tracked release train (see [FIG](../private-server/server-figures.md)).
A server of a product without one presents no version, no distance from the latest release, no available update, and no known issues.

This is distinct from a server whose product does have a release train but which has not reported a version.
That server presents its version as unknown, because there is a version to learn and canopy has not learnt it.
A server whose product has no release train presents no version affordance at all, because there is nothing to learn.

A group's headline version is the version of its canonical member — its highest-ranked live member, with kind breaking a tie — chosen among only those live members whose product has a tracked release train.
A group whose live members all belong to products without one has no headline version.

The fleet's active-version summary, and the fleet spread of the version figures, count only servers whose product has a tracked release train.

## Public listing

A server is offered to end-user-facing clients only when its product is eligible for public listing, its kind is central, and an operator has given it a public name.
A public name is only meaningful for a server whose product and kind make it eligible, and an operator is only offered the field for such a server.

## Billing attribution

Cloud cost attributes to the product that incurred it, so the product in a server's billing attribution is that server's own product.
A server's stage comes from its own rank and its deployment from its group, so a server's attribution is its own on every label that describes the server itself.

A group's billing attribution names a product only when its live members agree on one.
A group whose members span products attributes no product at all, since naming one product of several would attribute the group's shared cost to the wrong place.

A billing label set explicitly on a group is honoured as given, for the product as for any other label, so an operator can attribute a mixed group by hand.

## Groups

Group membership carries no product constraint, so a deployment's servers stay in one group whatever application each of them runs.
A group-scoped figure that depends on product resolves across the products its live members actually have, rather than assuming they share one.
