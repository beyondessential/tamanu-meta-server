# Classify servers by product

Canopy is taking on non-Tamanu servers, SENAITE first. Today every server
is implicitly a Tamanu server: `ServerKind` conflates a *product* (`Canopy`)
with two Tamanu *topology roles* (`Central`, `Facility`), and the version
catalogue has no product dimension at all, so any server's reported version
is graded against Tamanu's release train.

This adds a `product` axis alongside `kind`, makes `kind` a product-scoped
role, and gates the things that assume Tamanu on a per-product capability.

Spec: [APP](../specs/servers/products.md), with amendments to
[FIG](../specs/private-server/server-figures.md) and
[MCP](../specs/private-server/mcp.md).

## Shape

Three version states, not two. Canopy instances *do* report a version
(`jobs/src/bin/ownstatus.rs:22`, from `CARGO_PKG_VERSION`) but Canopy holds
no release train for it — today that version is graded against Tamanu's
catalogue and yields a nonsense distance. So:

| product | version tracking | public listing |
|---------|------------------|----------------|
| Tamanu  | `Tracked`  — presented and graded | yes |
| Canopy  | `Reported` — presented, ungraded  | no  |
| SENAITE | `None`     — no version at all    | no  |

Backups and health monitoring are *not* capabilities: backup types are
advertised per-server by the agent, and checks are graded by their
reporting source, so both already work for any product untouched. Managed
restore replicas are eligible by intent and backup type, not by product.

## Two-step column split

`kind`'s role is being split, so the migration adds `product` and does
**not** rewrite `kind`. `ServerKind::from_str` errors on unknown values, so
rewriting `kind = 'canopy'` → `'standalone'` in the same deploy would leave
an old binary unable to read the canopy self-server row at all. Instead
`"canopy"` parses as `Standalone` (the same aliasing `kind.rs` already does
for `"tamanu sync server"`), and a later release normalises the leftovers.

- [x] `just migration add_server_product` — `ALTER TABLE servers ADD COLUMN
      product TEXT NOT NULL DEFAULT 'tamanu'`, plus
      `CREATE INDEX servers_product ON servers (product)`
- [x] Backfill in the same migration: `UPDATE servers SET product = 'canopy'
      WHERE kind = 'canopy'`. Leave `kind` untouched
- [x] No `CHECK` constraint on `product`, matching how `kind` is stored, so
      adding a product stays code-only
- [x] After `just migrate`, scrub the regenerated `schema.rs` against `main`
      — keep only the `servers.product` line, drop any other branch's
      migrations that leaked in from the dev database

A follow-up plan handles the cleanup migration (`kind = 'standalone'` where
`'canopy'`, and dropping the alias). Not in this one.

## Types

- [x] New `crates/commons-types/src/server/product.rs`, modelled on the
      existing `kind.rs`: `Product { Tamanu, Senaite, Canopy }` with
      `Default = Tamanu`, `Display`, `FromStr`, `TryFrom<String>`, diesel
      `FromSql`/`ToSql` over `Text`, `Serialize`/`Deserialize` lowercase,
      and `utoipa::ToSchema`
- [x] `VersionTracking { Tracked, Reported, None }` in the same module
- [x] `Product::caps() -> Caps` as a `const fn`, with
      `Caps { version_tracking: VersionTracking, public_listing: bool }`
- [x] `Product::kinds() -> &'static [ServerKind]` and
      `Product::default_kind()`, so the edit endpoint and the UI agree on
      which kinds a product offers without restating the mapping
- [x] `kind.rs`: replace the `Canopy` variant with `Standalone`; `Display`
      writes `"standalone"`; `from_str` accepts `"standalone"` and keeps
      `"canopy"` as a legacy alias
- [x] Export `product` from `crates/commons-types/src/server.rs`

## Database crate

- [x] `servers.rs`: `product: Product` on `Server` (diesel
      `deserialize_as`/`serialize_as = String`, as `kind` does), plus
      `NewServer.product` and `PartialServer.product`. Update the
      `test_server_serialization` expected JSON
