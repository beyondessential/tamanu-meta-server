# The `backups` scheduler image: kopia + the canopy `backups` binary.
#
# This is the long-lived backups control-plane Deployment. The `backups` bin
# (crates/jobs, `[[bin]] backups`) runs the maintenance / inspection / preflight
# / s3-metrics loops, and drives kopia **in-process** (subprocesses) against each
# group's per-bucket S3 repository — so this image bundles the kopia CLI. There
# are no separate one-shot Kubernetes Jobs.
#
# ---------------------------------------------------------------------------
# Cross-account auth model (IRSA web-identity -> per-bucket assume-role)
# ---------------------------------------------------------------------------
# The pod runs under the `canopy-jobs` ServiceAccount, annotated for IRSA, so
# the AWS SDK sees the injected web-identity:
#   AWS_WEB_IDENTITY_TOKEN_FILE  - projected service-account token path
#   AWS_ROLE_ARN                 - the cluster-side IRSA role
# For each kopia subprocess the `backups` bin OVERRIDES `AWS_ROLE_ARN` to that
# group's per-bucket `target_role_arn` (keeping the projected token), so kopia's
# own AWS SDK does `AssumeRoleWithWebIdentity` against the per-bucket role
# DIRECTLY (not chained → up to the role's MaxSessionDuration, and auto-refreshed
# as the projected token rotates — no 1h cap). The per-bucket role must trust the
# `canopy-jobs` SA's OIDC subject (ops `backups`-stack).
#
# Build context is the repository root (so cargo can see the workspace):
#   podman build -f images/backups/Dockerfile -t canopy-backups .

# Build the backups binary from the canopy workspace.
FROM docker.io/rust:1-bookworm AS build
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release -p jobs --bin backups

# kopia base provides the kopia CLI; we add only our binary (glibc-compatible
# since both are Debian bookworm). No apt, no aws-cli.
FROM docker.io/kopia/kopia:0.23.1
COPY --from=build /src/target/release/backups /usr/local/bin/backups
ENTRYPOINT ["/usr/local/bin/backups"]
