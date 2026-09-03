# Stand up crate publish pipeline for canopy

Take `bes-canopy-api` from marked-publishable-but-never-published (as C3 landed it) to
actually published, with the version model, trusted-publishing pipeline, compatibility gate,
and prod-deploy gate the card calls for. The versioning model is settled in
[APIC](../../specs/platform/api-client-crate.md) and
[API](../../specs/platform/api-compatibility.md); this card implements it.

## Starting point (what C3 left)

- `crates/canopy-api` — `bes-canopy-api`, already `publish = true` with manifest
  `version = "0.0.0"` as a deliberate placeholder, and `info.version` in
  `crates/public-server/src/openapi.rs` is `""`. Publishability is a property of the crate
  rather than a record of whether a pipeline exists, so C3 set it; nothing in the repository
  publishes it, which is what this card supplies.
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

It stays in-job rather than becoming a separate workflow watching the release PR. With the
PAT below that separate workflow would in fact fire, so the reason is not that it cannot
work: it is that the PR would then be observably wrong between being opened and being
corrected, with CI racing the fix. In-job, the propagation is part of producing the PR.

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

## Bootstrap: one manual publish of the crate as it stands

crates.io requires a published crate before a trusted publisher can be configured against
it, so the name is reserved by one manual publish. That publish is `cargo publish` from
`crates/canopy-api` as it already stands: C3 left it `publish = true` at `version = "0.0.0"`,
which is exactly the state the first publish wants, so there is nothing to flip and nothing
to revert.

Publishing the real crate rather than a hand-built stub also makes the compatibility gate
useful a release earlier. The baseline on crates.io is then the crate's actual surface, so
the first comparison judges something; against an empty stub every lint would pass
vacuously.

**Done.** `0.0.0` is published and the trusted publisher is configured.

Taking the version from the repository did put the bootstrap in a sequence, where the
out-of-band stub had none: the `0.0.0` had to reach crates.io while the manifest still read
`0.0.0`, and so before the version-stamping change rewrites it from `info.version`. That
ordering held, so the constraint is spent rather than outstanding, and the card is free to
land whatever version stamping decides.

With the name reserved, the trusted publisher is configured on crates.io and CI publishes
tokenlessly from then on. release-plz does the OIDC exchange itself, so
`rust-lang/crates-io-auth-action` is not needed and `CARGO_REGISTRY_TOKEN` goes unset; the
release job just needs `id-token: write` alongside its `contents: write` and
`pull-requests: read`.

The configuration binds the owner, the repository, the workflow filename, and optionally a
GitHub Actions environment. The workflow is `release.yml`, and the environment is `release`,
which is the name the RFC's own example uses.

`release` rather than `prod` because the boundary covers both of a release's destinations,
crates.io and the cluster, and publishing a crate is not a deployment. Naming it after one
destination would send anyone looking for where the publish is gated to the wrong place, and
it leaves `prod` free for the deploy side if a staging environment ever wants the
distinction.

The environment earns its place through branch restriction and secret scoping rather than
approvals: a token cannot be obtained for a run outside it, so a publish cannot be driven
from a branch, and the PAT plus the deploy's existing credentials have a scoped home. No
required reviewers to begin with, since merging the release PR is already the human decision
and a second approval would only add a step. Reviewers can be added later without touching
the crates.io configuration.

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

## A PAT, so the release PR is checked

release-plz runs under a PAT rather than `GITHUB_TOKEN`, passed to both `actions/checkout`
and the release-plz step, so the release PR it opens triggers CI. That matters more here
than it usually would: the release PR is the one place carrying a regenerated
`openapi.json`, `generated.rs`, and manifest that no CI run has ever seen, and merging it is
what publishes and deploys.

A PAT makes its owner the author of the release PR and its commits, so a machine user is
worth having rather than attributing releases to whoever created the token. A GitHub App
would avoid that, at the cost of App setup; the PAT is the lighter route and the choice is
reversible without touching crates.io.

The PAT does not change how the deploy is triggered. Outputs still drive that, because it is
ordering within one workflow rather than a cross-workflow event, and it needs no credential.

### `RELEASE_PLZ_TOKEN`, repository-wide

The secret is `RELEASE_PLZ_TOKEN`, the name release-plz's own docs use, so anyone reading
those docs maps them onto this repo without translating.

