# Test cases: splitting machines, applications, and identities

Coverage the card owes. An unticked box is a scenario not yet covered.

## Model and lifecycle

- [ ] Creating a machine in a group, with no applications, presents as not-yet-checked-in rather than as an error (verifies spec: FLT)
- [ ] A machine's first report creates the applications it describes (verifies spec: FLT)
- [ ] An application Canopy has not seen before is adopted silently, with no operator step (verifies spec: APP)
- [ ] A report that omits a previously-reported application does not remove it (verifies spec: FLT)
- [ ] Only an operator archives an application; an archived one leaves the live fleet with its history intact (verifies spec: FLT)
- [ ] Archiving a machine archives the applications on it (verifies spec: FLT)
- [ ] An application with no operator-set name presents as the sentence case of its type (verifies spec: FLT)
- [ ] An operator-set name overrides the default and survives further reports (verifies spec: FLT)
- [ ] A machine with two applications reports its platform, memory and filesystems once, not once per application (verifies spec: FLT)

## Groups

- [ ] An application takes its machine's group (verifies spec: FLT)
- [ ] Moving a machine to another group moves every application on it (verifies spec: FLT)
- [ ] An application on a machine cannot be moved to a different group on its own (verifies spec: FLT)
- [ ] A machine-less application carries a group of its own (verifies spec: FLT)
- [ ] A machine-targeted check on a grouped machine resolves an incident target through that group (verifies spec: CHK)

## Application types

- [ ] A server with product `tamanu` and kind `central` migrates to an application of type `tamanu-central` (verifies spec: APP)
- [ ] A reported type is adopted without an operator step (verifies spec: APP)
- [ ] A reporter sending a different type under an unchanged key produces an unreachable application and a new one beside it (verifies spec: APP, STA)
- [ ] A type appears among an application's reserved read-only tags and on no machine (verifies spec: APP, FLT)
- [ ] A `tamanu-central` is eligible for public listing; other types are not (verifies spec: APP)
- [ ] A group's headline version comes from the `tamanu-central` on its highest-ranked machine (verifies spec: APP)
- [ ] A group with no `tamanu-central` has no headline version (verifies spec: APP)
- [ ] An upgrade plan measures from the same headline version a group presents (verifies spec: APP)

## Reachability

- [ ] A machine whose sources are all stale is unreachable (verifies spec: CHK)
- [ ] Every application on an unreachable machine is independently unreachable, with no propagation step (verifies spec: CHK)
- [ ] Applications recover independently when their machine reports again (verifies spec: CHK)
- [ ] An application dropped from a live machine's report is unreachable while its machine stays reachable (verifies spec: CHK)
- [ ] An unreachable target keeps its last observed check results and is presented as unreachable (verifies spec: CHK)
- [ ] An application's reachability check can be silenced independently of its machine's (verifies spec: CHK)
- [ ] A machine's reachability silence is reachable from an application on that machine (verifies spec: CHK)
- [ ] Reachability is measured against the target's own threshold, not a fixed one (verifies spec: CHK)
- [ ] A dead application on a live machine is unhealthy rather than unreachable (verifies spec: CHK)

## Checks

- [ ] Every machine check presents on every application on that machine, marked as the machine's (verifies spec: CHK)
- [ ] A degraded machine check opens one incident from the machine's scope, not one per application (verifies spec: CHK)
- [ ] A silence on a machine check quiets it on every application presenting it (verifies spec: CHK)
- [ ] An application's health rollup counts its machine's checks (verifies spec: CHK)
- [ ] An application check is catalogued as `<type>.<check>`; two types reporting one name are two entries (verifies spec: CHK)
- [ ] A machine check is catalogued under its bare name (verifies spec: CHK)
- [ ] A machine's monitoring switch does not silence the applications on it (verifies spec: CHK)
- [ ] A machine-scoped issue does not collide with a canopy-wide issue on the same `(source, ref)` (verifies spec: CHK)

