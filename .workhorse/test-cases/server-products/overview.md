# Classifying servers by product

Scenarios verifying that a server's product decides which of canopy's per-server
features apply to it, that the two ways a version can be missing stay
distinguishable, and that a group holding more than one product resolves its
shared figures without borrowing from a member that has nothing to give.

## Product and kind as separate axes

- [x] A row that predates the column reads as Tamanu, so nothing has to be reclassified to keep working — verifies spec: APP
- [x] A row still carrying the legacy `canopy` kind reads as standalone rather than failing to parse, since the migration deliberately left that column alone — verifies spec: APP
- [x] Creating a server without naming a product classifies it as Tamanu — verifies spec: APP
- [x] Creating a server with a product round-trips both the product and its role — verifies spec: APP
- [x] Creating a server with a role its product does not define is refused — verifies spec: APP
- [x] Changing a Tamanu central to SENAITE, which has no central, carries the server to SENAITE's default role rather than stranding it — verifies spec: APP
- [x] Changing between two products that both define standalone leaves the role alone — verifies spec: APP
- [x] Asking for a role the target product does not define is refused rather than quietly corrected — verifies spec: APP
- [x] A device is told its server's product alongside its kind, so an agent can read the classification canopy holds — verifies spec: APP
- [x] The product catalogue describes every product's capabilities and roles, so the operator UI never keeps its own copy — verifies spec: APP

## Version: tracked, reported, or absent

- [x] A Tamanu server presents its version with its distance from the latest release and a link into the catalogue — verifies spec: APP
- [x] A Tamanu server that has not reported a version presents "unknown", because there is a version to learn — verifies spec: APP
- [x] A SENAITE server presents no version affordance at all, label included: there is nothing to learn, so an "unknown" would read as a reporting failure — verifies spec: APP
- [x] A product whose version canopy does not track presents the bare version with no distance and no catalogue link — verifies spec: APP
- [x] A canopy instance's own reported build version presents ungraded end-to-end in the UI, rather than being measured against Tamanu's releases — verifies spec: APP

## Group figures across products

- [x] A group holding both a Tamanu server and a SENAITE one takes its headline version from the Tamanu member, even when the SENAITE member outranks it — verifies spec: APP
- [x] A group whose live members all belong to products with no tracked release train has no headline version at all — verifies spec: APP
- [x] A mixed-product group's card shows the Tamanu member's version rather than blanking because a member has none — verifies spec: APP

## Fleet-wide figures

- [x] The production-version summary counts only servers whose product has a tracked release train, so an untracked product does not contribute a release branch of its own — verifies spec: APP
- [x] The application-version spread covers only servers that have a version, and a server without one is absent from it rather than counted among those reporting nothing — verifies spec: APP
- [x] The database-engine spread still covers the whole fleet, that figure having nothing to do with which product a server runs — verifies spec: APP
- [x] A crossing with the application version as one axis drops an uncovered server from both axes rather than placing it in the unreported row — verifies spec: APP

## Public listing

- [x] The public mobile-app listing excludes a product canopy does not list publicly, even when that server has been given a central role and a public name — verifies spec: APP
- [x] The public-name field is offered only for a product and role that can be listed — verifies spec: APP
- [x] Choosing a product that cannot be listed takes the public-name field away — verifies spec: APP
- [x] A public name already set survives the server losing eligibility, and takes effect again if it regains it — verifies spec: APP

## Billing attribution

- [x] A SENAITE server sharing a group with Tamanu ones attributes its cost to SENAITE, not to the deployment's application — verifies spec: APP
- [x] A server's deployment still comes from its group while its stage comes from its own rank — verifies spec: APP
- [x] An ungrouped server carries no billing labels at all, there being no deployment to attribute to, while its classification tags remain — verifies spec: APP
- [x] A group whose live members agree on one product attributes to that product — verifies spec: APP
- [x] A group whose members span products attributes no product, rather than charging shared cost to one of several — verifies spec: APP
- [x] An explicit billing product set on a group is honoured as given, so a mixed group can be attributed by hand — verifies spec: APP
- [x] Backup storage attributes to canopy's own backup product whatever product label the group carries — verifies spec: APP
- [x] The server detail view renders the server's own labels rather than its group's, so the page agrees with what the device is handed — verifies spec: APP