It is a repository-wide Actions secret, not a secret of the `release` environment, because
the job that needs it is not the publishing job. The PAT is read by `release-pr`, which
opens the PR and pushes the propagation commit; the environment exists for `release`, which
publishes. Scoping the PAT to that environment would force `release-pr` to declare it, which
conflates opening a PR with releasing and sets a trap: adding required reviewers later to
gate publishing would also gate PR creation.

The blast radius is bounded on the token instead of on who can read the secret. A
fine-grained PAT, scoped to this repository alone, with Contents and Pull requests
read/write and owned by a machine user, grants a reader essentially what write access to
this repo already grants. Environment scoping would buy little against that, and costs
either the conflation above or a second environment whose only purpose is holding one
secret.

### The release PR is briefly inconsistent, and that is tolerable

release-plz opens the PR with the manifest bumped and `openapi.rs` untouched, which is a
state `check-generated` fails, and the propagation commit lands moments later. So CI can
start against a state that is wrong by construction.

`ci.yml`'s concurrency group already sets `cancel-in-progress: true`, so the propagation push
cancels the superseded run and the surviving one judges the corrected PR. Left as is: the
alternative is dropping the manifest version from `check-generated`, which is the one thing
holding the version invariant.

## Build steps

- [x] `info.version` in `crates/public-server/src/openapi.rs` carries `0.0.0`, matching what
      is published, so release-plz computes its first bump from the same number under either
      reading of where it reads the current version from
- [x] The codegen stamps `version` in `crates/canopy-api/Cargo.toml` from the document's
      `info.version`, touching only the `[package]` version and leaving the rest of the
      manifest alone
- [x] The generated source records the document it came from, as APIC requires: its version
      and a digest, so a document that moved without the version moving with it can be told
      from one that did not
- [x] `check-generated` covers `crates/canopy-api/Cargo.toml`, which is what stops the
      manifest version being edited by hand
- [x] `just semver-checks` runs `cargo-semver-checks` against the published baseline
- [ ] A CI job runs it
- [ ] `release-plz.toml` — `release_always = false`, semver checks on, tag name for a
      workspace member
- [ ] `.github/workflows/release.yml` — a `release-pr` job under the PAT that propagates the
      version into `openapi.rs` and regenerates, and a `release` job that publishes through
      trusted publishing in the `release` environment
- [ ] `cd.yml` becomes a reusable workflow the release job calls, and stops triggering on
      pushes to `main`
- [ ] `just check`, `just lint`, `just fmt-check`, and the generated-files checks pass

## What counts as a release, for the deploy

Found while wiring this up, and it decides the shape of `release-plz.toml`.

release-plz works out which packages changed by diffing each local package against the
`.crate` published for it, attributing commits per package, and comparing manifest and
lockfile dependencies. `bes-canopy-api` depends on no workspace-internal crate, so a change
to `private-server` touches neither its files nor its dependencies and does not bump it.

If release-plz manages only `bes-canopy-api`, then, a server-only change produces no release
PR and no release — and with the deploy gated on a release, canopy would stop deploying for
most of what lands. Every change that is not to the API client would sit on `main` forever.

The two versions cannot simply be merged into one, either. APIC has the crate's version
describing the crate's surface, so bumping it for an unrelated server change would republish
an identical surface under a new number and break the correspondence the whole model rests
on.

So a release needs two tracks: the published crate, versioned from the document, and canopy
itself, versioned from its commits and tagged rather than published. The deploy fires on any
release, and the crate publishes only when its own version moved.

Which packages carry the second track is the open question below.

## Open

- **Which packages carry canopy's own release track.** `public-server` and `private-server`
  are what the container runs, so versioning those two as a group would catch everything
  that reaches production: a change to `database` or a `commons-*` crate moves their
  lockfile dependencies, which release-plz counts as a change. They would be `git_only`,
  taking their version from tags rather than the registry, since nothing publishes them.
  That means giving them real versions in place of the frozen `6.6.6` the workspace carries
  today, which is a decision rather than a detail.
- **Getting the first real release to `1.0.0`.** release-plz computes the next version from
  the current manifest version via the `next_version` crate, so from a published `0.0.0` it
  gives a `0.x`; and with `release_always = false` the first release still has to come
  through a release PR, so this card's own merge will not publish. Whether release-plz
  honours a `1.0.0` already written into `openapi.rs` (and so into the manifest) rather than
  bumping past it needs trying against the tool. The fallback costs one manual step: edit
  the version in the first release PR, which the FAQ documents as supported, by hand or with
  `release-plz set-version`.
