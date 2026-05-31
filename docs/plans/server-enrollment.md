# Operator-first server enrollment

Replace the current "device-first / ticket-pull" registration with an
"operator-first / token-push" flow:

1. An operator creates a **server** directly, inside an existing **group**,
   filling in details and optionally picking a Tailscale identity.
2. Canopy mints a single-use **enrollment token** for that server.
3. The operator runs `bestool canopy register <base64>` on the box; bestool
   claims the pre-created server over mTLS by presenting the token, and Canopy
   binds the device (its mTLS public key) to the server.

Creating a group drops straight into "create a server in this group", so the
common path is one continuous flow. Servers and groups become deletable.
"Device" stops being a first-class concept in the operator's mental model — it
is demoted to an internal/advanced detail of a server.

The bestool half is **out of scope for this repo**; it is captured separately
in `docs/specs/bestool-canopy-register.md` for hand-off to a bestool agent.
This plan only covers Canopy (database, public-server, private-server, React).

## Why the current flow is backwards

Today (see exploration notes below) the *device* leads:

- The server generates a `CanopyTicket`
  (`crates/commons-types/src/server/ticket.rs:7`) carrying **its own** public
  key, hostname, URL, Tailscale hints, kind/rank.
- It comes online over mTLS; the device-auth extractor
  (`crates/commons-servers/src/device_auth/mtls.rs:29`) auto-creates an
  `Untrusted` device pinned by that public key.
- An operator pastes the base64 ticket into Canopy's **Devices** page
  (`private-web/src/routes/DevicesSearch.tsx` → `ImportTicketDialog`), hitting
  `POST /api/servers/import_ticket`
  (`crates/private-server/src/fns/servers.rs:575`) →
  `Server::upsert_from_ticket` (`crates/database/src/servers.rs:193`), which
  promotes the device to `Server` and upserts the `servers` row.
- Only *then* does the operator open the server, set a group, monitoring, etc.

Problems this plan fixes:

- It requires generating a ticket up front (`bestool t meta-ticket`), so there
  is **no way to add a server that isn't bestooled yet**.
- Server creation lives under **Devices**, conflating two concepts the operator
  shouldn't have to reason about.
- There is **no `Server::delete`** and **no `Device::delete`** (only
  `Device::merge_into`), so servers/devices accumulate forever.
- Grouping is an afterthought done per-server after import.

## Decisions (confirmed with the user)

1. **Legacy ticket import is removed entirely.** Delete the Import Ticket UI,
   the `import_ticket` endpoint, `Server::upsert_from_ticket`, the
   `CanopyTicket` type's Canopy-side usage, and `private-web`'s
   `lib/canopyTicket.ts`. `bestool t meta-ticket` is removed entirely (noted in
   the bestool spec). Reusable internals (URL canonicalization, cloud detection,
   device find/create/promote) are refactored into helpers consumed by the new
   register path, not deleted.
2. **Server delete = soft-delete, device rebindable.** Archiving a server sets
   `deleted_at`, **releases its device** (`device_id → NULL`, device role →
   `Untrusted`) and clears `registered_at`. History (statuses, associations,
   incidents) stays attached to the archived row. The freed device keeps its
   keys, so the same box re-registering against a *new* server is matched by
   `Device::from_key`, rebound, and re-promoted.
3. **Group delete keeps the empty-only guard.** `ServerGroup::delete`
   (`crates/database/src/server_groups.rs:146`) already refuses non-empty
   groups; keep that. Operators reassign/delete servers first.
4. **Enrollment token: single-use, 7-day expiry, reissuable.** Stored hashed,
   one active token consumed on first successful register, re-mintable from the
   server page.

## The enrollment blob (the contract with bestool)

`bestool canopy register <base64>` consumes a base64url-encoded JSON blob that
Canopy produces. This shape is the integration contract; it is duplicated in
the bestool spec.

```jsonc
{
  "v": "enroll-1",
  "api_url": "https://<public-server base>",  // from PUBLIC_URL; device API origin
  "server_id": "<uuid>",
  "group_id": "<uuid>",                        // informational + cross-check
  "token": "<base64url of 32 random bytes>"    // the single-use secret
}
```

- `api_url` comes from `PUBLIC_URL` (the device-facing origin — see the
  PUBLIC_URL/PRIVATE_URL split: operator links use PRIVATE_URL, device API uses
  PUBLIC_URL). The blob is for the device, so PUBLIC_URL.
- `group_id` is redundant with the server record but is included as requested,
  so bestool can display/confirm context and the register endpoint can
  cross-check.
