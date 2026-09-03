# Extract OpenAPI part as separate published crate

Canopy publishes `bes-canopy-api`, generated from its own committed
`crates/public-server/openapi.json`. `bestool-canopy` depends on it and drops the
build-time fetch of the live spec.

## Where the seam sits

`bestool-canopy` already has the right seam, and it is the thin part. `transport.rs`
is about 97 lines defining `CanopyTransport`:

```rust
async fn call(&self, request: http::Request<Bytes>) -> Result<http::Response<Bytes>>
```

The URI is path-only, the transport resolves the base URL, and a non-2xx comes back
as-is for the client to interpret. Nothing in that is specific to either repo.

The trait moves down into `bes-canopy-api` rather than staying in bestool, because
the generated code is not only wire types. `build.rs` emits both the typify types and
an `impl<T: CanopyTransport> CanopyClient<T>` block carrying one method per operation,
routed through `call_json` / `call_empty`. Whoever generates the methods has to own
the trait and the client they hang off.

## Layer split

`bes-canopy-api`, published from this repo:

- `CanopyTransport`, moved as-is
- `CanopyClient<T>` and its call plumbing: gzip the request body, map non-2xx to
  `CanopyHttpError`, parse the response
- generated wire types and generated per-endpoint methods, committed rather than
  produced in a build script
- `Redacted`
- no default transport

The crate needs no reqwest dependency. `client.rs`'s `reqwest::Method` and
`reqwest::StatusCode` are the `http` types re-exported, so switching to `http::`
leaves only `http`, `bytes`, `serde`, `serde_json`, `flate2`, `jiff`, `miette`,
`async-trait` and `bon`.

`bestool-canopy` keeps the environment-specific layers: `ReqwestTransport` (tailscale
probing, mTLS device identity, cert rotation), the `new` / `with_urls` constructors,
`registration.rs` (which path-depends on `algae-cli`), `backup.rs`'s `TargetOutcome`,
and the `raw-requests` escape hatch. Exactly what it settles on is bestool's call,
tracked as bestool card X1.

## The generator

The generator lives here, as a workspace member producing committed source, driven by
a `just` recipe alongside `gen-openapi`. bestool's `build.rs` is roughly 450 lines of
typify plus hand-rolled method emission, so it is the starting point to port; what
carries over unchanged and what gets rewritten is settled while doing it.

## Why the generated methods stay in the published crate

A types-only crate would be thinner but would cost the compatibility check. The `API`
spec counts removing a path or an operation as a break, and that is only mechanically
visible if the operations exist as methods for `cargo-semver-checks` to see disappear.
Keeping the generated methods in the published crate is what turns that clause from
aspiration into something a CI job can fail on.

## Commit the generated source

Generated Rust is committed, mirroring the existing pattern for `openapi.json` and
`private-web/src/api-types.ts`: both generated, both checked in, freshness enforceable
in CI. Committed source is reviewable in the diff, cheap to build, and is what
`cargo-semver-checks` wants to compare. A `build.rs` regenerating on every build hides
the surface and is harder to check. Drift is handled the same way `just gen-openapi`
drift would be, with a CI freshness check.

## Machinery that stops being load-bearing

`build.rs` injects `#[derive(bon::Builder)]` and `#[non_exhaustive]` on every
named-field struct, and its own comment gives the reason: canopy's OpenAPI evolves
independently of bestool, so a struct built with a literal breaks the moment canopy
adds a field. That is a consequence of fetching live with no version relating the two.
Once the crate is published with a real semver, adding an optional field is a minor
bump the consumer opts into.

Keep `#[non_exhaustive]` anyway, but as a deliberate choice: it is what makes adding
an optional property genuinely non-breaking under `cargo-semver-checks`.

The `rewrite_types` string substitutions (chrono to `jiff::Timestamp`, and wrapping
`secret_access_key` / `session_token` / `repo_password` in `Redacted`), currently
defended by asserts that fail the build when a substitution stops matching, become
canopy-side decisions at the source. Canopy knows which of its fields are secrets.

## Versioning: the spec holds the version, the crate inherits it

The crate is a pure function of the schema and the generator, so its surface moves only
when one of those moves. Under that correspondence a single version identifies the
document and the crate alike, and there is no reason for the two to differ:
`info.version` carries it and the generated crate takes it as its `Cargo.toml` version.

Which one holds it is the part that matters. The spec declares the version and the
crate inherits it, running the same direction as the derivation. The alternative, where
release-plz owns the crate manifest and the spec trails behind, keeps them equal only
by convention. This also puts `info.version` to the use OpenAPI 3.1 defines for it, as
the version of the document, which is a REQUIRED field that `""` satisfies in type
only. Commit 9c9a5f8b emptied it because crate versions here are frozen and the
auto-deploy never bumps them, which was true then and stops being true once a
versioned crate is published from this repo.

There is no ordering cycle, because codegen does not consume the version. The sequence
is generate, compare, stamp: generate the crate from the schema, run
`cargo-semver-checks` against the published baseline to decide the bump, then write the
result into the spec's literal and regenerate. release-plz already works in two phases
like this. A bump therefore edits one literal in `crates/public-server/src/openapi.rs`,
and the regenerated `openapi.json` and crate manifest follow from it in the same commit,
so a freshness check stays green rather than going stale on release.

The crate can still record a digest of the document it was generated from, carrying
forward what bestool did with `OPENAPI_BLAKE3` but pointed at the committed file. With
versions 1:1 that is mostly redundant, but it catches the mistake case, where a document
changed without the version moving with it.

