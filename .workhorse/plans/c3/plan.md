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

## The spec's own version stays empty

`crates/public-server/src/openapi.rs` sets `info.version = ""`, and it stays that way.
Commit 9c9a5f8b emptied it deliberately, because crate versions here are frozen and
the auto-deploy never bumps them, so the field was stale noise. This card does not
reopen that: the version it introduces is the generated crate's, and Cargo.toml is
where that already lives.

Setting it to the api crate's version does not work anyway. The crate's version is
derived from the crate, since `cargo-semver-checks` compares the crate generated
before a change against the one generated after in order to decide the bump, so
writing that version into the spec needs a value that only exists after generating a
crate from that spec. release-plz picks the bump at release time, once the code is
final and `gen-openapi` has long since run. It would also churn a checked-in file: the
committed `openapi.json` would go stale on every release, which either fails the
freshness check or forces a regenerate-and-commit into the release process.

The dependency runs spec to crate, so provenance runs crate to spec. The generated
crate records which document it came from, carrying forward what bestool did with
`OPENAPI_BLAKE3` and `OPENAPI_SOURCE` but pointed at the committed file rather than a
live fetch. Because the spec is committed, `git rev-parse HEAD:crates/public-server/openapi.json`
is a blob sha that comes for free and ties to history.

Note that OpenAPI 3.1 marks `info.version` REQUIRED and defines it as the version of
the OpenAPI document, distinct from the API implementation version, so a client
library's version would be the wrong thing to put there regardless.

Optional, and not needed here: the served spec and the committed spec are different
documents. `public-openapi-dump` could keep `""` while the running server injects the
deploy sha at runtime, so a consumer fetching the live document can tell which deploy
answered, at no cost to the committed file's stability.

## Out of scope

- **bestool card X1**: what `bestool-canopy` becomes. It survives; its remaining shape
  is bestool's decision.
- **canopy card L3**: the publish pipeline. Every workspace member is currently
  `publish = false` and `cd.yml` only builds an image and runs Pulumi, so there is no
  release path to publish onto. L3 also picks up the `cargo-semver-checks` gate, which
  needs a published baseline to compare against.

## Open

- `request_body_type` falls back to `serde_json::Value` for open `allOf` schemas such
  as `StatusPayload`, and `response_type` does the same for non-`$ref` non-array
  responses. Those are holes the semver check cannot see through, the same class of
  blind spot the `API` spec already notes for map values.