- `token` plaintext lives **only** in the blob; Canopy stores only its hash.
- No CA/cert is shipped in the blob: Canopy's public server uses a webPKI
  (Let's Encrypt) certificate, so bestool verifies the TLS connection against
  system roots. Pinning the LE CA in the blob would be fragile across cert/CA
  rotation, so it is deliberately omitted.

## Data model changes (migrations)

Create the migration with `just migration <name>` (never hand-create the
files/directory — that produces inconsistent naming). Two logical changes below;
they can be one migration or two — prefer two (`add-server-archival` and
`server-enrollment-tokens`) for independent down-migrations.

### `servers` columns + partial uniqueness

```sql
ALTER TABLE servers ADD COLUMN deleted_at    TIMESTAMPTZ;          -- soft-delete / archive
ALTER TABLE servers ADD COLUMN registered_at TIMESTAMPTZ;          -- set by register endpoint

-- host must stay unique among live servers, but an archived row must not block
-- recreating a server at the same host.
ALTER TABLE servers DROP CONSTRAINT servers_host_key;
CREATE UNIQUE INDEX servers_host_live ON servers (host) WHERE deleted_at IS NULL;

CREATE INDEX servers_live ON servers (deleted_at) WHERE deleted_at IS NULL;
```

`down.sql` reverses (restore the plain unique constraint, drop columns/indexes).
Note: restoring the plain unique constraint in `down` can fail if duplicate live
hosts exist; acceptable for a dev-only down.

### `server_enrollment_tokens` table

```sql
CREATE TABLE server_enrollment_tokens (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id   UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL UNIQUE,        -- SHA-256 of the plaintext token
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ                   -- set the instant a valid token is presented, win or lose (replay safety)
);
CREATE INDEX server_enrollment_tokens_server ON server_enrollment_tokens (server_id);
```

Reissue = insert a new row. A server may have several rows over time; "active"
= `consumed_at IS NULL AND expires_at > now()`. Minting a fresh token expires
any prior un-consumed ones (set their `expires_at = now()`), so only one blob is
ever valid at a time.

## Backend: `crates/database`

### `servers.rs`

- **Add `Server::create(conn, NewServer) -> Result<Server>`** — operator-driven
  insert. `NewServer` carries: `name`, `host`, `kind`, `rank`, `group_id`,
  `public_name`, `cloud`, `geolocation`, `is_monitored`, `alert_when_down_for`,
  `notes`, `tags`, and an optional pre-bound `device_id` (set when a Tailscale
  identity was chosen at create time). `registered_at` stays NULL.
- **Add `Server::soft_delete(conn, id)`** — in one transaction: set
  `deleted_at = now()`, capture `device_id`, set `device_id = NULL` and
  `registered_at = NULL`; if there was a device, `Device::untrust` it
  (role → `Untrusted`). Idempotent on already-archived rows.
- **Add `Server::restore(conn, id)`** — clear `deleted_at` (does **not** rebind a
  device; the box must re-register). Guard against host collision with a live
  server (return a conflict error).
- **Filter `deleted_at IS NULL` everywhere live servers are listed/looked up.**
  Audit and update each of: `get_all` (`servers.rs:81`), `list_by_kind`
  (`:106`), `all_pingable` (`:159`), `get_by_host` (`:178`),
  `get_by_device_id` (`:284`), `list_ungrouped` (`:341`),
  `ServerGroup::list_servers` (`server_groups.rs:167`), and any
  status/incident roll-ups that enumerate servers. `get_by_id`/`get_by_ids`
  should still resolve archived rows (detail/restore needs them) but callers
  that drive monitoring must exclude archived — verify each call site rather
  than blanket-filtering the by-id getters.
- **Refactor out of the deleted `upsert_from_ticket`** into reusable helpers:
  - `canonicalize_host(url) -> UrlField` (URL normalization, `servers.rs:211`).
  - `detect_cloud(hosting: &str) -> Option<bool>` (`servers.rs:244`).
  These are called by the register path.
- **Delete `Server::upsert_from_ticket`** and its `CanopyTicket` import.

### `server_enrollment_tokens.rs` (new model module)

- `struct ServerEnrollmentToken { … }` mirroring the table; re-export from
  `lib.rs`.
- `mint(conn, server_id, ttl: Duration) -> Result<(ServerEnrollmentToken, String)>`
  — generate 32 random bytes (`rand`), token string = base64url, store SHA-256
  hash, `expires_at = now() + ttl`; in the same tx expire prior un-consumed
  tokens for the server. Returns the plaintext for the blob (caller must not
  persist it).
- `consume(conn, server_id, plaintext) -> Result<()>` — hash, then a single
  atomic `UPDATE server_enrollment_tokens SET consumed_at = now() WHERE
  server_id = $1 AND token_hash = $2 AND consumed_at IS NULL AND expires_at >
  now() RETURNING …`. The burn is **committed on its own**, before any binding
  work — a valid token is spent the instant it is presented, regardless of
  whether the rest of registration then succeeds (replay safety; the operator
  reissues on any failure). The `RETURNING`-guarded update also makes concurrent
  registers race-safe (only one wins). Distinct errors for
  not-found/expired/already-consumed (see ERRORS.md below).
- `active_for(conn, server_id) -> Result<Option<ServerEnrollmentToken>>` — for
  the server page to show "token pending / expires <when>" without revealing the
  secret.

### `devices.rs`

- Reuse `from_key` (`:98`), `create` (`:112`), `create_with_tailscale` (`:147`),
  `trust` (`:608`), `untrust` (`:625`), `merge_into` (`:259`).
- **Add `Device::add_key(conn, device_id, key: Vec<u8>)`** — insert an active
  `device_keys` row if that key isn't already present on the device. Used by
  register when binding an mTLS key to a Tailscale-precreated device.

## Backend: `crates/public-server` — the register endpoint

New `POST /servers/register` (`crates/public-server/src/servers.rs`).

- **Auth/identity:** the device presents a client cert; extract the
  SubjectPublicKeyInfo bytes **directly** (factor the parse out of
  `device_auth/mtls.rs:21-25` into a helper that returns the key *without*
  auto-creating a device). Register must own device resolution, so it must not
  go through the auto-creating `Device` extractor.
- **Body:** `RegisterArgs { server_id: Uuid, token: String }` (accept and
  ignore/cross-check `group_id`).
- **Logic (token burn first, then binding):**
  1. Load the server by id; 404 if missing, 409 if archived (`deleted_at` set).
  2. `ServerEnrollmentToken::consume(server_id, token)` — rejects
     invalid/expired/already-consumed. **This commits the burn on its own** so a
     presented-but-then-failed register can't be retried with the same token.
  3. The remaining steps run in their own transaction (rolled back together on
     failure, but the token stays burned). Resolve target device:
     - If `server.device_id` is set (Tailscale-precreated) → target = that
       device; `Device::add_key(target, key)`.
     - Else → `Device::from_key(key)`; if found use it, else `Device::create`;
       set `server.device_id = target`.
     - If `from_key` resolved a *different* device than an already-set target,
       `Device::merge_into(other → target)`.
  4. `Device::trust(target, Server)` (promote).
  5. `server.registered_at = now()`.
  6. Return `RegisterResponse { server_id, device_id, central_public_key? }`
     (the device persists its identity/config from this).
- **mTLS termination** already accepts arbitrary client certs (it auto-creates
  devices today), so no proxy/Envoy change is needed.

**Existing public-server server endpoints:** `create` (`servers.rs:84`),
`edit` (`:115`), `remove` (`:152`). The device-driven `create` is made
**redundant** by operator-first creation. *Follow-up decision (do not silently
drop):* whether bestool still self-`create`s/`edit`s server metadata after
registration. Resolve against the bestool spec before removing `create`; keep
`edit` if bestool reports host/metadata. Flagged in "Open items".

## Backend: `crates/private-server/src/fns/servers.rs`

- **Add `create`** (`POST /api/servers/create`, `TailscaleAdmin`) — body mirrors
  `NewServer` plus optional `tailscale_identifier`. If an identifier is given,
  resolve it (reuse the tailnet directory path behind
  `resolve_tailnet_identifier`/`attach_tailscale_device`,
  `fns/servers.rs:667` & `fns/devices.rs:736`) and `Device::create_with_tailscale`
  before inserting the server with that `device_id`. Returns the new server id.
- **Add `delete`** (`POST /api/servers/delete`, `TailscaleAdmin`) →
  `Server::soft_delete`.
- **Add `restore`** (`POST /api/servers/restore`, `TailscaleAdmin`) →
  `Server::restore`.
- **Add `mint_enrollment`** (`POST /api/servers/mint_enrollment`,
  `TailscaleAdmin`) — `ServerEnrollmentToken::mint`, assemble the blob JSON
  (`api_url` from `PUBLIC_URL`, ids, plaintext token, optional `ca`), base64url
  it, return `{ blob, expires_at }`.
- **Add `enrollment_status`** (or fold into `get_detail`) — return
  `registered_at` and `active_for` (expiry only, never the secret) so the UI can
  show pending vs registered and "expires <when>".
- **Remove `import_ticket`** (`:575`) and drop it from `routes()` (`:221`).
- **Keep `attach_tailscale_device`** for post-hoc editing, but it is no longer
  the primary path.
- Surface archived servers in `get_detail`/`get_info` with the archived flag so
  the detail page can show a restore affordance; exclude them from `list_some`
  and `list_ungrouped`.

Run `just gen-openapi` after these handler changes (regenerates
`private-web/openapi.json` + `src/api-types.ts`); commit both alongside.

## ERRORS.md + `AppError`

Add variants (and ERRORS.md headings matching the problem type):

- `EnrollmentTokenInvalid` — unknown token for that server → 401/403.
- `EnrollmentTokenExpired` → 410 (gone) or 403.
- `EnrollmentTokenConsumed` → 409.
- `ServerArchived` — registering against / editing an archived server → 409.
- Group-non-empty delete already has handling via the guard; reuse/confirm its
  problem type.

## Frontend: `private-web`

### New: create-server flow

- Route `/servers/new` and `/groups/:id/servers/new` → a `ServerCreate` page
  reusing the field set from `ServerEdit.tsx` (`EditForm`), minus the raw
  `device_id` input, **plus** a Tailscale identity picker that previews via
  `resolve_tailnet_identifier`. When entered from a group, the group is
  preselected and locked (changeable but defaulted).
- On submit → `servers.create` → navigate to the **setup** view (below).

### New: setup / instructions view

After create (and reachable from a not-yet-registered server's detail page):

- Calls `servers.mint_enrollment` and shows:
  - install bestool,
  - `bestool canopy register <base64>` with a copy button,
  - "token expires <relative time>" and a **Reissue** button (re-mints, replaces
    the blob),
  - a live "waiting for this server to check in…" hint that flips to "registered"
    once `registered_at` is set (poll `enrollment_status`/`get_detail`).

### Group create → server create

- After `server_groups.create`, redirect to `/groups/:id/servers/new`. A Back
  action leaves the (legal) empty group in place.
- Add **Add server** buttons on `GroupDetail.tsx` and the groups list
  (`GroupsList.tsx`).

### Server detail / delete / archive

- `ServerDetail.tsx`: add **Delete (archive)** with confirm → `servers.delete`.
- Show a banner + setup instructions while `registered_at` is null.
- For archived servers: show an **archived** state with a **Restore** action;
  keep history visible.

### Demote "Device" from a first-class concept

- Remove `ImportTicketButton`/`ImportTicketDialog` from `DevicesSearch.tsx` and
  delete `private-web/src/lib/canopyTicket.ts` + `parseCanopyTicket` usage.
- Fold device info on `ServerDetail` into a collapsible **Advanced / identity**
  section (mTLS key(s), Tailscale identity, connection history) rather than a
  primary panel. It is **collapsed by default** — device is an internal detail,
  surfaced only when the operator expands it. Keep the `/devices` area for power users (trust/untrust/merge),
  but it is no longer where servers are born.
- Group delete: keep the guarded behaviour; show a friendly "move or delete its
  servers first" message on the 4xx.

## Tests

Backend (`commons_tests::db` and `::server`):

- `Server::create` inserts ungrouped and grouped; `host` uniqueness only among
  live rows (archived row does not block recreating same host).
- `soft_delete` archives, releases + untrusts the device, hides the row from
  `get_all`/`list_ungrouped`/`list_by_kind`/group listings, keeps it in
  `get_by_id`; `restore` brings it back and 409s on a live-host clash.
- Token: `mint` returns a usable plaintext + stores only the hash; minting again
  expires the prior token; `consume` succeeds once then errors
  (consumed/expired/unknown).
- Register endpoint (public-server HTTP test with `mtls-certificate` header):
  - happy path with no pre-bound device → device created, bound, promoted,
    `registered_at` set, token consumed;
  - happy path with a Tailscale-precreated device → mTLS key added to *that*
    device (no second device), promoted;
  - re-register of a soft-deleted-then-recreated server by the same cert → the
    freed device is matched by key and rebound;
  - expired/consumed/invalid token, archived server → correct errors;
  - replay safety: a register whose binding step fails *after* a valid token is
    presented leaves the token consumed (a retry with the same token is rejected
    as already-consumed) — assert `consumed_at` is set even on the failure path.
- Removing `import_ticket`: delete `crates/private-server/tests/import_ticket.rs`
  and `crates/database/tests/upsert_from_ticket.rs` (or repoint to register).

Frontend e2e (Playwright, `private-web/e2e`):

- create group → lands on create-server-in-group; back leaves empty group;
- create server → setup view shows a blob + reissue;
- archive a server hides it from lists and shows restore.

## Open items / follow-ups (surface, don't drop)

- **Public-server `create`/`edit`/`remove` server endpoints:** decide their fate
  against the bestool spec (does bestool self-report metadata post-register?).
  Resolve before removing `create`.
- **`CanopyTicket` type removal from `commons-types`:** safe within Canopy once
  `upsert_from_ticket` is gone, but confirm no other Canopy consumer; bestool's
  `meta-ticket` (separate repo) is removed entirely via the spec.
- **`central_public_key` in the register *response*:** only useful if there's an
  application-layer trust anchor the device needs (distinct from TLS, which is
  webPKI). Confirm whether anything consumes it before wiring it; otherwise drop
  it from the response too.
- **Token TTL configurability:** 7 days is the default; consider an env override
  later, not now.