- [x] `servers.rs` `search_central` (~:645): add an explicit
      `product = tamanu` filter alongside the existing `kind = central`.
      Today the kind filter excludes SENAITE incidentally; make it hold on
      purpose
- [x] `servers.rs` `tags_for_device` (~:764): emit `canopy:product`
      alongside `canopy:kind` and `canopy:rank`. Without it a Canopy
      instance's agent loses information it gets today, since its
      `canopy:kind` goes from `canopy` to `standalone`
- [x] `server_groups.rs` `kind_priority` (~:33): `Central` 0, `Facility` 1,
      `Standalone` 2 — drop the `Canopy` arm
- [x] `server_groups.rs` `recompute_version` (~:390): filter members to
      `caps().version_tracking == Tracked` *before* the `min_by`; no such
      member ⇒ `(None, None)`. The `statuses` trigger needs no change, it
      only fires on `NEW.version IS NOT NULL`
- [x] New `ServerGroup::member_products` (or equivalent): the distinct
      products among a group's live members, for the billing resolution
      below
- [x] `reported_detail.rs` `production_versions` (~:161): join `servers` and
      restrict to products whose tracking is `Tracked`

## Billing attribution

`BillingLabels` (`commons-servers/src/backup_jobs.rs:236`) hardcodes
`product: "tamanu"`. Three call sites, behaving differently:

- [x] `BillingLabels.product` becomes `Option<String>`, omitted by
      `into_tags` when `None` — the same treatment `stage` already gets, and
      for the same reason
- [x] `from_group` takes the group's resolved product: `Some(p)` when its
      live members agree on one, `None` when they span products. An explicit
      `billing.product` group tag still wins verbatim
- [x] A server-scoped constructor for the per-server case, carrying the
      server's own product and own rank
- [x] `public-server/src/tags.rs:93` (`effective_tags_for_server`) uses it,
      so a SENAITE box reports `billing.product: senaite`. Attribution stays
      inside the existing `if let Some(group_id)` — an ungrouped server
      carries none
- [x] `private-server/src/fns/servers.rs:693`: the server detail view
      currently renders the *group's* labels. Switch it to the server's own,
      so the page matches what the device is actually handed
- [x] `backup_bucket_billing_tags` keeps forcing `"backups"` — verify the
      `Option` change doesn't let a group's product leak into bucket tags

## Private server

- [x] `fns/servers.rs`: `product` through create, update and detail
      responses. Reject an update whose `kind` isn't in the new `product`'s
      `kinds()`, and when `product` changes without an explicit `kind`, move
      the server to the new product's `default_kind()`
- [x] `fns/servers.rs`: keep a stored `public_name` when a server stops
      being eligible rather than clearing it
- [x] `fns/statuses.rs:242`: compute `version_distance` only for `Tracked`
      products. `Reported` sends the version with no distance; `None` sends
      no version
- [x] `fns/statuses.rs:960` (`FIG#fleet-spread`): add `product` to the
      per-server fleet row so the frontend can exclude non-tracked servers
      from the version axis
- [x] `fns/server_groups.rs:115` (`group_billing_labels`): resolve the
      group's product via `member_products`
- [x] `canopy-mcp/src/servers.rs`: `product` as a filter and a returned
      field on find-servers; product counts in the fleet summary
- [x] `canopy-mcp/src/lib.rs:73` and `versions.rs`: descriptions still say
      "Tamanu version". Accurate for the version catalogue, which is
      Tamanu's; reword the *server* descriptions that now span products

## Frontend

- [x] `types.ts`: re-export `Product` and the capability shape from the
      regenerated `api-types.ts`
- [x] New `ServerProductChip.tsx` beside `ServerKindChip.tsx`
- [x] `ServerCreate.tsx` / `ServerEdit.tsx`: a Product select; narrow the
      Kind options to the chosen product's kinds (fixing an existing gap —
      the hardcoded `central`/`facility` menu means `canopy` isn't
      selectable at all today); gate the public-name field on the product's
      `public_listing` as well as `kind === "central"`
