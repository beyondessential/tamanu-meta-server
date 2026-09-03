# Stand up crate publish pipeline for canopy

Take `bes-canopy-api` from unpublished (as C3 landed it) to published, with the version
model, trusted-publishing pipeline, compatibility gate, and prod-deploy gate the card calls
for. The versioning model is settled in
[APIC](../../specs/platform/api-client-crate.md) and
[API](../../specs/platform/api-compatibility.md); this card implements it.

## Starting point (what C3 left)

- `crates/canopy-api` — `bes-canopy-api`, `publish = false`, manifest `version = "0.1.0"`
  hand-written, and `info.version` in `crates/public-server/src/openapi.rs` is `""`.
- The codegen does not yet stamp the crate version from `info.version`, and records no
  document digest. Both belong to "the crate inherits the version" and land here.
- `cd.yml` triggers on every push to `main`: arm64 binaries, container image to ghcr, then
  Pulumi against the ops repo. No crate publish, no release job.
- `ci.yml` has a `generated` job (`just check-generated`, `just check-api-deps`) but nothing
  runs `cargo-semver-checks`.

## The version lives in one place and the manifest is written from it

`just gen-api` stamps `version` in `crates/canopy-api/Cargo.toml` from `openapi.json`'s
`info.version`, so the manifest is an output of generation rather than a second place the
number is kept. `check-generated` grows `crates/canopy-api/Cargo.toml` in its diff list,
which is what actually holds the invariant: a hand-edited manifest version fails CI, because
regenerating rewrites it from the document.

`info.version` stops being `""` and carries the real version. Commit 9c9a5f8b emptied it on
the grounds that crate versions here are frozen and never bump, which stops being true with
this card.

There is no cycle, because generation does not read the version to decide what to emit.

## Release flow: release-plz computes, `openapi.rs` records

release-plz keeps the parts that are tedious to hand-roll — changelog, release PR, tag,
GitHub release, publish — and its version *decision* is propagated into `openapi.rs` rather
than left in the manifest.

The complication is that release-plz has no hook at the release-PR stage: the config
reference exposes no `version_files`, no post-bump command, and nothing that runs between
computing the version and opening the PR. The only "run a command" knobs are changelog
pre/postprocessors.

So the propagation is not a release-plz hook. It runs as further steps of the same
`release-pr` job, after the release-plz step, which is a pattern the docs already describe
for pushing an extra commit into the release PR: take the PR from the action's `pr` output,
`gh pr checkout` it, edit, and push (checkout needs `persist-credentials: true`).

1. release-plz computes the bump and writes it to `crates/canopy-api/Cargo.toml`, its native
   behaviour, which we do not fight.
2. The following steps read that number back out — or take it from
   `fromJSON(steps.release-plz.outputs.pr).releases[0].version` — write it into the
   `info.version` literal in `crates/public-server/src/openapi.rs`, run
   `just gen-openapi && just gen-api`, and push the result onto the PR branch.
3. Regeneration rewrites the manifest version from the document — the same number, so it
   settles — and refreshes `openapi.json` and `generated.rs` in the same commit. The release
   PR is therefore internally consistent and `check-generated` would be green on it, instead
   of the document going stale the moment a release lands.

Doing it in-job rather than in a separate workflow matters because of the token rule below:
a workflow watching for the release PR would never fire, since release-plz opened that PR
with `GITHUB_TOKEN`.

The loop is stable across releases: the manifest release-plz reads was itself written from
`openapi.rs`, so it computes against the right baseline every time.

This keeps `openapi.rs` as the place the version is declared and the manifest derived from
it, per APIC, while the number is still settled after the change it describes is final.

## The compatibility gate is a version-sufficiency check

`cargo-semver-checks` derives the release type from the manifest version against the
published baseline and passes when that bump already covers what it found. Where the
manifest version equals the published one it assumes a patch bump, so a break fails.

That gives the gate its shape without an escape hatch, and it is the same shape the API spec
already describes:

- A compatible change needs no version edit in the feature PR. The gate sees no break and
  passes, and release-plz raises the minor or patch at release.
