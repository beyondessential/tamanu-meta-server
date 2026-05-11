# OpenAPI for public-server and private-server

## Goal

Generate an OpenAPI 3.x spec from the axum handlers in both server crates, serve
it (plus an interactive UI) inline, and use it as a single source of truth for:

1. API contract documentation for anyone consuming the public-server (device
   integrators, the bestool/CLI clients, future external consumers).
2. Generated TypeScript types for the React frontend in `private-web/`,
   replacing the hand-maintained `private-web/src/types.ts`.
3. A spec-drift safety net: a snapshot test that flags changes to the wire
   contract.

## Non-goals

- Switching to a typed RPC framework (tRPC, twirp, tonic). The existing
  axum-handler pattern stays.
- Generating a full TS client (we keep the `callApi`/`useApi`/`useApiAction`
  hooks in `private-web/src/api.ts` — just feed them generated types).
- Adding GraphQL or any other API style.

## Stack choice

- `utoipa` 5.x — schema derives + `#[utoipa::path(...)]` macros for handlers.
- `utoipa-axum` 0.2 — `OpenApiRouter` drop-in for `axum::Router` that mirrors
  the `Router` API and collects path declarations as routes are registered.
  Supports axum 0.8 (which we use).
- `utoipa-swagger-ui` 9.x — embeds Swagger UI and serves it at a chosen route.

Feature flags we need on `utoipa`:
- `jiff_0_2` — native `ToSchema` for `jiff::Timestamp`, `jiff::Zoned`,
  `jiff::civil::Date`.
- `uuid` — native `ToSchema` for `Uuid`.
- `axum_extras` — utoipa-side helpers for axum-specific extractors
  (`Path<(A, B)>` tuples, etc.).
- `macros` (default) — `#[derive(ToSchema)]`, `#[utoipa::path]`.

Types **without** a utoipa feature that appear in our payloads — each needs a
`value_type = ...` or `schema_with = ...` override on the field, or a custom
`ToSchema` impl on the newtype that wraps it:

- `ipnet::IpNet` (`database::devices::DeviceConnection`, `NewDeviceConnection`)
  → describe as `String` with format `cidr`.
- `url::Url` (wrapped by `database::url_field::UrlField` in `Server.host`) →
  describe as `String` with format `uri`. Define `ToSchema` once on `UrlField`.
- `node_semver::Version` (wrapped by `commons_types::version::VersionStr`,
  appears in `Status.version`) → describe as `String` with example
  `"2.10.5"`. Define `ToSchema` once on `VersionStr`.
- `jiff::SignedDuration` (wrapped by `database::pg_duration::PgDuration`) →
  describe as `String` (ISO-8601 duration) or `Number` of seconds, whichever
  the existing serde impl chooses. Verify before annotating.
- `serde_json::Value` (`database::statuses::Status.extra`, a few request args)
  → utoipa renders this as an unrestricted object; should "just work".

## Layout of the spec

- **One spec per server.** The public-server and private-server have disjoint
  consumers and different security postures, so generating one OpenAPI doc
  per binary is cleaner than merging.
- **Spec endpoint**: `/api/openapi.json`. Always-on — both servers are on
  trusted networks (mTLS for public; Tailscale for private) and the spec
  contains no secrets.
- **Swagger UI**: `/api/docs/` on each server.
- **Tags**: one tag per module (`admins`, `devices`, `issues`, …).

## Deferred decisions

- **Whether to add a `utoipa` feature flag** to gate the `ToSchema` derives on
  `database` and `commons-types`. Default: no — `utoipa` is a small dep and
  splitting the trait derives behind a feature adds friction for every type
  added later. If build-time cost ever bites, revisit.
- **Snapshot testing the spec.** Plausibly useful, plausibly noisy. Try without
  first; add if drift becomes a problem.
- **Whether to ship a `redoc` or `scalar` UI in addition to Swagger UI.** Skip
  for now; Swagger UI is universal.

---

## Phase 0 — workspace deps (private-server stack)

- Add to `[workspace.dependencies]` in root `Cargo.toml`:
  - `utoipa = { version = "5", features = ["jiff_0_2", "uuid", "axum_extras"] }`
  - `utoipa-axum = "0.2"`
  - `utoipa-swagger-ui = { version = "9", features = ["axum"] }`
