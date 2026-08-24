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

## Identity over QUIC — the existing device key, not a pinned throwaway cert

**This revises H1 and the card description**, which had the relay carry a throwaway certificate and be authenticated by its tailnet peer tag, with SPKI pinning as optional hardening. Canopy already has the concept those were reaching for: a device with a **device key** (spec `DPK`, provisioned credentials). Adopting it for the QUIC connection answers identity, differentiation between relays, and enrollment in one move, and adds no new mechanism.

### How it works

The relay presents a **client certificate carrying its device key** in the QUIC TLS handshake. Canopy reads the peer certificate's `subject_pki.raw`, looks it up with `Device::from_key`, and checks the resulting device carries the `relay` role. That is the same SPKI lookup the HTTP mTLS path performs (`device_auth/mtls.rs`), against the same `device_keys.key_data` column, so the two paths authenticate the same way against one store.

`keygen::generate_device_key` already mints exactly this: a P-256 keypair whose SPKI is derived *by self-signing a throwaway certificate and reading `subject_pki.raw`* — the same operation the relay's TLS stack will perform. So the bytes stored at provisioning are guaranteed to match what the QUIC handshake presents, by construction rather than by care.

### It is stronger here than on the HTTP path

Worth recording, because it inverts the usual expectation. The HTTP mTLS path carries a documented weakness (`mtls.rs`, on `ClientCertHeader`): TLS is terminated at an ingress proxy, so authentication rests on a header, there is **no proof of possession**, and a public key is not a secret — hence the whole single-trusted-header apparatus guarding against a caller pasting in an enrolled device's certificate.

Over QUIC, Canopy terminates TLS itself. The handshake *is* proof of possession: the relay must sign with the private key. No proxy, no header, nothing to spoof. The QUIC path gets for free the property the HTTP path has to work to approximate.

### Enrollment falls out of it

The DTR gap closes with no new path. An admin uses the existing **create-device workflow** with `role = relay`, Canopy mints the keypair and hands back the private key once, and that key is installed into the cluster as a Secret for the relay's deployment to read. Nothing authenticates before an operator has acted, and the relay is created, tracked, and revoked exactly as any other device is — which is what `K8S` already says of it.

Revocation is likewise the existing path: deactivate the key (`is_active`), and `Device::from_key` stops resolving it.

### Consequences

- **SPKI pinning as a separate question dissolves.** The device key *is* the pin, and it is a first-class record with a provisioning workflow behind it rather than a fingerprint captured at enrollment.
- **Relays differentiate by device key.** Each cluster's relay holds its own, so the connection maps to a cluster through the device row — the lookup the connection registry needs, and never a claim the relay makes in a message.
- **Tailnet identity is no longer load-bearing for authentication.** The tailnet still provides the network path, confidentiality, and reachability control, and its ACL remains organised in the tailnet and out of Canopy's concern. But Canopy no longer needs the tailnet directory lookup on this path at all, so no refactor of `tailnet::resolve` is required and the QUIC listener does not depend on the Tailscale control-plane API being up.

### Implementation notes

- Canopy's rustls server config must **request a client certificate and accept any** (there is no CA and no chain to validate), then Canopy inspects the presented certificate itself and does the SPKI lookup. That is a custom `ClientCertVerifier`; the verifier is deliberately not the gate, the device-key lookup is.
- `Device::add_key` refuses a second active key on a device that already carries a different one, so relay key rotation is deactivate-then-add rather than overlap-then-cut. Fine at this fleet size; note it, because it means a rotation has a window where the relay cannot reconnect.
- The relay's own verification of *Canopy's* server certificate is the remaining minor choice: skip it and rest on the tailnet for that direction, or pin. Skipping matches the original reasoning and is the default unless there is a reason to do otherwise.

## Open — to decide on this card

1. **Deployment & versioning** — who deploys the relay and how it's versioned against Canopy.
2. **Canopy's own cluster** — read through a relay like any other, or direct in-cluster reads with a widened ClusterRole.

## Spec impact to carry back

- **`DTR`** (`private-server/device-trust.md`) — the relay's creation path is the existing provisioned-credential workflow at `role = relay`, which closes the gap B1 flagged as unspecified. Check whether DTR's "how a device comes to exist" list needs the relay naming explicitly or already covers it.
- **`K8S`** stays accurate as written: it says a relay is enrolled as a device carrying the relay role and is created, authenticated, tracked, and revoked as any other device is. The device-key decision realises that sentence rather than changing it.

## Decisions

_(captured as they land)_