- A coordinated break raises the major in `info.version` **in the coordinating PR**. The
  gate then sees a sufficient bump and passes, and release-plz releases the major that is
  already there.

So the major raise happens in the PR that does the coordinating, which is where API says the
coordination gets recorded. An uncoordinated break is a red CI job rather than a report from
the field.

The gate needs a published baseline, so it cannot run on this card's own PR. It only becomes
meaningful after the bootstrap publish below.

## Bootstrap: a dummy publish out-of-band

The name is reserved by publishing a `0.0.0` stub by hand, carrying the right name and
metadata (`repository`, `license`, `description`) and nothing else. crates.io requires a
published crate before a trusted publisher can be configured against it, so this is the one
manual publish.

The stub is built in a scratch directory rather than by momentarily flipping this repo's
crate, so the repo never holds a `0.0.0` state and nothing has to be reverted afterwards.

With the name reserved, the trusted publisher is configured on crates.io against the repo,
the release workflow's filename, and a GitHub environment, and CI publishes tokenlessly from
then on. release-plz does the OIDC exchange itself, so `rust-lang/crates-io-auth-action` is
not needed and `CARGO_REGISTRY_TOKEN` goes unset; the release job just needs
`id-token: write` alongside its `contents: write` and `pull-requests: read`.

Deciding the workflow filename and environment name is part of this, since both are baked
into the crates.io trusted-publisher configuration and changing them later means editing it.

## Everything moves behind the release

A plain merge to `main` stops deploying. Binaries, image, and Pulumi all move behind the
release, so `main` is a merge target and a release is the only thing that reaches prod. If a
dev or staging environment later wants continuous deployment from `main`, that is a
deliberate move back rather than the default.

`release_always = false` is the release-plz side of this: it releases only when the merge
commit belongs to a `release-plz-` PR.

## The deploy hangs off the release job's output, not off the tag

A tag or release created with `GITHUB_TOKEN` does not start another workflow run, so a
tag-triggered or `on: release` deploy would sit there doing nothing. release-plz's own
guidance for this case is to use the action's outputs rather than a privileged token, which
is what we do: the release job exposes `releases_created` and `releases[0].version` /
`.tag`, and the deploy runs gated on `releases_created == 'true'`, as a reusable workflow the
release job calls. Ordering is then explicit and no extra credential exists to leak.

The release job itself is still `on: push: branches: [main]` and fires normally, because the
release PR is merged by a person and that push is not a `GITHUB_TOKEN` event. It is
release-plz's *own* artefacts, the PR and the tag, that trigger nothing.

`cd.yml`'s existing jobs are the deploy's body and mostly move across unchanged. The image
tag is currently `sha-<short sha>`; with releases as the unit of deployment there is now a
version to tag with instead, which is worth taking.

## Conventions the new workflows follow

The release-plz quickstart uses `dtolnay/rust-toolchain` and `actions/checkout@v6`; this repo
installs the toolchain inline with rustup and pins actions to full versions
(`actions/checkout@v7.0.1`). The new workflows match the repo, not the quickstart.

## Open

- **Whether CI runs on the release PR.** With `GITHUB_TOKEN` it does not, so the release PR
  would merge unchecked. That matters more than usual here, because the release PR is the
  one place carrying a regenerated `openapi.json`, `generated.rs`, and manifest that no CI
  run has ever seen, and merging it is what publishes and deploys. Running CI on it needs a
  GitHub App token or a PAT passed to both `actions/checkout` and the release-plz step; a
  GitHub App keeps the author as a bot and stays scoped and revocable. The alternative is to
  accept it and rely on `cargo publish`'s own verification after merge.
- **Getting the first real release to `1.0.0`.** release-plz computes the next version from
  the current manifest version via the `next_version` crate, so from a published `0.0.0` it
  gives a `0.x`; and with `release_always = false` the first release still has to come
  through a release PR, so this card's own merge will not publish. Whether release-plz
  honours a `1.0.0` already written into `openapi.rs` (and so into the manifest) rather than
  bumping past it needs trying against the tool. The fallback costs one manual step: edit
  the version in the first release PR, which the FAQ documents as supported, by hand or with
  `release-plz set-version`.
