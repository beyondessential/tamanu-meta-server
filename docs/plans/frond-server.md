# frond-server: QUIC server for canopy

A new internet-facing server that speaks a custom application protocol over raw QUIC, deployed alongside the existing public/private servers and sharing the same Postgres.

## Why

We need a custom application protocol over QUIC for devices. Two architectural facts force this into a separate binary rather than an extension of public-server:

- The Envoy/Envoy-Gateway fabric in front of the cluster cannot proxy WebTransport or raw QUIC usefully (verified May 2026: Envoy issues #41981, #42221, #40229; no other production-viable proxy implements WT either). So QUIC has to terminate in the application.
- AWS NLB shipped QUIC passthrough mode with QUIC-LB Plaintext-CID routing (Nov 2025). With the AWS Load Balancer Controller injecting `AWS_LBC_QUIC_SERVER_ID`, we can horizontally scale a quinn-based server while preserving connection migration — without joining the Cilium-on-EKS adventure.

This server is not a replacement for public-server. It is a sibling, with its own deployment, its own port, its own auth path.

## What

A new binary `frond-server` that:

- Listens on a configurable UDP port for QUIC (default proposal: 4433).
- Negotiates a single ALPN: `bes.canopy/1`.
- Authenticates clients with mTLS using **bare SPKI** (RFC 7250 raw public keys). For now: a single hardcoded allowed client SPKI (the contents of `identity.pub.pem` at the repo root). Wiring this to `device_keys` / `Device::from_key` is deferred — call out as a TODO.
- Generates QUIC connection IDs in QUIC-LB Plaintext format, embedding the per-pod 8-byte server ID from `AWS_LBC_QUIC_SERVER_ID` (base64-decoded) when present, falling back to a random ID for local dev. Single code path, env-var-as-parameter.
- Speaks the application protocol on top of QUIC streams. **Protocol semantics (messages, framing, multiplexing) are deliberately out of scope for this plan and will be covered by a follow-up.**

## Crate layout

A new workspace member `crates/frond-server/`. Reuses `commons-errors`, `commons-types`, and `commons-servers::health` for the HTTP health sidecar (Phase 5). No `database` dependency until the SPKI lookup is wired up post-MVP.

Sketch of `Cargo.toml`:

```toml
[package]
publish = false
name = "frond-server"
version = "0.1.0"
edition = "2024"
license = "GPL-3.0-or-later"

[[bin]]
name = "frond-server"
required-features = ["cli"]

[dependencies]
quinn = "0.11"
rustls = { version = "0.23", default-features = false, features = ["ring", "std"] }
rustls-pemfile = "2"
clap = { workspace = true, features = ["derive", "env"], optional = true }
commons-errors = { path = "../commons-errors" }
commons-servers = { path = "../commons-servers" }
commons-types = { path = "../commons-types" }
tokio = { workspace = true, features = ["macros", "rt-multi-thread", "signal"] }
tracing.workspace = true
lloggs = { workspace = true, optional = true, features = ["miette-7"] }
miette = { workspace = true, optional = true, features = ["fancy"] }
serde = { workspace = true, features = ["derive"] }
base64 = "0.22"

[features]
default = ["cli"]
cli = ["dep:clap", "dep:lloggs", "dep:miette"]
```

## Phases

Each phase is a commit (or a small contiguous group of commits, per "commit as you write"). Plan→implement→unplan when all phases land.

### Phase 0 — META_LOG → CANOPY_LOG rename

Repo-wide chore preceding frond-server work, since the new binary should use `CANOPY_LOG` from day one and we want a single rename rather than a mixed naming scheme.

- Replace `META_LOG` with `CANOPY_LOG` across the workspace (public-server, private-server, jobs, anywhere else it appears).
- Update Helm/manifest hand-off notes if any are tracked here.
- Single commit `chore: rename META_LOG to CANOPY_LOG`.

### Phase 1 — scaffold

- Add `crates/frond-server/` to workspace `members`.
- Empty `lib.rs`, minimal `main.rs` that parses args (port, bind, logging via `CANOPY_LOG`) and exits.
- `just watch-frond` recipe analogous to `watch-public`.
- `cargo check` passes.

### Phase 2 — QUIC listener with ALPN, throwaway identity

- Wire `quinn::Endpoint::server` with a freshly-generated server keypair at process start. **TODO marker:** persist this in the database and load on startup; deferred to a follow-up plan.
- ALPN: `bes.canopy/1` only (reject everything else).
- Bind: same shape as public-server (`PORT` env, `BIND_ADDRESS` env, default `7899`, IPv6 localhost in dev).
- Accept loop: log peer, ALPN, then close.
- Smoke-test integration test in `tests/connect.rs` using a quinn client in-process.

### Phase 3 — QUIC-LB Plaintext CID generator

- Implement a custom `quinn_proto::ConnectionIdGenerator` that emits 16-byte CIDs in QUIC-LB Plaintext format:
  - byte 0: config rotation byte per draft-ietf-quic-load-balancers §5.2
  - bytes 1–8: 8-byte server ID
  - bytes 9–15: 7 random bytes (matches AWS LBC's `nonce_length_bytes: 7`)
- Read `AWS_LBC_QUIC_SERVER_ID` env var, base64-decode, expect 8 bytes.
- Fallback: 8 random bytes at startup if env var absent. Same code path either way.
- Unit tests: encoding shape, server-ID extraction round-trip, random fallback.
- **Pre-spike before coding:** (a) check whether a `quic-lb` quinn-ecosystem crate already exists; if so, use it; (b) read draft-ietf-quic-load-balancers §5.2 to nail down the exact bit layout of byte 0 (config rotation bits + length encoding), since that's the load-bearing detail AWS will decode against.

### Phase 4 — bare SPKI mTLS

- Configure rustls with `RawPublicKey` certificate type (rustls 0.23+).
- **Server identity:** generated fresh at process start (Phase 2 already does this). TODO marker for database-backed persistence.
- **Client verification:**
  - Custom `ClientCertVerifier` extracting the client SPKI bytes.
  - For now: a single hardcoded allowed SPKI, embedded as a `const` in the source — the contents of `identity.pub.pem` from the repo root, decoded to its raw SPKI byte form at compile time (or `LazyLock` at runtime if compile-time decoding is awkward).
  - Reject any client whose SPKI doesn't match. No auto-create, no DB lookup.
  - **TODO:** swap to `Device::from_key` lookup once the database wiring lands. The client-cert-verifier seam is the only place that changes.

### Phase 5 — graceful shutdown + HTTP health sidecar

- SIGTERM handler: stop accepting new connections, signal connection close to peers, `Endpoint::wait_idle` with a deadline (default 60s, `SHUTDOWN_GRACE_SECONDS` env).
- Sibling HTTP/1 listener on a separate port (`HEALTH_PORT` env, default TBD — pick something near 7899 that doesn't clash, e.g. 7900) running `commons_servers::health::routes()` — `/livez` and `/healthz` already exist. NLB's HTTP/TCP target group health-checks point at this port. This is required because UDP target groups don't support UDP-level checks.

### Phase 6 — observability

- `tracing` spans per connection: peer IP, device_id, ALPN, CID prefix.
- A few counters: active connections, accepted, rejected, handshake errors.
- Match the lloggs/miette setup public-server uses (`PreArgs::parse_with_env`).

### Phase 7 — release plumbing

- `release.toml` mirroring public-server's.
- Whatever CI step builds release binaries needs to be updated; assumed external to this repo (confirm).

### Phase 8 — application protocol [DEFERRED]

Wire-protocol semantics for `bes.canopy/1` are explicitly out of scope here. Once frond-server can accept-auth-and-disconnect cleanly, a separate plan defines what messages flow over the streams.

## Deployment requirements (out-of-repo)

Deployment manifests don't live in this repo, but the contract frond-server expects from the cluster is:

- **NLB QUIC listener** on the chosen port:
  - `service.beta.kubernetes.io/aws-load-balancer-type: external`
  - `service.beta.kubernetes.io/aws-load-balancer-nlb-target-type: ip`
  - `service.beta.kubernetes.io/aws-load-balancer-quic-enabled-ports: "<port>"`
  - NLB has **no security groups** (AWS QUIC limitation).
  - **No IPv6 target group** (AWS QUIC limitation). IPv4 only on the data path.
- **CID injection:**
  - Namespace label `elbv2.k8s.aws/quic-server-id-inject=enabled`.
  - Pod annotation `service.beta.kubernetes.io/aws-load-balancer-quic-enabled-containers: frond-server`.
  - LBC version must support server-ID injection — confirm cluster's LBC version before deployment.
- **HTTP health target group** registered against the sidecar port, checking `/livez`.
- `terminationGracePeriodSeconds: 90` (drain window > Phase 5 grace).
- `PodDisruptionBudget` with `maxUnavailable: 1`.
- QUIC v1 only — quinn defaults match.

## Local dev

- `just watch-frond` runs the binary against `127.0.0.1:7899` (or `[::1]`) with a freshly-generated server keypair each restart (no persistence yet).
- `tests/connect.rs` integration test: spin up server + quinn client in-process, complete handshake using the hardcoded `identity.pub.pem` SPKI as the client identity, exchange a ping.
- No `AWS_LBC_QUIC_SERVER_ID` → random 8-byte server ID. Behaviour identical to prod, just routes-to-itself.

## Settled decisions

- **QUIC port:** 7899.
- **Health port:** TBD near 7899 (e.g. 7900); HTTP-style with `/livez`/`/healthz` via `commons_servers::health::routes()`.
- **Unknown SPKIs are rejected.** No auto-create. (Public-server may shift to the same posture in a separate change.)
- **Client allowlist:** single hardcoded SPKI from `identity.pub.pem` for now, deferred to database-backed lookup.
- **Server identity:** generated fresh at process start, no persistence yet, marked TODO for database-backed storage.
- **Log env var:** `CANOPY_LOG` everywhere (Phase 0 renames the existing `META_LOG` usages too).

## Risks

- `quinn_proto::ConnectionIdGenerator` may constrain CID shape in ways that don't match the QUIC-LB plaintext format. Mitigation: spike Phase 3 first — if quinn restricts us, escalate before doing more work.
- rustls `RawPublicKey` support is recent; the `ClientCertVerifier` API may not surface raw SPKI bytes cleanly. Mitigation: prototype Phase 4 with a no-op verifier, layer auth on top once the wire path works.
- AWS may change the QUIC-LB draft version it tracks. The risk is small (AWS docs say "stable for several months") but the encoding is the contract.

## Out of scope

- HTTP/3, WebTransport.
- Application protocol design (separate plan).
- Migrating any existing public-server traffic to QUIC.
- Multi-region / multi-cluster QUIC steering.