The version describes the crate's surface rather than the API's shape, which settles the
case where the crate is a function of the generator as well as the schema: a typify
upgrade or a change to the ported codegen that moves the generated surface raises the
document's version too, even though the API it describes is untouched. The bump is
decided by `cargo-semver-checks` either way.

The crate is named `bes-canopy-api`.

## No serde_json::Value fallbacks

bestool's generator degrades three of the 33 operations to `serde_json::Value`. None of
them is lossy in the schema or in the Rust types, so the ported generator types all
three.

`POST /status/{server_id}` takes `StatusPayload`, whose Rust form is a typed struct
plus a flattened catch-all:

```rust
#[serde(flatten)]
#[schema(additional_properties = true, value_type = Object)]
pub extra: serde_json::Map<String, serde_json::Value>,
```

utoipa renders that flatten as an `allOf` with a free-form member, and
`is_open_schema` pattern-matches the shape and gives up. The generator emits the typed
struct with its own `extra` map field instead.

`GET /bestool/snippets` and `GET /status/{server_id}/check-severities` are typed maps
already. The handler returns `Json<BTreeMap<String, CheckSeverity>>` and the schema
says `additionalProperties` with a `$ref` and `propertyNames` of string. They degrade
only because `response_type` handles `$ref` and `type: array` and lets everything else
fall through. The generator maps `additionalProperties` to `HashMap<String, T>`.

This is a usability fix before it is a compatibility one. `HealthCheck` carries the
same flatten, and typify drops the free-form part, so a client built from today's
generator cannot express per-check extra fields at all: no latency, no free disk, no
certificate expiry. That is why the whole status push body falls back to `Value`, on
the endpoint reporters use most.

On the compatibility side, typing the maps makes their value types visible to
`cargo-semver-checks`, so a change to `CheckSeverity` or `SnippetResponse` stops being
invisible. It does not close the map blind spot the `API` spec notes, which is about
dynamic keys: check names are operator-defined, so the key set stays a hand-maintained
promise. Half the gap.

The reason the fallbacks exist is that bestool was downstream, consuming whatever
arrived and unable to change either end. Both ends are canopy's now, so a shape the
generator cannot type is a bug to fix at whichever end is wrong.

## Considered: share the Rust types instead of generating

If both ends are ours, the JSON round-trip is optional: the wire types could live in
`bes-canopy-api` directly, with public-server depending on it and using them in its
handlers, leaving no generator to be deficient. Not taken. It inverts the dependency so
that the server imports its wire types from a published crate, it leaks `database` and
`commons-types` shapes into that crate, and it reverses the spec-to-crate direction the
versioning model rests on. The Rust types are already correct and only the generator is
thin, so the generator is the cheaper place to fix.

## Build steps

- [x] `crates/canopy-api-codegen`: workspace member, not published, binary that reads
      `crates/public-server/openapi.json` and writes `crates/canopy-api/src/generated.rs`
- [x] Port the typify schema emission: wrap `components.schemas` into a JSON Schema root,
      emit wire types, keep `#[non_exhaustive]` plus a builder on named-field structs
- [x] Timestamps by declared format, not by string substitution: `format: date-time`
      becomes a timestamp type, and `CredentialProcessOutput.Expiration` gains that format
      in `openapi.rs` so it needs no special case
- [x] Secrets from a declared list of schema properties, validated against the document so
      a renamed field fails generation rather than silently unwrapping
- [x] Open `allOf` (a typed member beside a free-form member) emits the typed struct with a
      map field for the further keys, so `StatusPayload` and `HealthCheck` are typed
  - [x] typify 0.7 drops `additionalProperties` from a schema that also has declared
        properties, so the catch-all field is injected into the emitted AST, and
        generation fails if a folded schema does not receive one
- [x] `additionalProperties` with a declared value type emits a map of that type, so
      `/bestool/snippets` and `/status/{server_id}/check-severities` are typed
- [x] Emit one method per operation over `http::Method`, names derived from the path with
      the verb disambiguating a path served by several verbs
- [x] `crates/canopy-api`: package `bes-canopy-api`, hand-written `CanopyTransport`,
      `CanopyClient<T>`, error type, `Redacted`, and the committed generated source
- [x] No HTTP client dependency: `http` and `bytes` only for the wire, gzip for bodies
- [x] `just gen-api` beside `gen-openapi`, and a CI check that regenerating leaves no diff
- [x] Assert no operation generates an untyped JSON body or response
- [x] `just check`, `just lint`, `just fmt-check`, `just test-package bes-canopy-api`

## Specs and repo docs

[APIC](../../specs/platform/api-client-crate.md) covers the published crate: generated in
this repository, one version shared with the document, every operation typed, transport
supplied by the consumer, and what the generated types carry.
[API](../../specs/platform/api-compatibility.md) keeps the compatibility definition and
now states it against Canopy's own published crate, and its unreached-surface section
narrows to map keys, since a map's value type is generated and judged with the crate.

`AGENTS.md` line 45 states the compatibility bar in terms of `bestool-canopy` generated
from `crates/public-server/openapi.json`. It names the published crate instead once this
lands, so the rule agents read matches the specs.

## Out of scope

- **bestool card X1**: what `bestool-canopy` becomes. It survives; its remaining shape
  is bestool's decision.
- **canopy card L3**: the publish pipeline, and the first publish. Every workspace
  member is currently `publish = false` and `cd.yml` only builds an image and runs
  Pulumi, so there is no release path to publish onto. This card therefore lands the
  crate unpublished, and L3 flips it, publishes it, implements the version stamping,
  and adds the `cargo-semver-checks` gate, which needs a published baseline to compare
  against. L3 carries the detail.
