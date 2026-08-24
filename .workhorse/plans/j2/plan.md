# In-cluster relay and transport — tech design

The foundational card for Kubernetes monitoring: build the Canopy-authored relay that runs in each cluster and the transport it speaks to Canopy over. K1 (registry), M1 and N1 (the two check families) build on what this card lays down. Verifies spec `K8S` (`monitoring/kubernetes.md`).

This plan is the working record for the technical approach. Architecture inherited from the B1 brainstorm (`plans/b1/plan.md`, "Access to clusters") is treated as settled; this card decides the mechanics B1 deliberately left to the implementation.

## Settled coming in (from B1/H1/G2/K2)

- **One relay per cluster**, Canopy-authored, holding the cluster's RBAC on its own ServiceAccount. It dials outward to Canopy; Canopy never dials the cluster and holds no cluster credential. A registered cluster *is* a relay identity.
- **Transport is QUIC (`quinn`) over Tailscale**, TLS carrying throwaway certificates (WireGuard already provides confidentiality and peer auth). `quinn` is configured onto the workspace's `aws-lc-rs` rustls provider, not the default `ring`.
- **Kernel-mode Tailscale sidecar at both ends** (`TS_USERSPACE=false`, `NET_ADMIN`, TUN) — userspace mode is a TCP-only proxy that QUIC can't pass. Established practice in our infra.
- **Identity** is a device with a `relay` role, associated with no server, authenticated by its tailnet peer tag resolved from the QUIC connection's remote address — the same check the HTTP path already does (`device_auth/tailnet.rs`), taking the address from the connection rather than an extractor. With cert verification skipped, the tailnet ACL + tag are the whole gate.
- **What crosses the connection** (settled by K2): filings (both check families, computed relay-side), three named queries (namespace roster for L1's picker, connected-and-answering handshake for K1, embedded check-suite version for SELF's skew alert), and two commands (sleep/wake). No method returns a Kubernetes object. Filings converge on the same ingestion path a device push takes.
- **The relay embeds `bestool-alertd` as a library** to run the harvest against each instance's local Postgres; only results cross to Canopy.
- **Protocol versioning from the start** — the relay is a second deployable (`vN`) running against Canopy (`vM`).

## Code structure — three crates

Decided. The relay is a canopy-workspace deployable, built and released from this repo alongside the servers, so its protocol contract stays in the same tree as the Canopy side that consumes it.

- **`crates/relay`** — the in-cluster binary. Owns the heavy, relay-only dependency set: `kube` + `k8s-openapi` for the watches and the substrate checks, `bestool-alertd` as a library for the harvest, `quinn` for the client end of the transport. Its own workspace member with a `[[bin]]`, not a bin hung off an existing crate, because it releases on its own cycle.
- **`crates/relay-protocol`** — the shared wire contract: message types, framing, and version negotiation. Depended on by both ends so the two deployables cannot drift on message shape. Carries no `kube` and no `bestool-alertd`; a filing crosses as its serialised payload, not as a Kubernetes object.
- **Canopy-side connection worker** — a new bin in `crates/jobs`, alongside the other long-lived Deployment pods (`backups.rs` is the pattern). It speaks the protocol and hands filings to the ingestion path; it needs no `kube` of its own, which keeps `jobs`' existing isolation of `kube`/`k8s-openapi` intact.

### Many relays, one listener

There is one relay per registered cluster, so the Canopy-side worker is not a client dialling out but a **QUIC listener holding N concurrent inbound connections**, one per cluster. Consequences to carry through the rest of the design:

- The worker keeps a **connection registry keyed by cluster**, which is what K1's connected-and-answering query and SELF's per-cluster connectivity check both read.
- Which cluster a connection belongs to is **derived from the authenticated relay identity**, never claimed by the relay in a message. The device row for the relay is the cluster's registration, so the mapping is a lookup, not a assertion to trust.
- The worker being a singleton means its loss makes every cluster unreadable at once. That surfaces correctly through the existing per-cluster connectivity check (every instance fails), but it is worth naming as a single point of failure the design accepts.

## Open — to decide on this card

1. **Protocol design** — stream/message framing over QUIC, request/response for queries and commands, and the version-negotiation scheme.
2. **Filing handover** — how filings reach the same ingestion path a device push takes, without re-deriving parity on Canopy's side.
3. **Identity over QUIC** — adapting the tailnet resolve to take the address from the connection; whether to pin the relay's SPKI fingerprint at enrollment.
4. **Relay enrollment** — how the relay's device row comes to exist (DTR gap), which follows from pinning and from who deploys.
5. **Deployment & versioning** — who deploys the relay and how it's versioned against Canopy.
6. **Canopy's own cluster** — read through a relay like any other, or direct in-cluster reads with a widened ClusterRole.

## Decisions

_(captured as they land)_
