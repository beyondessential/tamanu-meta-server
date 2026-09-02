# Test cases: splitting machines, applications, and identities

Coverage the card owes. An unticked box is a scenario not yet covered.

## Model and lifecycle

- [x] Creating a machine in a group, with no applications, presents as not-yet-checked-in rather than as an error (verifies spec: FLT)
- [ ] A machine's first report creates the applications it describes (verifies spec: FLT)
- [ ] An application Canopy has not seen before is adopted silently, with no operator step (verifies spec: APP)
- [ ] A report that omits a previously-reported application does not remove it (verifies spec: FLT)
- [x] Only an operator archives an application; an archived one leaves the live fleet with its history intact (verifies spec: FLT)
- [x] Archiving a machine archives the applications on it (verifies spec: FLT)
- [ ] An application with no operator-set name presents as the sentence case of its type (verifies spec: FLT)
- [ ] An operator-set name overrides the default and survives further reports (verifies spec: FLT)
- [x] A machine with two applications reports its platform, memory and filesystems once, not once per application (verifies spec: FLT)

## Groups

- [x] An application takes its machine's group (verifies spec: FLT)
- [x] Moving a machine to another group moves every application on it (verifies spec: FLT)
- [x] An application on a machine cannot be moved to a different group on its own (verifies spec: FLT)
- [x] A machine-targeted check on a grouped machine resolves an incident target through that group (verifies spec: CHK, INC)

## Application types

- [x] A server with product `tamanu` and kind `central` migrates to an application of type `tamanu-central` (verifies spec: APP)
- [ ] A reported type is adopted without an operator step (verifies spec: APP)
- [ ] A reporter sending a different type under an unchanged key produces an unreachable application and a new one beside it (verifies spec: APP, STA)
- [x] A type appears among an application's reserved read-only tags and on no machine (verifies spec: APP, FLT)
- [x] A `tamanu-central` is eligible for public listing; other types are not (verifies spec: APP)
- [x] A group's headline version comes from its highest-ranked version-tracked application (verifies spec: APP)
- [x] A group whose applications are all of untracked types has no headline version (verifies spec: APP)
- [x] A group of facilities still has a headline version; a central only wins a rank tie (verifies spec: APP)
- [x] An upgrade plan measures from the same headline version a group presents (verifies spec: APP)

## Reachability

- [ ] A machine whose sources are all stale is unreachable (verifies spec: CHK)
- [ ] Every application on an unreachable machine is independently unreachable, with no propagation step (verifies spec: CHK)
- [ ] Applications recover independently when their machine reports again (verifies spec: CHK)
- [ ] An application dropped from a live machine's report is unreachable while its machine stays reachable (verifies spec: CHK)
- [x] An unreachable target keeps its last observed check results and is presented as unreachable (verifies spec: CHK)
- [ ] An application's reachability check can be silenced independently of its machine's (verifies spec: CHK)
- [ ] A machine's reachability silence is reachable from an application on that machine (verifies spec: CHK)
- [x] Reachability is measured against the target's own threshold, not a fixed one (verifies spec: CHK)
- [x] A gap inside the threshold is simply reachable, with no intermediate state (verifies spec: CHK)
- [x] A target quiet for longer than the status lookback window is unreachable, not never reported (verifies spec: CHK)
- [x] A target with no report at all is never reported, on every surface that presents one (verifies spec: CHK)
- [x] A group whose members all reported long ago still offers the archive that cascades to them (verifies spec: FLT)
- [ ] A dead application on a live machine is unhealthy rather than unreachable (verifies spec: CHK)

## Checks

- [ ] Every machine check presents on every application on that machine, marked as the machine's (verifies spec: CHK)
- [x] A degraded machine check contributes one issue at machine scope, not one per application (verifies spec: CHK)
- [x] A machine-scoped and an application-scoped issue in one group join the same incident (verifies spec: INC)
- [x] A silence on a machine check quiets it on every application presenting it (verifies spec: CHK)
- [ ] An application's health rollup counts its machine's checks (verifies spec: CHK)
- [x] An application check is catalogued as `<type>.<check>`; two types reporting one name are two entries (verifies spec: CHK)
- [x] A machine check is catalogued under its bare name (verifies spec: CHK)
- [x] A curated source's check is catalogued by name alone, even when that name is a machine one (verifies spec: CHK)
- [x] A silence on one type's namespace leaves the other type's state alone (verifies spec: CHK)
- [x] A namespace first reporting after the split registers pending review (verifies spec: CHK)
- [x] Decommissioning one namespace's check leaves the same name in another namespace live (verifies spec: CHK)
- [x] A machine's monitoring switch does not silence the applications on it (verifies spec: CHK)
- [x] A machine-scoped issue does not collide with a canopy-wide issue on the same `(source, ref)` (verifies spec: CHK)

## Status pushes