- Pull these into `crates/private-server/Cargo.toml`,
  `crates/commons-errors/Cargo.toml`, `crates/commons-types/Cargo.toml`,
  `crates/database/Cargo.toml`.

Commit: `openapi: add utoipa workspace deps`.

## Phase 1 — `ToSchema` on shared types

Add `#[derive(utoipa::ToSchema)]` (alongside the existing `Serialize`/
`Deserialize` derives) to every type that flows through a JSON request or
response.

### commons-errors

- Add a `ProblemDetailsSchema` struct (the wire shape of RFC 7807:
  `type`, `title`, `status`, `detail`, plus our `instance` if used). Derive
  `ToSchema`. Reference it from every handler's error response.
- `AppError` itself does not need `ToSchema` — at the wire level it always
  serializes as `ProblemDetails`.

### commons-types

Unit-variant enums (trivial — just add the derive):
- `device::DeviceRole`
- `server::kind::ServerKind`
- `server::rank::ServerRank`
- `status::ShortStatus`
- `version::VersionStatus`
- `issue::Severity`
- `issue::ResolvedReason`

Structs:
- `geo::GeoPoint` — primitive fields, trivial.
- `server::ticket::CanopyTicket` — primitives + Uuid, trivial.
- `server::cards::FacilityServerStatus`, `CentralServerCard` — uses Uuid,
  `VersionStr`, `ServerRank`, `ShortStatus`. Trivial once `VersionStr` has a
  schema.

Newtype with manual schema:
- `version::VersionStr` (wraps `node_semver::Version`) — write `impl ToSchema`
  by hand returning a `String` schema with example `"2.10.5"` and pattern
  describing semver.

### database

**Note (discovered during private-server impl):** private-server's `fns/<mod>`
modules define their own wrapper response types (`DeviceInfo`, `IncidentData`,
`ServerInfo`, etc.) — database structs are *not* serialized directly to the
wire. So database `ToSchema` derives are needed only for the public-server
phase. Listed here for completeness; deferred to Phase 5.

Newtypes with manual schema:
- `url_field::UrlField` → `String`, format `uri`.
- `pg_duration::PgDuration` — used internally only (not on any handler),
  skip until it shows up.

Structs with trivial derives (just add `ToSchema`):
- `artifacts::{Artifact, NewArtifact}` (public-server direct returns)
- `versions::{Version, ViewVersion, NewVersion}` (public-server)
- `issues::{Issue}` (public-server `events::create` returns this)
- `servers::{Server, NewServer, PartialServer}` (public-server)
- `statuses::{Status, NewStatus}` (public-server)
- The rest (`Admin`, `Device*`, `Incident*`, `IssueNote`/`IncidentNote`,
  `TailscaleUser`, `BestoolSnippet`, `SqlPlaygroundHistory`,
  `ChromeRelease`) are not directly serialized by any current handler —
  add `ToSchema` only when/if they get exposed.

Structs that need field-level overrides (when added):
- `devices::{DeviceConnection, NewDeviceConnection}` — `ip` field gets
  `#[schema(value_type = String, format = "cidr")]`.

Commits:
- `openapi: ToSchema on commons-types`
- `openapi: ToSchema on database types`
- `openapi: ProblemDetailsSchema in commons-errors`

## Phase 2 — private-server: `OpenApiRouter` migration + handler annotation

For each module under `crates/private-server/src/fns/`:

1. Change the module's `routes()` to return `OpenApiRouter<AppState>` and
   register handlers via `.routes(routes!(handler1, handler2, ...))` instead
   of `Router::new().route(...)`.
2. Annotate every handler with `#[utoipa::path(post, path = "/<route>", ...)]`,
   specifying `request_body`, `responses`, `tag`, and `security` where
   relevant.
3. Derive `ToSchema` on every `*Args` and `*Data` / `*Info` struct in the
   module.

Module-by-module checklist (handler counts from the survey):

- `admins.rs` — 3 handlers (list, add, delete). All `TailscaleAdmin`-gated.
- `bestool.rs` — 5 handlers. Mix of `TailscaleUser` (optional auth) and none.
- `commons.rs` — 3 handlers, all unauth or optional auth.
- `devices.rs` — 12 handlers. All `TailscaleAdmin`. Largest module; many
  pagination args.
- `incidents.rs` — 10 handlers, all `TailscaleAdmin`.
- `issues.rs` — 14 handlers, all `TailscaleAdmin`. `submit_manual_event`
  takes a `Timestamp`.
