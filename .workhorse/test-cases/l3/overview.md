# Crate publish pipeline

Most of this card is CI/CD wiring, so the cases split into two kinds: the version-stamping
logic, which is ordinary code and covered by unit tests, and the pipeline's behaviour, which
is only truthfully verified by watching a real release happen.

The pipeline cases are unticked and stay that way until a release has actually run. They are
coverage this card owes, not optional extras.

## Version stamping

- [x] The generated manifest takes its version from the document's `info.version`
- [x] Dependency versions are untouched, including a `version` key in a later table that
      matches the same shape as the package's own
- [x] Stamping is idempotent, and stamping the version already present rewrites nothing
- [x] A manifest with no `[package]` section, or a `[package]` with no version, is refused
      rather than silently left alone
- [x] A `[package]` table at the end of the file is still found
- [x] Generation fails, with a message naming the cause, when the document declares no
      `info.version` — an unversioned crate is never produced (verifies spec: APIC)
- [x] The generated source records the document's version and a BLAKE3 digest of it
      (verifies spec: APIC)

## The version invariant

- [x] `just gen-openapi && just gen-api` leaves no diff on a clean tree
- [ ] A hand-edited version in `crates/canopy-api/Cargo.toml` fails `just check-generated`,
      because regenerating rewrites it from the document (verifies spec: APIC)
- [ ] Editing `info.version` and regenerating moves the document, the client's manifest, and
      the recorded constants together in one commit (verifies spec: APIC)

## The compatibility gate

- [ ] An uncoordinated breaking change to the public API fails the `semver` CI job
      (verifies spec: API)
- [ ] The same change passes once `info.version` raises the major, so a coordinated break
      lands and an uncoordinated one cannot (verifies spec: API)
- [ ] Adding an optional property passes without a version edit in the feature PR
      (verifies spec: API)

## Releasing

- [ ] A merge to `main` opens or updates a release PR and deploys nothing
- [ ] The release PR carries the propagated `info.version`, the regenerated document and
      client, and is green on `check-generated`
- [ ] Merging the release PR publishes `bes-canopy-api` to crates.io through trusted
      publishing, with no registry token in the repository (verifies spec: APIC)
- [ ] Merging the release PR deploys, and the image is tagged with canopy's version
- [ ] A server-only change releases canopy and deploys without publishing the client
- [ ] A client-only change (a codegen change, say) publishes without deploying
- [ ] Canopy's first release is `1.0.0`
- [ ] `cargo publish` is never reachable for the vendored `canopy-utoipa-axum`, nor for any
      crate outside the two tracks
