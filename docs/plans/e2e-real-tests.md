# Playwright e2e: real tests, not page-load smoke

The existing `private-web/e2e/*.spec.ts` files are mostly placeholders.
The fixture spins up a per-worker DB + private-server + Vite, but the
DB starts empty and the specs just `goto(...)` and assert that an
`h1` or `[role="alert"]` is visible. The app shell always has an `h1`
("Canopy" in the nav), so the assertions pass even when the page
itself 404s. Multiple "detail page loads" tests navigate to
`/servers/00000000-0000-0000-0000-000000000000` (a UUID that
deliberately doesn't exist) and call that a pass.

`admins.spec.ts` is the only existing spec that actually seeds data
via the API and asserts on it. That's the model we're building out to
the rest of the suite.

---

## 1. Seeding infrastructure

### A `pg`-based SQL fixture

Add the `pg` npm devDependency. Expose a new worker-scoped `sql`
fixture in `test-fixtures.ts` that opens a single client per worker
against the per-worker database (`StackHandle.dbName` already
exists; we just need to surface a connection URL too, or wire it
straight from the fixture). The client gets disposed when the
worker tears down.

API on the fixture: a thin wrapper around `pg.Client.query` that
returns the rows:

```ts
const rows = await sql<{ id: string }>(
  "INSERT INTO servers (host, kind) VALUES ($1, 'central') RETURNING id",
  ["https://e2e.example.invalid"],
);
```

Tests get a `sql` fixture in addition to `page` and `request`.

### Seed helpers

Layer typed helpers on top of `sql` for the most common shapes.
Names and roughly the parameters they take:

- `seedServer({ name?, host?, kind?, rank? }) -> { id, name, host, ... }`
- `seedDevice({ role? }) -> { id, role }`
- `seedDeviceServerLink(deviceId, serverId)` (attach a device to a server)
- `seedStatus({ serverId, deviceId?, healthy?, health?, extra?, createdAt? }) -> { id, ... }`
- `seedIssue({ serverId, source?, ref?, severity?, message?, active?, ... }) -> { id, ... }`
- `seedVersion({ major?, minor?, patch?, status? }) -> { id, ... }`

Each helper returns the inserted row's id (and a few other readable
fields) so the test can assert on what it just inserted without
re-fetching. Defaults pick sensible test values (random host, unique
name).

Live in `private-web/e2e/seed.ts`.

### Tear-down

Per-test cleanup is *not* needed: the worker DB is fresh per worker
and torn down at the end. Tests can lean on uniqueness via random
names where needed, but most can simply not care about isolation.

---

## 2. Per-spec rewrites

Each existing spec gets replaced with one or more meaningful tests
that:

1. Seed the rows it intends to display.
2. Navigate to the page.
3. Assert on the *seeded* values' visibility (name, host, ID,
   status text, etc.) — not on generic chrome elements.

### `status.spec.ts`

- Seed two centrals (one production, one staging) and a facility
  under the production central. Seed recent statuses for each.
- `/status` should render the production rank heading, both
  servers by name, and the facility's status dot grouped under its
  parent.
- Keep one "page chrome" test for the nav and title (a minimal
  smoke for plumbing). Drop the "either data or alert" test —
  redundant once we actually exercise the data path.

### `servers.spec.ts`

- List page: seed a couple of servers; assert their names appear as
  row links. Seed a facility, switch to the Facility tab, assert it
  shows up there.
- Detail page: seed a server, navigate to `/servers/<id>`, assert
  the seeded name appears as the heading and the host appears in
  the URL block. (Replaces the 404'd UUID test.)
- Edit page: seed a server, navigate to `/edit`, assert the name
  input is pre-filled with the seeded value.

### `devices.spec.ts`

- Seed an untrusted device and a trusted device. Verify the
  Untrusted tab shows the untrusted device row and the Trusted tab
  shows the other (with the right active-key marker).
- Detail page: seed a device, go to `/devices/<id>`, assert the
  device's role and IDs render.

### `versions.spec.ts`

- Seed a couple of versions at different statuses. Verify the index
  page groups them and the detail page renders the changelog of the
  specific version.

### `admins.spec.ts`

- Already meaningful — leave the structure, just port it to the new
  `sql`-based seeding for consistency (the API-based seeding it
  uses today is fine but the rest of the suite will use `sql`).

### `sql.spec.ts` and `bestool.spec.ts`

- These are admin-only UI tabs. Keep them as page-loads-and-renders
  smoke tests but tighten the assertions to specific elements (a
  named editor / a Help button / etc.) rather than the bare
  presence of any heading.

---

## 3. Tests for the just-shipped health-check feature

At least one spec dedicated to the new wire fields + UI. Lives in
`private-web/e2e/health.spec.ts`. Coverage:

- **ServerDetail Healthy chip**: seed a server with a status where
  `healthy = false`. Go to `/servers/<id>`. Assert the "Unhealthy"
  chip is visible. Repeat the inverse for healthy.
- **Checks table**: seed a server with a status whose `health[]`
  has one failing and one passing check (and one optional extra
  field on the failing one). Assert the failing check appears
  first, the extras render as key/value lines, the passing check
  appears below.
- **StatusDot health border**: seed two servers — one fully healthy,
  one healthy-but-degraded (top-healthy with a failing check). Go
  to `/status`. Assert the degraded server's dot has the warning
  outline applied (can read via `getAttribute("style")` or by
  checking computed style for an outline width > 0).
- **Status snapshot modal**: seed a server with a status and an
  issue against it. Go to the issue (or to a place that shows
  IssueRow). Click the snapshot button, assert the modal opens with
  the status row's `created_at` and `healthy` indicator visible.

Aim for one assertion per test rather than long scripts. If the
snapshot modal proves fiddly to drive, settle for opening the modal
and asserting it shows the "Healthy" chip — the other coverage is
backed by Rust integration tests.

---

## 4. Out of scope (intentional)

- **No mocking of the Tailscale directory.** Tests stick to flows
  that don't need a directory — `TailscaleUser` already has a
  dev-bypass that auths every request as `admin@localhost`.
- **No auth assertions.** We're not exercising the `tag:canopy-*`
  rejection paths or admin-only gates here; the Rust tests cover
  those.
- **No screenshots / visual regression.** Plain DOM assertions
  only.
- **No CI integration changes.** `just test-e2e` already runs the
  suite; CI wiring is a separate concern.
- **No mobile / responsive coverage.** Single-viewport tests.

---

## 5. Implementation order

1. `chore: add pg devDependency and a sql fixture for e2e` — wire
   the fixture, plumb the DB URL through `StackHandle`, write
   `seed.ts` with the helpers above. Leave the existing specs
   untouched so the suite still passes.
2. `test: rewrite status / servers / devices / versions specs to
   seed real data` — one commit or split per spec depending on
   size; aim for the rewritten suite to remain green and to fail
   if you break the corresponding page.
3. `test: tighten admins / sql / bestool specs` — port admins to
   `sql` seeding, tighten sql/bestool smoke tests.
4. `test: cover healthy/health UI with playwright` — new
   `health.spec.ts` covering the chip, checks table, dot border,
   and (best-effort) snapshot modal.

After step 4: re-read this plan, drop anything missed onto the
end of the implementation, and unplan.
