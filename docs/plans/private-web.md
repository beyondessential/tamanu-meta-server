# Plan: migrate private-server frontend from Leptos to React + MUI + Vite

## Goal

Replace the Leptos SSR + WASM-hydrate frontend in `crates/private-server/` with a separate React + MUI + Vite SPA at `/private-web/`. The Leptos UI keeps serving production traffic unchanged throughout the migration; the new SPA is dev-only until cutover, when Leptos is stripped out and the SPA is shipped (embedded in the binary or as a sibling container — decided at cutover time).

The reference for patterns (build pipeline, SPA fallback, `--dev-no-auth`) is `~/code/work/seedling/crates/web/`. Patterns port; seedling-specific names (OI, Actor, etc.) do not.

## Settled decisions

- **Auth:** Tailscale-only via the existing axum middleware, plus a seedling-style `--dev-no-auth` flag for loopback so local dev doesn't need Tailscale running.
- **Public-server stays as-is.** Tera-rendered, untouched.
- **Browser support:** WebTransport ruled out earlier; we're using plain HTTP/JSON for the API. All major browsers ship the relevant features regardless.
- **Production rollout:** new frontend is *not* built into the container during the migration. Production keeps running Leptos. Cutover is a single switch at the end.
- **Migration strategy:** stack commits on a single branch, frequently. Splitting into PRs is the user's job (via jj) — don't think in PR boundaries.

## Phases

### Phase 1 — backend prep (no React yet)

**1a. Dev auth bypass — already in place.**

`commons-servers::tailscale_auth::TailscaleAdmin` already short-circuits to a synthetic `admin@localhost` user under `cfg!(debug_assertions)` (i.e. `cargo run` / `cargo test`). The existing tests rely on this. No new flag is needed for the React dev workflow because `cargo run` of private-server already accepts unauthenticated requests. If a release-build bypass becomes necessary later (e.g. staging), revisit then.

**1b. JSON-everywhere for server functions.**

Switch every `#[server]` fn in `crates/private-server/src/fns/` from Leptos's default form-encoded inputs to JSON inputs and outputs. 53 functions across 8 modules (admins, commons, devices, servers, statuses, sql, versions, bestool).

Done in the same change:
- Update tests in `crates/private-server/tests/` that POST to `/api/private_server/fns/...` from `.form(&[...])` to `.json(...)`.
- Update the AGENTS.md test guideline that currently says "use `.form(&[...])` for parameters (not `.json()`)" to say the opposite.

This unblocks the React client and is a tractable mechanical change. Audit the 53 fns first: confirm which currently use form vs. json so the sweep is uniform.

### Phase 2 — scaffold `/private-web/`

Create the new frontend project at `/private-web/` (repo root). Tooling:
- Vite + `@vitejs/plugin-react`
- TypeScript
- **React 19** (stable since late 2024, no reason to start on 18)
- MUI v9 — **without writing emotion in our own code**. MUI internally uses emotion, so it's a transitive dep regardless, but we don't import `@emotion/styled` or `@emotion/react` ourselves; we stick to MUI's prebuilt components and the `sx` prop. If we end up wanting our own components we revisit (Pigment CSS or plain CSS modules).
- `react-router-dom` v7
- A neutral data-fetching hook (`useApi` or `useRequest` — exact name TBD, **not** `useOi`)
- Theme provider with light/dark detection
- Vitest for unit tests; Playwright optional, deferred

Vite proxy config forwards `/api/private_server/fns/*` to the running private-server (default `http://[::1]:8081`). No CORS handling needed because of the proxy.

No pages migrated yet — just scaffold + a single "hello" route to prove the proxy round-trip works against a real server function (e.g. `is_current_user_admin`).

### Phase 3 — page migrations

Migrate pages in dependency-light order. Roughly:

1. **Statuses dashboard** (`/status`) — read-only, exercises `Resource`/`Suspense` pattern translation, custom-event refresh, release summary component.
2. **Admins** (`/admins`) — small page, simple CRUD, good template for forms + confirmation dialogs.
3. **Versions** (`/versions`, `/versions/:id`) — read + create/update artifacts, version-range vs exact override logic.
4. **Servers** (`/servers`, nested: `list`, `detail`, `edit`, `import`, `geo`) — heaviest section: ticket import, geo, parallelized parent + status + version lookups.
5. **Devices** (`/devices`, nested: `search`, `list`, `detail`, `history`) — search across multiple indices, role updates, key name editing.
6. **Bestool** (`/bestool`, `bestool/snippets/:id`) — versioned snippet supersession.
7. **SQL playground** (`/sql`) — read-only DB queries with timeout + history pagination.

Each page replicates the existing Leptos behaviour. Bulma styling is **not** ported — pages get restyled in MUI from scratch. When MUI X DataGrid would meaningfully reduce list-page code (devices, servers, versions), use it; otherwise stick to base MUI.

Components in `crates/private-server/src/components/` translate roughly 1:1 in concept (paginated_list, legend, smalls, shorties, sub_tabs, release_summary, time_ago, version_indicator, toast). Rebuild as React + MUI; do not attempt to share types with Rust beyond what serde produces over the wire.

### Phase 3.5 — Playwright tests, page-by-page

Rather than batching e2e coverage at the end, add Playwright tests as each page lands so the test suite grows with the migrated UI.

- Add `@playwright/test` and a `playwright.config.ts` to `/private-web/` when the first migrated page is ready.
- Tests live in `/private-web/e2e/` and run against `pnpm dev` plus the running `private-server` API. Use Playwright's `webServer` config to start Vite for the test run; the API is the operator's responsibility to start (same as dev).
- Each migrated page gets at least:
  - one happy-path test that loads the page and asserts a real backend response renders;
  - one interactive test where applicable (form submission, role change, dialog flow).
- Don't seed via the React UI — set up state via direct DB writes or by calling the API directly from the test, then exercise the UI path.
- CI integration is deferred until cutover, since the React app isn't in the container yet.

### Phase 4 — cutover

When all pages have parity in `/private-web/`:

1. Add `build.rs` + `rust-embed` (or sibling container, decided here) to ship the SPA in the deployed artefact.
2. Add an axum SPA-fallback handler in private-server that serves the embedded assets and `index.html`.
3. Strip Leptos: remove `cargo leptos` deps, the `hydrate` feature, the Leptos `app/` and `fns/` layer (server fns become plain axum handlers — possibly under a redesigned URL shape now that we're free to choose).
4. Remove Bulma assets, Leptos `static/private/` CSS files, `tamanu_logo.svg` if no longer referenced.
5. Update the workspace `[[workspace.metadata.leptos]]` section accordingly (probably delete it).
6. Deploy. Production switches to the React SPA in one release.

## Out of scope

- Touching `crates/public-server/`.
- Designing a long-term API contract; the `/api/private_server/fns/<module>/<fn>` URL pattern stays during the migration and gets redesigned (or kept) at cutover.
- WebTransport, server-push, real-time streaming. Plain HTTP/JSON is sufficient for canopy's admin-UI use case.
- Sharing types between Rust and TS via codegen. We can revisit at cutover; for now hand-written TS interfaces matching the JSON shape are fine.
- Browser-support gymnastics; assume baseline modern browsers.

## Open questions

- Exact data-fetching hook name (currently leaning `useApi`).
- Whether to embed the SPA in the binary at cutover (single artefact, simpler ops) or ship a separate frontend container (independent rollout, cache-friendly). Decide at cutover.
- Whether to keep the `/api/private_server/fns/*` URL shape post-cutover or rename to something cleaner. Cosmetic; decide at cutover.