- `servers.rs` — 8 handlers. Mix of `TailscaleAdmin` and unauth.
- `sql.rs` — 4 handlers, mostly unauth or optional `TailscaleUser`.
- `statuses.rs` — 3 handlers, unauth. `server_grouped_ids` returns a
  `BTreeMap<ServerRank, Vec<Uuid>>` — utoipa renders maps fine, but verify.
- `versions.rs` — 8 handlers, `TailscaleAdmin` for writes, unauth for reads.

Total: ~70 handler annotations.

### `Page<T>` generic

`Page<T>` is used with four concrete instantiations:
- `Page<BestoolSnippetInfo>`
- `Page<DeviceInfo>`
- `Page<ServerInfo>`
- `Page<SqlHistoryEntry>`

Approach: derive `ToSchema` on `Page<T>` and register each instantiation in
the top-level `#[derive(OpenApi)]` `components(schemas(...))` list (or use
inline schemas at the `responses(...)` call site, whichever utoipa
ergonomically supports — verify at implementation time).

### Top-level wiring

In `crates/private-server/src/fns.rs`:

```rust
let (router, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
    .nest("/api/admins", admins::routes())
    .nest("/api/bestool", bestool::routes())
    // ...
    .split_for_parts();
```

Where `ApiDoc` is a `#[derive(OpenApi)]` struct in the same module that lists:
- `info` (title, version from `CARGO_PKG_VERSION`, description).
- `components(schemas(...))` for the `Page<T>` instantiations and any
  schemas not auto-pulled.
- `security_schemes(...)` defining `TailscaleAdmin` and `TailscaleUser` as
  apiKey schemes on the `Tailscale-User-Login` header.
- `tags(...)` one per module.

In `crates/private-server/src/lib.rs::routes()`:
- Replace the `.merge(fns::routes())` line with two adds: the generated
  `Router` from `split_for_parts()` and a `SwaggerUi::new("/api/docs")
  .url("/api/openapi.json", openapi)` mounted alongside.
- Make sure the SPA fallback (`spa::handler`) sits below the swagger and
  spec routes so they don't get eaten.

Commits (one per module — feel free to bundle small ones):
- `openapi: scaffold OpenApiRouter and ApiDoc`
- `openapi: annotate admins`
- `openapi: annotate bestool`
- `openapi: annotate commons`
- `openapi: annotate devices`
- `openapi: annotate incidents`
- `openapi: annotate issues`
- `openapi: annotate servers`
- `openapi: annotate sql`
- `openapi: annotate statuses`
- `openapi: annotate versions`
- `openapi: mount swagger-ui and spec endpoint`

## Phase 3 — security schemes and error responses

- Declare two security schemes on the private spec:
  - `tailscale-user` — apiKey, header `Tailscale-User-Login`.
  - `tailscale-admin` — apiKey, header `Tailscale-User-Login`, with a
    description note that the user must be on the admin list.
- Apply per-handler `security = (("tailscale-admin" = []))` etc.
- Every handler gets standard error responses declared:
  - `400` (bad request — version parse, source manual, etc.)
  - `401` (unauthenticated)
  - `403` (forbidden — wrong role)
  - `404` (resource not found)
  - `500` (server error)
- All non-2xx responses share the `ProblemDetailsSchema` body.

Likely a helper macro or `responses(...)` shorthand to avoid restating these
on every handler. Investigate utoipa's `IntoResponses` derive — we can derive
it once on `AppError` (or a wrapper) and reference that.

## Phase 4 — tests

- One unit-style test in `crates/private-server/tests/openapi.rs` that builds
  the `OpenApi` value, serializes it to JSON, and asserts:
  - It deserializes back via `serde_json::from_str::<OpenApiSpec>`.
  - Each module is represented (presence check on a sample path per module).
  - No `$ref` is unresolved (utoipa-side validation if available).
- Optionally write the spec to `crates/private-server/openapi.json` at build
  time via a small `xtask`-style binary or skip and let CI generate it on
  demand. Decision: skip the committed file — let the build emit on request.

## Phase 5 — public-server (deferred to a separate stack)

Same approach, with the following twists:

- `password.rs`, `server_versions.rs`, and the HTML/redirect/binary routes in
  `versions.rs` (`view_artifacts`, `view_mobile_install`, `download_artifact`,
  the SVG/QR/HTML responses) are excluded from the spec. They keep their
  existing `Router::new().route(...)` wiring and are merged in as a normal
  `axum::Router` after the `OpenApiRouter` is split.
