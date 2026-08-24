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

## Open — to decide on this card

1. **Code & deployable structure** — where the relay lives as a crate/binary, and where Canopy's connection-owning worker lives.
2. **Protocol design** — stream/message framing over QUIC, request/response for queries and commands, and the version-negotiation scheme.
3. **Canopy-side connection ownership** — the singleton worker that holds relay connections and gets the sidecar; how filings hand off to the device-push ingestion path.
4. **Identity over QUIC** — adapting the tailnet resolve to take the address from the connection; whether to pin the relay's SPKI fingerprint at enrollment.
5. **Relay enrollment** — how the relay's device row comes to exist (DTR gap), which follows from pinning and from who deploys.
6. **Deployment & versioning** — who deploys the relay (ops/Pulumi) and how it's versioned against Canopy.
7. **Canopy's own cluster** — read through a relay like any other, or direct in-cluster reads with a widened ClusterRole.

## Decisions

_(captured as they land)_