- [ ] A split push is ingested against the machine and applications it names, unmodified (verifies spec: STA)
- [x] A unified push is separated by the machine-subject rule and ingested against both grains (verifies spec: STA)
- [x] A push with no health field is a legacy Tamanu report, while an empty health set is the source reporting nothing (verifies spec: STA)
- [ ] A reporter field named `source`, `health`, `check` or `result` inside `detail` is recorded and does not collide with the envelope (verifies spec: STA)
- [ ] Two applications sharing a key cannot be expressed in a payload (verifies spec: STA)
- [ ] Correlation is by machine, key and type together (verifies spec: STA)
- [ ] A check name reported bare is catalogued qualified by the reporting application's type (verifies spec: STA, CHK)
- [ ] A push response returns effective tags for the machine and each application described (verifies spec: STA)
- [x] `caddy_certs` files against the application while `caddy_version` files against the machine (verifies spec: STA)
- [x] `ips` files against the machine while `ips_errors` files against the application (verifies spec: STA)

## Figures

- [ ] Platform spreads over machines; a two-application box counts once (verifies spec: FIG)
- [ ] Application version spreads over applications (verifies spec: FIG)
- [ ] A crossing counts machines whatever is on its axes, and names the unit it counts (verifies spec: FIG)
- [ ] A machine whose applications disagree on an application figure appears in each matching cell (verifies spec: FIG)
- [x] The OS timezone and an application's configured timezone present as separate figures and may differ (verifies spec: FIG)
- [ ] The Munin flag is a machine figure, and the Munin link is offered on the machine (verifies spec: SVC, FIG)
- [ ] A machine reporting no OS falls back to the family its applications' database engine gives away (verifies spec: FIG)
- [ ] Runtime version falls back to the reporting identity's connection metadata (verifies spec: FIG)

## Identities

- [ ] A machine-gated route resolves the machine from the authenticated identity (verifies spec: FLT)
- [ ] An admin-gated route resolves no machine (verifies spec: FLT)
- [x] `GET /machines/self` returns the identity, its machine, and the applications on it (verifies spec: DID)
- [x] `GET /servers/self` keeps the shape it has always had, for callers that predate the split (verifies spec: API, DID)
- [x] Enrolment accepts `server` as an alias for the machine role, and Canopy stores and presents `machine` (verifies spec: DTR)

## Billing

- [x] An application's labels name its type, stage and deployment (verifies spec: APP)
- [x] A machine's labels carry no type, and its stage is the highest rank among its applications (verifies spec: APP)
- [x] A group's labels carry no type (verifies spec: APP)
- [x] A group's backup storage still attributes to Canopy's backup product (verifies spec: APP)

## Backups and restores

- [x] A machine onboarded into an existing backup configuration is not stale on arrival (verifies spec: BKJ)
- [x] Adding a second application to a machine does not restart that machine's backup staleness anchor (verifies spec: BKJ)
- [x] Backup capability and participation follow the machine, and a two-application box is one participant (verifies spec: BAK, BKO)
- [x] A device request resolves identity to machine to group, without reaching the applications on it (verifies spec: BAK)
- [x] A restore-replica declaration names a machine, and a whole-group declaration expands over machines (verifies spec: RST)
- [x] `restore-verification` and `redaction` file at machine scope; `migration-test` files at application scope (verifies spec: RST)
- [x] A migrate worklist entry names the machine's snapshot and the application whose candidate it carries (verifies spec: RST)

- [x] A box running two workloads gets one replica of its one snapshot, not one per workload (verifies spec: RST)
- [x] A migration report that omits the application has it resolved from the machine and the version (verifies spec: RST)

## Maintenance windows

- [x] A window over a machine grades every check on every application on that box to skipped (verifies spec: MNT)
- [x] A window covers the box it names and no other (verifies spec: MNT)
- [x] An application's detail page reports it as under maintenance when its machine's window holds (verifies spec: MNT)
- [x] Declaring from an application's page opens the window over its machine (verifies spec: MNT)
- [x] Windows predating the split are backfilled onto the machine their application ran on, leaving group windows alone (verifies spec: MNT)
- [x] A machine-scoped silence still applies only to the machine's own checks, not to the applications on it (verifies spec: CHK)
- [ ] The fleet maintenance view links a machine target to its detail page (verifies spec: MNT)

## Names and certificates

- [x] Declaring a name another application already holds is refused, and the refusal names the holder (verifies spec: CRT)
- [x] A certificate request from a two-application machine resolves to the application declaring the requested name (verifies spec: CRT)
- [x] A request for a name none of the machine's applications declares is refused identically whether another application holds it or nobody does (verifies spec: CRT)
- [x] The entitlement answer carries one entry per application on the machine (verifies spec: CRT)
- [ ] The entitlement answer on a status-push response matches the standalone one (verifies spec: CRT, STA)
- [x] Releasing a name stops renewal and leaves existing records and certificates in place (verifies spec: CRT)

## Fleet query interface