- `timesync.rs` (raw bytes, Timesimp protocol) is excluded.
- Security schemes: declare `device-mtls` (XFCC / mtls-certificate header)
  with sub-roles `server-device`, `admin-device`, `releaser-device`. mTLS in
  OpenAPI is awkward — represent as an apiKey on the cert header and note in
  the description that it's mTLS in practice.
- Swagger UI at `/api/docs`, spec at `/api/openapi.json`. Same as private.

Handler counts (JSON only — others stay un-annotated):
- `artifacts.rs` — 1
- `bestool.rs` — 1
- `events.rs` — 1
- `servers.rs` — 4
- `statuses.rs` — 1
- `versions.rs` — 4 (list, create, remove, list_artifacts, update_for)

Total: ~12 handler annotations + the ToSchema work is mostly reused from
Phase 1.

## Phase 6 — TS codegen for `private-web`

**Status: done, with a couple of follow-ups deferred (see below).**

Shipped:
- `crates/private-server/src/bin/openapi-dump.rs` — standalone binary that
  builds the `OpenApi` value and prints pretty JSON to stdout. No DB
  required.
- `just gen-openapi` — runs the dump binary, writes `private-web/openapi.json`,
  then invokes `npm run gen:api-types` to refresh
  `private-web/src/api-types.ts`.
- `private-web/openapi.json` and `private-web/src/api-types.ts` are both
  committed: fresh checkouts can `npm install && npm run dev` without
  needing to build the rust side first.
- `private-web/src/types.ts` is now a thin re-export layer over
  `api-types.ts`. UI-only constants (`SEVERITIES`, `RESOLVED_REASONS`,
  `RESOLVED_REASON_LABEL`, `SERVER_RANK_ORDER`, `ServerGroupedIds`) stay
  hand-written; the hand-written `Page<T>` generic also stays since
  utoipa emits one schema per concrete instantiation. Aliases
  (`DeviceInfoData = DeviceInfo`, `ServerInfoFull = ServerInfo`,
  `DeviceShortInfo = DeviceInfo`) keep existing call sites working.

Implementation choices, with rationale for future me:
- `openapi-typescript` is installed with `--legacy-peer-deps` because v7
  pins TypeScript ^5 but the project uses TS ^6 (no real incompatibility —
  it's a CLI tool, doesn't import the project's TS).
- A `Solidify<T>` helper in `types.ts` strips the `field?: T | null`
  emitted for every `Option<T>` Rust field. Serde always emits the field
  (as `null`), so the `?` is wrong at runtime; utoipa's "not required"
  marking is a known mismatch with serde's wire behaviour.
- Conflicting `operationId`s (every `ack`/`unack`/`resolve`/etc. that
  exists in both `issues` and `incidents`, and `list` in admins + issues)
  are disambiguated by setting `operation_id = "issue_xxx"` /
  `"incident_xxx"` on those handlers. Other handlers keep their
  function-name default.

Deferred follow-ups (file as issues, not blockers):
- Update `callApi<T>(...)` to optionally accept generated path types from
  `paths` so consumers get a strongly-typed `(module, fn)` pair instead of
  free-form strings.
- Wire `gen-openapi` into a pre-commit hook or CI drift-check.
- Audit call sites for places that could now use the generated types
  directly (e.g. references to `Schemas["DeviceInfo"]` instead of going
  through the alias).

## Risks

- **node_semver::Version round-trip.** `VersionStr` serializes as a string;
  our manual schema must match. If a consumer sends `"2.10"` (no patch), the
  current parser rejects it — document `pattern` strictly.
- **`Page<T>` ergonomics.** If utoipa's generic schema handling is more
  awkward than expected (e.g. forces us to alias every instantiation), we
  may end up with `PageDeviceInfo`, `PageServerInfo` etc. in the spec.
  Acceptable, but ugly. Verify early.
- **Auth extractor surprises.** utoipa-axum's `routes!` macro doesn't
  introspect the handler signature — it only consumes the `#[utoipa::path]`
  annotation. So our custom auth extractors won't trip it up. (Confirmed
  from the docs; verify when the first module is wired.)
- **Build-time cost.** `utoipa-gen` is a proc-macro; cross-crate it adds
  measurable compile time. Single-server scope keeps that bounded.
