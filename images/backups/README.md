# backups scheduler image

The image for canopy's long-lived **backups** control-plane Deployment: the
kopia CLI plus the canopy `backups` binary (`crates/jobs`, `[[bin]] backups`).

The `backups` bin runs the maintenance / inspection / upstream-preflight /
S3-metrics loops, and drives **kopia in-process** (subprocesses) against each
backup-configured group's per-bucket S3 repository (one kopia repo per group).
There are **no** separate one-shot Kubernetes Jobs and no result round-trip —
the bin runs kopia, parses its `--json` output, and writes results straight to
the database. Collapsing the old per-op Jobs into this one process loses nothing
(every Job already shared the one `canopy-jobs` IRSA identity), and a long-lived
process lets kopia hold a refreshing per-group credential.

## What it does each tick

Per backup-configured group, gated by hash-jittered cadence + a concurrency cap
(one op per group at a time):

- **init** (groups in `provisioning`) — create the repo (idempotent), apply
  retention, set canopy as the maintenance owner, disable client-side
  automatic maintenance.
- **maint-quick / maint-full** (`ready` groups, daily / weekly) — assert
  per-`(group,type)` retention per source, expire + delete snapshots, run
  (quick or full) maintenance.
- **inspect** (`ready` groups) — read-only: list snapshots (latest per source),
  gather repo stats, run an integrity verify; writes ground-truth inventory and
  raises the corruption alert on a verify failure.

## Auth model

The pod runs under the IRSA-annotated `canopy-jobs` ServiceAccount, so AWS
injects web-identity (`AWS_WEB_IDENTITY_TOKEN_FILE` / `AWS_ROLE_ARN`). For each
kopia subprocess the bin **overrides `AWS_ROLE_ARN`** to that group's per-bucket
`target_role_arn` (keeping the projected token), so kopia's own AWS SDK does
`AssumeRoleWithWebIdentity` against the per-bucket role **directly** — not
chained, so the session can run up to the role's `MaxSessionDuration` and
auto-refreshes as the projected token rotates (no 1-hour cap that would break a
long maintenance run). The per-bucket role must trust the `canopy-jobs` SA's
OIDC subject (ops `backups`-stack). The repo passphrase is read from the group's
k8s Secret (`repo_password_ref`) and passed to kopia via `KOPIA_PASSWORD`.

## Build

Multi-stage build that compiles the `backups` binary from the canopy workspace,
so **the build context must be the repository root**:

```sh
podman build -f images/backups/Dockerfile -t canopy-backups .
```

Pinned to `kopia/kopia:0.23.1` (base) and `rust:1-bookworm` (build stage,
glibc-compatible with the kopia base). Bump deliberately; don't float to
`:latest`.

## Where this lives

This image lives in the canopy repo (it bundles the canopy `backups` binary, so
it's canopy-specific rather than a generic kopia image). The kopia **base** is a
third-party artifact pinned by tag.
