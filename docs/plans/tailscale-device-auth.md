# Tailscale device auth on the existing `/public/...` mount

Add a second auth path to the device extractor so a tailnet-resident
device can hit the existing `/public/...` endpoints (mounted under
the private-server's Tailscale tunnel) and be identified as its
`Device` row by its Tailscale node identity. mTLS stays additive —
both the internet-facing public-server binary and the tunneled
`/public` nest continue to accept mTLS, and tailnet callers get a
zero-cert path.

## Why

Today the device extractor in
`crates/commons-servers/src/device_auth.rs` only accepts mTLS
(`x-forwarded-client-cert` / `mtls-certificate` / `ssl-client-cert`).
The private server mounts the public-server's router at `/public`
(`crates/private-server/src/lib.rs:17-23`), so calls to
`https://<tunnel-host>/public/events` go through that same mTLS-only
extractor — hence the `401 AuthMissingCertificate` we're seeing in
prod despite the call coming over the tailnet.

The obvious "read Tailscale identity headers" approach hits a wall:
the Operator's ingress proxy sets `Tailscale-User-Login` /
`-User-Name` / `-User-Profile-Pic` only for **logged-in human users**.
For tagged devices those headers are not populated at all. So
header-only auth can't tell one tagged server from another, and a
May-11 sketch (`~/.claude/plans/currently-we-have-a-functional-prism.md`,
not committed) that deferred this exact decision is unusable as-is.

**However**, an empirical test against the Operator's ingress proved
two useful facts:

1. The proxy sets `X-Forwarded-For` to the **calling node's
   Tailscale ULA address** (`fd7a:115c:a1e0::3701:2c8a` in the
   captured request). Tailscale gives every node both a 100.64/10 v4
   and an `fd7a:115c:a1e0::/48` v6 — `axum-client-ip`'s `ClientIp`
   extractor already picks one of them up via the existing
   `ClientIpSource` wiring.
2. Neither prefix is routable on the public internet, so the
   workload pod cannot be tricked into seeing such an address from a
   non-tailnet caller — provided the upstream proxy chain is
   correctly configured (which it already is for the existing mTLS
   header trust).

Combine that with the Tailscale control-plane API
(`GET /api/v2/tailnet/{tailnet}/devices`, which returns every node's
`addresses[]` and `nodeId`), and we can map `ClientIp → Device` in
the extractor with no tsnet binding, no sidecar, no new crate.

## Approach

One auth refactor + one HTTP client + one schema migration. No new
listener, no new binary.

### Extractor: dual-path device auth

Refactor `crates/commons-servers/src/device_auth.rs`:

1. Split into `device_auth/{mod,mtls,tailnet}.rs` (the file is small
   enough today that one file would still be fine, but the dual-path
   logic plus the explicit "skip on no resolver" check earns the
   split).
2. `mtls.rs` keeps the existing cert-parsing logic (lines 64–115 of
   today's file) as `async fn resolve_mtls(parts, db) -> Result<Option<Device>>`.
   Missing header → `Ok(None)`. Malformed header → `Err`. Unchanged
   semantics.
3. `tailnet.rs` exposes `async fn resolve_tailnet(parts, state) -> Result<Option<Device>>`:
   - Extract `ClientIp` via `parts.extract::<ClientIp>().await`.
     `ClientIp` is already populated by the `axum-client-ip`
     middleware that's wired in every server's main.
   - If the IP isn't in `100.64.0.0/10` or `fd7a:115c:a1e0::/48`,
     return `Ok(None)`. This is the spoof guard — neither prefix is
     internet-routable, so the proxy chain can't put one there for
     an internet caller.
   - Look up the IP in `state.tailnet_directory()`. Cache miss →
     return `Ok(None)` (let mTLS try, or fall through to the
     existing `AuthMissingCertificate`).
   - Look up `Device::from_tailscale_node_id(node_id)`. Found →
     return. Missing → auto-create
     `Device { role: Untrusted, tailscale_node_id, tailscale_node_name,
     tailscale_tailnet }` (mirrors the mTLS first-contact path at
     lines 117-127 of today's `device_auth.rs`).
4. `AuthDevice::from_request_parts` (in `mod.rs`) tries
   `resolve_mtls` then `resolve_tailnet`; returns
   `AuthMissingCertificate` only if both yielded `None`. Order
   biases to mTLS to preserve current behaviour for any device
   presenting both.
5. The `device_role_struct!` macro (lines 15-46 today) is unchanged.
   `AdminDevice`, `ServerDevice`, `ReleaserDevice` continue to wrap
   `AuthDevice` and apply role checks — both auth paths benefit
   automatically.
6. `AuthDevice` carries an `auth_method: AuthMethod` (`Mtls |
   Tailnet { node_id }`) for the connection-log row and future
   audit.

### Tailnet directory (HTTP client + cache)

New module `crates/commons-servers/src/tailnet_directory.rs`. A
small read-through cache around the Tailscale REST API.

```rust
#[derive(Clone)]
pub struct TailnetDirectory {
    inner: Arc<RwLock<Inner>>,
    client: reqwest::Client,
    oauth: OAuthClient,    // refreshes bearer tokens
    tailnet: String,       // e.g. "felix-bes.au"
    api_base: Url,         // default https://api.tailscale.com
    refresh_period: Duration,    // default 60s
    miss_cooldown: Duration,     // default 5s
}

pub struct DirectoryEntry {
    pub node_id: String,
    pub node_name: String,
    pub tailnet: String,
    pub tags: Vec<String>,
}

impl TailnetDirectory {
    pub async fn lookup(&self, ip: IpAddr) -> Result<Option<DirectoryEntry>>;
    pub async fn refresh(&self) -> Result<()>;
}
```

Behaviour:

- On construction, kick off a background `tokio::spawn` loop that
  calls `refresh()` every `refresh_period`.
- `lookup(ip)`: hit the in-memory `HashMap<IpAddr, DirectoryEntry>`.
  On hit, return. On miss, optionally call `refresh()` (at most
  once per `miss_cooldown` — guards against a thundering herd from
  unknown IPs), then re-check; return `None` if still missing.
- `refresh()` is a single `GET /api/v2/tailnet/{tailnet}/devices`,
  parses the device list, rebuilds the IP map. Cheap; one Tailscale
  API call per minute is well under the documented rate limits.
- OAuth: the access token has a TTL (~1 hour); the client refreshes
  it lazily on 401 from the device-list call.

Auth to the Tailscale API uses an OAuth client (preferred over a
personal access token):

- `TAILSCALE_OAUTH_CLIENT_ID`
- `TAILSCALE_OAUTH_CLIENT_SECRET`
- `TAILSCALE_TAILNET` (tailnet identifier — typically the email-like
  ID shown in the admin console)
- `TAILSCALE_API_BASE` (optional, for testing against mock or
  self-hosted control planes)

OAuth scope: `devices:core:read`. Narrower than a PAT.

### Per-server state wiring (preserve public-server's mTLS-only invariant)

The Tailscale path **must not** activate on the internet-facing
public-server binary, because its proxy doesn't speak Tailscale and
a misconfigured upstream could feed it spoofed `X-Forwarded-For`
values. The cleanest way to enforce that by construction:

- `commons-servers::device_auth` reads the directory off an axum
  state slot of type `Option<TailnetDirectory>` via `FromRef`.
- Private-server's `AppState` carries `Some(directory)` when env
  vars are present, `None` otherwise (so local dev without
  Tailscale env vars still works).
- Public-server's `AppState` doesn't have the field at all — its
  `FromRef` impl yields `None` constant, so `resolve_tailnet` short
  circuits on `state.tailnet_directory().is_none()`.

That keeps the public-server binary unable to authenticate by
Tailscale identity by **type-system construction**, not just by
configuration. Same trick the May-11 plan used; reused here.

### Per-route gate: reject tagged-device callers from non-`/public` surfaces

The dual-auth path on `/public/...` is the **only** surface that
welcomes tagged devices. Every other route in the private-server's
tree — the `/api/*` admin routes, the SPA fallback, `/api/docs`,
the health endpoint — should refuse them, even if a future Tailscale
change starts populating identity headers in a way that confuses
the existing admin extractors.

Current behaviour is "almost right by accident": `TailscaleAdmin`
requires `Tailscale-User-Login`, which isn't populated for tagged
devices, so admin endpoints already 401. But:

- `crate::spa::handler` (the SPA fallback) serves the embedded
  React bundle to anyone reaching the router. A tagged device
  gets the HTML/JS even though it can't usefully use it.
- `/api/docs/*` (Swagger UI) is similarly unprotected.

Defense in depth: add an axum middleware that classifies the
caller as a tagged device when **all** of the following hold, and
returns 403 if so:

1. `Tailscale-User-Login` header is absent.
2. `ClientIp` (already extracted by axum-client-ip) is in the
   tailnet ranges (`100.64.0.0/10` or `fd7a:115c:a1e0::/48`).

Either of those alone is fine on its own — internet callers
have no Tailscale headers but their IP isn't tailnet, K8s probes
come from pod IPs and also aren't tailnet, logged-in humans on
the tailnet have the user-login header. The combination uniquely
identifies tagged-device callers.

New file `crates/commons-servers/src/tailnet_guard.rs`:

```rust
pub async fn reject_tagged_devices(
    req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    if req.headers().contains_key("Tailscale-User-Login") {
        return Ok(next.run(req).await);
    }
    let client_ip = req.extensions()
        .get::<axum_client_ip::ClientIp>()
        .map(|c| c.0);
    if matches!(client_ip, Some(ip) if is_tailnet_ip(ip)) {
        return Err(AppError::TaggedDeviceNotAllowed);
    }
    Ok(next.run(req).await)
}
```

Wired in `crates/private-server/src/lib.rs:routes()` by splitting
the current single merged router into two: the `/public` nest
(no guard) and everything else (guard applied):

```rust
let non_public = Router::new()
    .merge(commons_servers::health::routes())
    .merge(api_router)
    .merge(SwaggerUi::new("/api/docs").url("/api/openapi.json", api_spec))
    .fallback(spa::handler)
    .layer(middleware::from_fn(
        commons_servers::tailnet_guard::reject_tagged_devices,
    ));

Router::new()
    .nest("/public", public_mount)
    .merge(non_public)
    .with_state(state)
```

New `AppError::TaggedDeviceNotAllowed` → 403. Entry in `ERRORS.md`.

This makes the policy explicit: a new route added under `/api/*`
or as a static handler inherits the rejection automatically. To
opt a future route in to tagged-device access, mount it under
`/public` (or, if that doesn't suit, factor the gate into a
sub-router and selectively skip the layer).

### Schema additions

Single migration `migrations/<ts>_device_tailscale_identity/`:

```sql
-- up.sql
ALTER TABLE devices
    ADD COLUMN tailscale_node_id   TEXT UNIQUE,
    ADD COLUMN tailscale_node_name TEXT,
    ADD COLUMN tailscale_tailnet   TEXT;
CREATE INDEX devices_tailscale_node_id_idx ON devices (tailscale_node_id)
    WHERE tailscale_node_id IS NOT NULL;

-- down.sql
DROP INDEX devices_tailscale_node_id_idx;
ALTER TABLE devices
    DROP COLUMN tailscale_tailnet,
    DROP COLUMN tailscale_node_name,
    DROP COLUMN tailscale_node_id;
```

`crates/database/src/devices.rs`:

- Add the three optional fields to `Device`, `DeviceWithInfo`, the
  insertable shape.
- `Device::from_tailscale_node_id(&mut conn, node_id) -> Option<Device>`,
  parallel to `Device::from_key`.
- Extend `Device::create` (or add a `create_with_tailscale`
  constructor) for the auto-discovery path.

### Configuration toggles

- `TAILSCALE_OAUTH_CLIENT_ID`, `TAILSCALE_OAUTH_CLIENT_SECRET`,
  `TAILSCALE_TAILNET` — required together to enable the path on
  private-server. Absent → `None` directory → tailnet path skipped,
  exactly the current behaviour.
- `TAILSCALE_REQUIRED_TAG` — optional, e.g. `tag:canopy-server`. If
  set, the extractor rejects (`Ok(None)`, then mTLS path fires) when
  the resolved node's `tags[]` doesn't contain this value. Defense
  in depth so a contractor laptop on the tailnet can't pose as a
  server. Unset → any tailnet node is allowed.
- No debug bypass needed beyond what `cfg!(debug_assertions)` already
  gives — `commons-tests` will build a mock `TailnetDirectory` that
  hands out fixed entries, and inject it via `FromRef`.

## What's NOT in this plan

Deliberately deferred — each becomes its own plan if/when wanted:

- **Admin attach/detach UI.** Auto-discovery is the only bootstrap.
  Pre-attaching a node id to an existing device row can be done by
  `UPDATE devices SET tailscale_node_id = ... WHERE id = ...` for
  the rare case where it matters. The May-11 plan sketched a full
  attach/detach/merge UI; we can revive that when there's demand.
- **Merge of an mTLS-only device row and a tailnet-only row.**
  Devices that present *both* end up as two separate `Device`
  rows. Surface to the admin UI later.
- **Self-hosted Headscale support.** The API contract differs
  slightly. `TAILSCALE_API_BASE` leaves the door ajar but we don't
  exercise the Headscale path.
- **Multi-tailnet.** One tailnet per private-server deployment is
  the only supported topology in this plan.

## Files touched

New:

- `migrations/<ts>_device_tailscale_identity/{up,down}.sql`.
- `crates/commons-servers/src/device_auth/{mod,mtls,tailnet}.rs`
  (replaces today's flat `device_auth.rs`).
- `crates/commons-servers/src/tailnet_directory.rs` — HTTP client
  and read-through cache against the Tailscale REST API.
- `crates/commons-servers/src/tailnet_directory_mock.rs` (gated on
  `#[cfg(any(test, feature = "test-support"))]`) — used by
  `commons-tests` and integration tests.
- `crates/commons-servers/src/tailnet_guard.rs` — middleware that
  rejects tagged-device callers from non-`/public` surfaces.

Modified:

- `crates/database/src/schema.rs` — three new columns on `devices`.
- `crates/database/src/devices.rs` — model + lookup +
  first-contact constructor.
- `crates/commons-servers/src/lib.rs` — re-export new module
  structure and the directory type.
- `crates/commons-servers/Cargo.toml` — add `reqwest` (likely
  already there transitively) and any small JSON helpers.
- `crates/private-server/src/state.rs` — add `Option<TailnetDirectory>`
  field, env-driven construction in `init()`.
- `crates/private-server/src/lib.rs` — split the current single
  merged router so the `/public` nest is mounted without the
  tagged-device guard, and everything else gets the guard layer.
- `crates/commons-tests/src/server.rs` — `run_with_tailnet_device_auth`
  helper modelled on `run_with_device_auth`, wired to the mock
  directory.
- `ERRORS.md` — new entries:
  - `AuthTailnetDirectoryUnavailable` (503) — directory background
    refresh has been failing for too long.
  - `AuthTailnetNodeNotPermitted` (403) — required tag is set and
    the resolved node lacks it.
  - `TaggedDeviceNotAllowed` (403) — tagged-device caller hit a
    non-`/public` surface.

Untouched (deliberately):

- `crates/public-server/**` — public-server binary stays mTLS-only
  by construction (`AppState` has no directory field).
- `crates/private-server/src/fns/**` — handler code is unchanged;
  every existing extractor (`ServerDevice`, `AdminDevice`,
  `ReleaserDevice`) automatically benefits from the dual auth path.

## Verification

1. `just migrate` — applies the migration locally.
2. `just check` — workspace compiles.
3. `nice just test-package database` — `from_tailscale_node_id` and
   first-contact constructor.
4. `nice just test-package commons-servers` — `TailnetDirectory`
   refresh, lookup, spoof-guard (non-tailnet IP → `None`), missing
   directory → `None`. Mock the Tailscale API with `wiremock` or
   similar.
5. `nice just test-package private-server` — new integration tests:
   - `tailnet_device_auth.rs`: happy path (known node → role-gated
     `/public/...` endpoint 200), unknown node (auto-creates
     Untrusted device, subsequent server endpoint 403), spoofed
     `X-Forwarded-For` with a public IP (tailnet path skipped,
     falls through to `AuthMissingCertificate`).
   - `tagged_device_guard.rs`: tagged-device caller (no
     `Tailscale-User-Login`, tailnet source IP) hitting `GET /`,
     `GET /api/docs`, and a representative `/api/...` route → 403
     `TaggedDeviceNotAllowed` from each. Same calls with a
     `Tailscale-User-Login` header set → behave normally
     (handler-specific). Same calls with a non-tailnet source IP
     → no 403 from the guard.
6. `nice just test-package public-server` — sanity that hitting it
   with `X-Forwarded-For: 100.64.0.1` and no mTLS cert returns
   `AuthMissingCertificate` (its state has no directory, so the
   tailnet path cannot fire).
7. `nice just test`.
8. Local manual smoke: with mock directory + dev-mode bypass,
   curl one of the `/public/...` endpoints — confirm accepted as
   the mock device.
9. Prod smoke: deploy with the three Tailscale env vars set. From
   a tagged server, repeat the original failing call —
   `curl -XPOST https://tamanu-meta-prod.tail53aef.ts.net/public/events`
   should now succeed with auto-created or pre-attached device.

## Unplan criteria

This plan is "done" (and the `unplan:` commit deletes the file)
when:

- Migration applied in prod.
- At least one real tagged device round-trips `/public/events` and
  `/public/status/<id>` via the tailnet (no mTLS cert) in prod.
- CI is green on the new test files.
- `commons-tests` exposes `run_with_tailnet_device_auth` and at
  least one private-server integration test consumes it.

Leave the plan file in place if any of those is unverified.
