# Stand up crate publish pipeline for canopy

Take `bes-canopy-api` from unpublished (as C3 landed it) to published, with the version
model, trusted-publishing pipeline, compatibility gate, and prod-deploy gate the card
calls for. The versioning model is settled in [APIC](../../specs/platform/api-client-crate.md)
and [API](../../specs/platform/api-compatibility.md); this card implements it.

## Starting point (what C3 left)

- `crates/canopy-api` — `bes-canopy-api`, `publish = false`, manifest `version = "0.1.0"`
  hand-written, `info.version` in `crates/public-server/src/openapi.rs` is `""`.
- The codegen does **not** yet stamp the crate version from `info.version`, and records no
  document digest. Both are part of "the crate inherits the version by construction" and
  land here.
- `cd.yml` triggers on every push to `main`: builds arm64 binaries, builds+pushes the
  container image, runs Pulumi against the ops repo. No crate publish, no release job.
- `ci.yml` has a `generated` job (`just check-generated`, `just check-api-deps`) but no
  `cargo-semver-checks`.

## Design areas to settle

Notes land under each as decisions are made.

### 1. Version inheritance mechanism
The manifest version must equal `info.version` *by construction*, not by a tool writing the
same number twice. The only way the crate manifest genuinely derives from the document is if
the codegen writes it. Open: does `gen-api` stamp `crates/canopy-api/Cargo.toml`'s `version`
from `openapi.json`'s `info.version`? That collides with any tool (release-plz) that wants to
own the manifest version — see area 2.

### 2. Release flow: how much of release-plz, and who stamps `info.version`
The tension: release-plz's model is that the crate manifest owns the version and release-plz
bumps it from conventional commits (+ semver-checks). This card's model is that `openapi.rs`
owns it and the manifest follows. Reconciling these is the crux of the card.

### 3. Trusted publishing + bootstrap
Manual `0.0.0` dummy publish (correct name + `repository` metadata) to reserve the name and
let the trusted publisher be configured on crates.io, then CI publishes subsequent releases
tokenlessly via `rust-lang/crates-io-auth-action`. First real release targets `1.0.0`.

### 4. The `cargo-semver-checks` compatibility gate
A CI job comparing the regenerated crate against the published baseline. Needs a published
baseline to exist, so it is inert until after the first publish. Open: blocking behaviour and
how a coordinated break (major bump) is recognised as permitted.

### 5. Prod-deploy gate
`cd.yml` currently deploys on every push to `main`. Move the prod deploy so only a release
(the release-plz PR merge / the tag it creates) deploys. Open: what still happens on a plain
`main` push (image build for non-prod environments?).