- [x] `Get machine` returns platform and hardware figures; `Get application` returns version and database engine (verifies spec: MCP)
- [x] `Find machines` returns machines with their application counts (verifies spec: MCP)
- [x] `Find issues` filtered by application returns the machine's issues among the application's own (verifies spec: MCP)
- [x] `Get incident` reports each issue's scope (verifies spec: MCP)
- [x] A machine's issue names its machine rather than reading as group-wide (verifies spec: MCP)
- [ ] MCP health classifications match what the operator UI presents for the same machine or application (verifies spec: MCP)

## Wire compatibility

The bar is mechanical: `bestool-canopy` generated from the public OpenAPI spec
before and after this branch must not be a semver-breaking change. These cases
cover the runtime half of that, which the spec diff cannot see.

- [x] The public spec loses no path, no schema, no field and no enum variant across the branch (verifies spec: API)
- [x] A restore worklist entry names the restored box as both `machine_id` and `server_id` (verifies spec: API, RST)
- [x] A verification report naming only `server_id` settles its replica exactly as one naming `machine_id` (verifies spec: API, RST)
- [x] A verification report naming both, or neither, is refused rather than resolved by preference (verifies spec: API, RST)
- [x] An application's device tags carry `canopy:type` and both halves of the pair it replaced (verifies spec: API, APP)
- [x] A Tamanu facility's `canopy:product` reads `tamanu`, so both Tamanu types still bill as one (verifies spec: APP)
- [ ] A fielded bestool pushing the earlier server-scoped status shape still files its checks (verifies spec: STA)

## Migration

- [ ] Every existing server becomes one application and one machine (verifies spec: FLT)
- [ ] `alert_when_down_for`, the group and the identity link land on the machine (verifies spec: FLT)
- [ ] A migrated application's type is corrected by the first report that disagrees with it (verifies spec: APP)
- [ ] Existing silences, incidents and check states survive the rename intact
- [x] Each application type that reported a check gets its own catalog entry, with its own first and last seen (verifies spec: CHK)
- [x] A machine-subject name from a structured source is re-keyed, not multiplied (verifies spec: CHK)
- [x] Curated sources' catalog entries pass through the fan-out untouched (verifies spec: CHK)
- [x] The fan-out carries the review stamp, so no vetted check drops to its pending-review cap at migration (verifies spec: CHK)
- [x] A catalog entry with no application left to name a type is dropped, and the loss is filed as an issue naming it
- [x] A group-, machine- or fleet-scoped policy on an application check becomes one row per live type in that scope (verifies spec: CHK)
- [x] A scoped policy over a scope with no applications is dropped
- [x] The fleet-wide fan-out does not mint a namespace from canopy's own nil application
- [x] Rolling the migration back collapses each check to its most recently reported namespace, spanning the whole set's first and last seen

## Interface

- [x] The machine form asks nothing about what runs on the box: no product, kind, rank, URL or public name
- [x] One type chip replaces the product and kind chips wherever an application is listed
- [x] The edit form shows an application's type rather than offering it
- [x] The public-name field is offered only for a type eligible for public listing
- [x] A group names a billing product only when its applications all run one software
- [x] Creating a machine lands on that machine's own page
- [x] A machine's reachability can be silenced from the machine form, as a server's could (verifies spec: CHK)

- [x] The group page lists rank, then machine, then applications
- [ ] Both detail pages end with the group's tree, with the current page highlighted
- [ ] The application page presents its own and its machine's checks in one list
- [ ] The application page carries no backups and no identity
- [x] The machine page carries no URL
- [x] A machine's page presents what the box reports: platform, processors, memory, bestool
- [x] A box carrying two workloads lists both, each linking to its own page
- [x] A machine with nothing on it reads as awaiting check-in, on its page and in its group
- [x] An application links to its machine, and the machine back to the application
- [x] A maintenance window over a machine suspends the applications on that machine (verifies spec: MNT)
- [x] A machine's check can be silenced against the machine, from the machine's page (verifies spec: CHK)
- [x] The scopes a machine's check offers are the machine and its group, and nothing above (verifies spec: CHK)
- [x] One machine silence reads the same in the consolidated view, in what the agent is told, and in what an incident counts (verifies spec: CHK)
- [x] A machine silence does not reach the applications on that machine (verifies spec: CHK)
- [x] A group silence covers a machine in that group (verifies spec: CHK)
- [ ] Neither detail page shows a status dot beside its title
- [ ] A group card's operator count counts people once across machines
- [ ] The operator tooltip names each person and the machines they are on
- [x] The status card encloses every machine, including one hosting a single application (verifies spec: CHK)
- [x] A machine enclosure carries the machine's state; the indicator inside carries the application's (verifies spec: CHK)
- [x] A box under a maintenance window is marked as such on the status page (verifies spec: CHK, MNT)
- [x] A group-wide window marks every box in that group's card (verifies spec: MNT)
- [x] A group card's dots are rank rows of machine enclosures rather than one flat strip
- [x] Rank rows carry the rank spelled out behind their applications
- [x] A group card is three bands, and a quiet one is two: the status band is omitted with neither operator nor incident
- [ ] The fleet listing has no ungrouped tab
