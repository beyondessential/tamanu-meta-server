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

## Protocol — a stream per exchange

Decided. QUIC streams are cheap and independently delivered, so **the stream is the correlation**: no request-ID bookkeeping, no multiplexing layer, and a cancelled request is just a reset stream. A slow namespace-roster query cannot stall a queue of filings behind it, which a single multiplexed stream would allow.

- **Filings go up as unidirectional streams**, opened by the relay: open, write, close. No response body.
- **Queries and commands are bidirectional streams**, opened by Canopy: the five exchanges K2 settled (namespace roster, connected-and-answering, embedded suite version; sleep, wake).
- Messages are length-delimited, with the message enum living in `relay-protocol`.

### Filings are unacknowledged

Accepted deliberately. The relay gets QUIC's delivery guarantee but no application-level acknowledgement that a filing was *ingested*, so a filing Canopy accepts on the wire and then fails to ingest is lost until the next refile. **The periodic refile is the reconciliation mechanism** — it already exists to survive a missed event, a restart, or a reconnection (K2's cadence), so per-filing acks would be redundant machinery covering a window the refile already closes. Worth revisiting only if the refile interval ever grows long enough that a lost filing matters in the gap.

### Version negotiation rides on ALPN

The protocol version is carried in the **QUIC ALPN token** (`canopy-relay/1` and successors). Canopy offers the range it supports and the relay picks, so an incompatible pair fails at the TLS handshake with a clear "no application protocol" rather than connecting and then failing to parse a message. Protocol versioning is present from the first release, since the relay is a second deployable running vN against Canopy vM.

Detail that is not protocol-breaking rides in a post-handshake control message instead: the embedded check-suite version for SELF's skew alert, and the relay's build info.

## Filing handover — hoist the ingestion core, two filing shapes

K2 requires filings to converge on the ingestion path a device push takes, so parity is not re-derived on Canopy's side. Today that path is `file_health_events`, a **private function in `crates/public-server/src/statuses.rs`** reachable only through the axum handler. The relay speaks QUIC, not HTTP, and `jobs` depending on `public-server` would invert the dependency direction.

### Hoist the push ingestion into `commons-servers`

Decided. Move `file_health_events`, `collect_check_results`, `split_health_from_extra`, and the `StatusPush`/`HealthCheck` types out of `public-server` into `commons-servers`, which already reaches `issues::file_check` from `tailnet_sweeps.rs`. The HTTP handler and the relay connection worker then become two thin callers of one ingestion core.

The alternative — re-implementing the conversion in `jobs` — produces exactly the drift G2 identified as the one real risk to parity ("the crate already builds its payload through the same serialisation a pushed bestool uses; the one real risk is Canopy re-modelling the filing on its side"). The refactor touches shipped code on the status-push path, so it carries regression risk of its own and wants the existing status-push tests kept green across the move.

### Two filing message types, split by family

The two check families do not share a filing shape, and forcing them into one envelope would mean a lowest-common-denominator type that fits neither.

- **Harvest filings (`alertd` source)** are per-server, and the relay already produces the payload a pushed bestool produces. So the filing message **is the status-push body** rather than a re-modelled filing type, fed straight into the hoisted ingestion. Parity becomes structural rather than something maintained.
- **Substrate filings (`kubernetes` source)** scope to a server, to a server group (a namespace), or Canopy-wide with each cluster an instance, and the source is reserved from the device API — so there is no push analogue to converge on. These construct `CheckFiling` / `InstancedCheckFiling` and go through `issues::file_check` directly.

Both carry `Scope` from the single `database::issues::Scope` enum; the relay never hand-rolls a server/group discriminator of its own.

## Open — to decide on this card

1. **Identity over QUIC** — adapting the tailnet resolve to take the address from the connection; whether to pin the relay's SPKI fingerprint at enrollment.
2. **Relay enrollment** — how the relay's device row comes to exist (DTR gap), which follows from pinning and from who deploys.
3. **Deployment & versioning** — who deploys the relay and how it's versioned against Canopy.
4. **Canopy's own cluster** — read through a relay like any other, or direct in-cluster reads with a widened ClusterRole.

## Decisions

_(captured as they land)_