## Status pushes

- [ ] A split push is ingested against the machine and applications it names, unmodified (verifies spec: STA)
- [ ] A unified push is separated by the machine-subject rule and ingested against both grains (verifies spec: STA)
- [ ] A push with no health checks is still treated as a legacy Tamanu report (verifies spec: STA)
- [ ] A reporter field named `source`, `health`, `check` or `result` inside `detail` is recorded and does not collide with the envelope (verifies spec: STA)
- [ ] Two applications sharing a key cannot be expressed in a payload (verifies spec: STA)
- [ ] Correlation is by machine, key and type together (verifies spec: STA)
- [ ] A check name reported bare is catalogued qualified by the reporting application's type (verifies spec: STA, CHK)
- [ ] A push response returns effective tags for the machine and each application described (verifies spec: STA)
- [ ] `caddy_certs` files against the application while `caddy_version` files against the machine (verifies spec: STA)
- [ ] `ips` files against the machine while `ips_errors` files against the application (verifies spec: STA)

## Figures

- [ ] Platform spreads over machines; a two-application box counts once (verifies spec: FIG)
- [ ] Application version spreads over applications (verifies spec: FIG)
- [ ] A crossing counts machines whatever is on its axes, and names the unit it counts (verifies spec: FIG)
- [ ] A machine whose applications disagree on an application figure appears in each matching cell (verifies spec: FIG)
- [ ] A machine-less application is absent from crossings (verifies spec: FIG)
- [ ] The OS timezone and an application's configured timezone present as separate figures and may differ (verifies spec: FIG)
- [ ] A machine reporting no OS falls back to the family its applications' database engine gives away (verifies spec: FIG)
- [ ] Runtime version falls back to the reporting identity's connection metadata (verifies spec: FIG)

## Identities

- [ ] A machine-gated route resolves the machine from the authenticated identity (verifies spec: FLT)
- [ ] An admin-gated route resolves no machine (verifies spec: FLT)
- [ ] `GET /machines/self` returns the identity, its machine, and the applications on it (verifies spec: DID)
- [ ] `GET /servers/self` reaches the same answer and is marked deprecated (verifies spec: DID)
- [ ] Enrolment accepts `server` as an alias for the machine role (verifies spec: DTR)

## Billing

- [ ] An application's labels name its type, stage and deployment (verifies spec: APP)
- [ ] A machine's labels carry no type, and its stage is the highest rank among its applications (verifies spec: APP)
- [ ] A group's labels carry no type (verifies spec: APP)
- [ ] A group's backup storage still attributes to Canopy's backup product (verifies spec: APP)

## Backups

- [ ] An application onboarded into an existing backup configuration is not stale on arrival (verifies spec: BKJ)
- [ ] Backup capability and configuration follow the machine (verifies spec: APP)

## Migration

- [ ] Every existing server becomes one application and one machine (verifies spec: FLT)
- [ ] `alert_when_down_for`, the group and the identity link land on the machine (verifies spec: FLT)
- [ ] A migrated application's type is corrected by the first report that disagrees with it (verifies spec: APP)
- [ ] Existing silences, incidents and check states survive the rename intact
- [ ] `/servers/{id}` redirects to the application that server became

## Interface

- [ ] The group page lists rank, then machine, then applications
- [ ] Both detail pages end with the group's tree, with the current page highlighted
- [ ] The application page presents its own and its machine's checks in one list
- [ ] The application page carries no backups and no identity
- [ ] The machine page carries no URL
- [ ] Neither detail page shows a status dot beside its title
- [ ] A group card's operator count counts people once across machines
- [ ] The operator tooltip names each person and the machines they are on
- [ ] The status card encloses every machine, including one hosting a single application
- [ ] A machine enclosure carries the machine's state; the indicator inside carries the application's
- [ ] Rank rows carry the rank spelled out behind their applications
- [ ] The fleet listing has no ungrouped tab