- [x] `ServerDetail.tsx:280`: product chip beside the kind chip
- [x] `VersionIndicator.tsx`: keep `version ?? "unknown"` for `Tracked`,
      render the bare version for `Reported`, render nothing for `None`.
      The unknown state stays for a tracked product that hasn't reported —
      it means "not learnt yet", which is not the same as "has no version"
- [x] `ServerDetail.tsx`, `StatusSnapshot.tsx`, `ServerShorty.tsx`, group
      cards: suppress distance, available-updates and known-issues for
      anything but `Tracked`
- [x] `FleetFigures.tsx:44`: exclude non-`Tracked` servers from the
      application-version spread and from any crossing using it as an axis —
      absent from the spread, not counted among the unreported. Leave the
      Postgres, runtime and bestool version spreads covering every server
- [x] ~~`Servers.tsx`: product filter alongside kind and rank~~ — **drifted.**
      `Servers.tsx` is a tab shell, and no list view filters by kind or rank
      either, so there was no filter to sit alongside. Product is instead
      *presented* on every list row via `ServerShorty`'s chip, and filtering
      stays where it already existed: the listing API and MCP's find-servers.
      `APP` was corrected to stop implying an operator-facing filter

## Generated files

- [x] `just gen-openapi` — regenerates `private-web/openapi.json` and
      `private-web/src/api-types.ts`
- [x] The public-server has its own committed `openapi.json` with a drift
      test; regenerate it too, since `tags.rs` and the server shape change
- [x] `just typecheck` from the repo root (not bare `tsc`)

## Testing

- [x] `database`: the backfill lands `product = 'canopy'` on the
      `kind = 'canopy'` row and leaves `kind` alone
- [x] `database`: `recompute_version` skips non-tracked members; a mixed
      group keeps its Tamanu headline; an all-SENAITE group has none
- [x] `database`: `production_versions` excludes non-tracked products
- [x] `database`: `tags_for_device` includes `canopy:product`
- [x] `database`: `search_central` excludes a SENAITE server that has a
      public name and a central kind set directly
- [x] `commons-servers` unit: `BillingLabels` omits the product for a mixed
      group, carries it for a single-product group, and an explicit tag wins
- [x] `public-server`: the tags endpoint returns the server's own
      `billing.product`; an ungrouped server gets no billing labels
- [x] `private-server`: create and update round-trip `product`; a
      product change moves an incompatible kind to the new default; no
      `version_distance` for `Reported` or `None`
- [x] Playwright (`private-web/e2e/`, extend `seed.ts` for `product`):
      a SENAITE server shows no version affordance; a Canopy server shows
      its version with no distance chip; the product select narrows the kind
      options; a mixed group shows its Tamanu headline version; the servers
      list filters by product

## Deploy notes

The migration is additive and the `kind` column is untouched, so an old
binary keeps reading every row. Order doesn't matter.

`Product::from_str` errors on an unrecognised value, the same as `kind`, so
a hand-inserted product would surface as a read error rather than degrade
quietly. That's the existing contract for this kind of column; worth
knowing when debugging.

## Deliberately not in scope

- A SENAITE `BackupType` and its `backup_type_defaults` row. Backups work
  meanwhile: `BackupType` is an open enum and an unrecognised type flows
  through as `Custom`, inheriting the retention floor instead of a tuned
  schedule. Separate spec once the bestool side settles
- The `kind = 'canopy'` cleanup migration and dropping the `from_str` alias
- Product-scoping the version catalogue itself (`versions`,
  `version_known_issues`, `artifacts`). Non-tracked products grade against
  nothing, which is enough — a catalogue per product waits for a real
  second release train
- The `*.cd.tamanu.app` release-candidate page in
  `public-server/src/server_versions.rs`, a Tamanu tool by nature
- `LEGACY_SOURCE = "tamanu"` and the `tamanuVersion` extra, correctly
  Tamanu-specific wire compatibility
- Product as a fleet-spread or crossing axis. `FIG` holds that figures come
  from what sources report, "not from anything an operator enters", and
  product is operator-set, so a `product × version` breakdown would need an
  explicit FIG exception. Nobody has asked for one
