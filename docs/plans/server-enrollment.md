# Operator-first server enrollment

Replace the current "device-first / ticket-pull" registration with an
"operator-first / token-push" flow:

1. An operator creates a **server** directly, inside an existing **group**,
   filling in details and optionally picking a Tailscale identity.
2. Canopy mints a single-use **enrollment token** for that server, encrypts the
   enrollment payload under a freshly-generated 4-word passphrase (age/scrypt),
   and shows the **encrypted ticket** plus the **passphrase** separately.
3. The operator runs `bestool canopy register` on the box (feeding it the ticket
   and the passphrase, which decrypts it back to the token+payload);
   bestool claims the pre-created server over mTLS via a **challenge/response
   that proves it holds the private key** behind the cert it presents, and
   Canopy binds that device to the server.

Creating a group drops straight into "create a server in this group", so the
common path is one continuous flow. Servers and groups become deletable.
"Device" stops being a first-class concept in the operator's mental model — it
is demoted to an internal/advanced detail of a server.

The bestool half is **out of scope for this repo**; it is captured separately
in `docs/specs/bestool-canopy-register.md` for hand-off to a bestool agent.
This plan only covers Canopy (database, public-server, private-server, React).

## Why the current flow is backwards

Today the *device* leads:

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
   `lib/canopyTicket.ts`, **and the `CanopyTicket` type itself** (delete
   `crates/commons-types/src/server/ticket.rs` plus the `pub mod ticket;` /
   `pub use ticket::CanopyTicket;` in `crates/commons-types/src/server.rs` —
   verified there are no other Canopy consumers once `upsert_from_ticket` and
   `import_ticket` are gone). `bestool t meta-ticket` is removed entirely (noted
   in the bestool spec). Reusable internals (URL canonicalization, cloud
   detection) are refactored into helpers consumed by the new register path, not
   deleted.
   This is a net security improvement, with one conscious tradeoff (see
   *Security model*, "Legacy tradeoff").
2. **Server delete = soft-delete, device rebindable via the gated flow.**
   Archiving a server sets `deleted_at`, **releases its device**
   (`device_id → NULL`, role → `Untrusted`), **deactivates that device's keys**
   (`is_active = false`), and clears `registered_at`. History (statuses,
   associations, incidents) stays attached to the archived row. The device row
   persists, so the same box can re-enroll against a *new* server — but only
   through the full token + proof-of-possession flow, which re-activates/adds
   the presented key. There is **no silent key-match re-promotion** (see
   *Security model*, items 4 and 7).
3. **Group delete keeps the empty-only guard.** `ServerGroup::delete`
   (`crates/database/src/server_groups.rs:146`) refuses non-empty groups; keep
   that. The guard's count must include archived servers — see *Backend:
   database → server_groups.rs*.
4. **Enrollment token: single-use, 7-day expiry, reissuable.** Stored hashed.
   Burned **at completion** (atomically with the successful bind), not on mere
   presentation — replay is covered by the challenge nonce instead (see
   *Security model*, item 3). 7 days is deliberate: enrollment runs on human
   operational timescales, not machine ones.

## Security model

This is a new authentication/trust boundary, reviewed adversarially before
implementation. The findings below are baked into the design.

**1. Proof-of-possession, not just a presented public key.** The mTLS layer
identifies a device by the `SubjectPublicKeyInfo` (SPKI) bytes of the presented
cert (`device_auth/mtls.rs:27`), with no CA chain and — critically — no proof
the caller holds the matching *private* key (possession is assumed to be
enforced by the terminating proxy's handshake, which is an out-of-repo
invariant). A public key is not secret, so binding identity on presented-SPKI
alone is unsafe. Therefore enrollment is a **two-step challenge/response**:
Canopy issues a random nonce and the device must return a signature over it with
its private key, which Canopy verifies against the presented SPKI. This gives
Canopy *application-layer* proof-of-possession independent of the proxy.

*What PoP does and doesn't cover:* it stops an attacker binding a **victim's**
public key or a fabricated SPKI, and stops request replay (the nonce is
single-use). It does **not** stop someone who holds a leaked token from binding
**their own** key (= takeover) — that residual is inherent to a bearer-token
push model and is mitigated by token secrecy (encrypted-ticket handling,
redaction, short challenge window) plus alerting, not by PoP. Treated as: ticket
+ passphrase leakage is a breach to investigate, not a routine threat to design
DoS-resistance around.

*Channel binding (optional, env-gated):* to bind the app-layer signature to the
TLS session (defeating relay/MITM through the terminating proxy), the signed
transcript can include the TLS exporter value (RFC 9266 "tls-exporter": label
`EXPORTER-Channel-Binding`, empty context, 32 bytes). The app never shares TLS
keying material with a terminating proxy, so this only works if the proxy
computes the exporter and forwards it in a header. It is therefore **gated on an
env var that names that header** (e.g. `CANOPY_ENROLL_EKM_HEADER=x-tls-exporter`):
when set, Canopy requires channel binding (reads the EKM from that header,
includes it in the expected transcript, rejects if absent/mismatched) and
`begin` advertises the requirement to the client; when unset, app-layer PoP runs
without it. Both peers derive the same exporter from the shared TLS session, so
bestool computes it client-side with the same parameters and includes it in the
signature. May not be deployable until the proxy supports it — hence the gate.

**2. No device is auto-created at the mTLS boundary.** Today `mtls::resolve`
auto-creates an `Untrusted` device for any first-contact key
(`device_auth/mtls.rs:29-37`), so anyone reaching the endpoint once from the
internet can mint an `Untrusted` device row. Change `resolve` to **never
create**: an unknown key resolves to no device (auth fails). New devices are
born in exactly one place — `register/complete`, gated by token + PoP. (The
tailnet path may keep first-contact creation: tailnet membership is itself a
trust gate.)

**3. Token burned at completion; nonce carries replay safety.** The token is
*validated* (not consumed) at `begin`; it is *consumed* only inside the
successful `complete` transaction, atomically with the bind. Replay of a
captured `complete` fails because the nonce is single-use and short-lived. This
removes the keyless-griefing DoS (denying enrollment now requires completing PoP
with some key, i.e. performing the takeover) without weakening replay
protection. Pair with **rate-limiting** `/register/*` per `server_id` + source
and **alerting** when a token is consumed without a completed bind, or when a
challenge fails signature verification.

**4. Binding never merges and never steals another server's identity.** In
`complete`:
- Reject (generic failure) if the presented key is already an **active** key on
  a device bound to a *different live* server.
- Pre-bound (Tailscale) device: add the presented key only if the device has **no
  existing active mTLS key** (a Tailscale-precreated device has none); if it
  already has one, refuse and require operator action. Surface/audit any key
  addition to an already-trusted device.
- Otherwise resolve by key to a *free* (unbound / Untrusted) device or create a
  new one. **Never** call `Device::merge_into` from the register path — merging
  is destructive (re-parents all FKs, deletes a row) and must stay an admin-only
  action, never reachable by an enrolling box.

**5. Error responses on the public endpoint are opaque.** All pre-completion
failures — unknown server, archived server, invalid/expired/consumed token, bad
nonce, bad signature — return one generic "enrollment failed" to the device, so
the endpoint isn't an existence/lifecycle oracle for a probed `server_id`
(UUIDv4, not guessable, but ids leak via URLs/logs/the ticket payload). The specific
reason is logged server-side and exposed only through the admin-gated
`enrollment_status`.

**6. The token is never logged, on either side.** Redact `token` and the
enrollment payload from request/response logging, tracing spans, and
`AppError`/RFC-7807 `detail` (`#[serde(skip)]` on Debug surfaces; ensure no
body-logging middleware covers `/register/*`). A test asserts the token does not
appear in any error body.

**6a. The enrollment ticket is encrypted at rest in transit.** `mint_enrollment`
does not return the payload in the clear: it encrypts it with `algae-cli`'s
age/scrypt passphrase profile (the same primitives bestool's `protect`/`reveal`
use) under a freshly-generated 4-word passphrase (~52 bits, EFF large wordlist),
and returns the base64'd ciphertext (`ticket`) and the `passphrase` separately.
The ticket is then safe to copy around; the passphrase is shared out-of-band.
This protects the ticket *in transit* only — the public `register/begin|complete`
endpoints and the token/PoP handshake are unchanged; bestool decrypts with the
passphrase and presents the same plaintext `token`. The benefit only holds if
ticket and passphrase travel on **different channels**; the residual brute-force
risk is bounded by the 4-word entropy + scrypt KDF on top of the token already
being single-use, 7-day, rate-limited, and PoP-gated.

**7. Soft-delete deactivates keys to prevent silent identity resurrection.**
Because identity is the (public, non-secret) SPKI, a released device that *kept*
active keys could be silently re-promoted on key-match. So soft-delete
deactivates keys (decision 2); re-enrollment must go through token + PoP.

**8. No tailnet gating; 7-day TTL kept.** Restricting `/register/*` to the
tailnet was considered and **declined**: tokens must be usable outside the
tailnet (tailnet membership is itself a high trust gate, so gating on it would
make tokens redundant where they're needed). Shortening the 7-day TTL was
considered and **declined**: enrollment runs on human timescales. The *challenge*
nonce, by contrast, is short-lived (minutes).

**Legacy tradeoff (accepted):** the removed ticket flow bound a key the device
*itself* asserted and the operator vouched for by pasting the ticket; the new
flow binds whatever key completes PoP against a valid token. With PoP + the
no-auto-create + no-merge guards above, the per-binding assurance is sound, and
the usability win is large. Recorded as a conscious decision.

## The enrollment ticket (the contract with bestool)

`bestool canopy register` consumes a base64-encoded, **age-encrypted** ticket
that Canopy produces, plus a 4-word passphrase that decrypts it. Decrypting the
ticket with the passphrase yields the JSON payload below. This shape is the
integration contract; it is duplicated in the bestool spec.

```jsonc
{
  "v": "enroll-1",
  "api_url": "https://<public-server base>",  // from PUBLIC_URL; device API origin
  "server_id": "<uuid>",
  "token": "<base64url of 32 random bytes>"    // the single-use secret
}
```

- The ticket is the age/scrypt encryption of the JSON above under the
  freshly-generated 4-word passphrase, base64'd (standard) for transport.
  bestool reuses algae's `reveal`/`decrypt_stream` + `PassphraseArgs` to prompt
  for the passphrase and decrypt, then runs the existing PoP handshake with the
  decrypted token.
- `api_url` comes from `PUBLIC_URL` (the device-facing origin — operator links
  use PRIVATE_URL, device API uses PUBLIC_URL). The payload is for the device, so
  PUBLIC_URL.
- `token` plaintext lives **only** inside the encrypted ticket; Canopy stores
  only its hash. bestool reads the ticket from **stdin/file, never an argv
  positional**, and prompts for the passphrase (so neither lands in shell history
  or `ps`/`/proc/<pid>/cmdline`) — see the bestool spec.
- **No `group_id`** in the payload: the server record already authoritatively
  holds the group, so shipping it bought no security (it can't be a second factor
  when it travels in the same ticket it'd be checked against). It would only
  matter for reusable/multi-server tokens, which we are not doing. UI shows the
  group from the server record.
- No CA/cert is shipped: `api_url` is served with a webPKI (Let's Encrypt)
  certificate, so bestool verifies TLS against system roots. Pinning a CA in the
  payload would be fragile across rotation.

## Data model changes (migrations)

Create migrations with `just migration <name>` (never hand-create the
files/directory — that produces inconsistent naming). Prefer separate migrations
(`add-server-archival`, `server-enrollment-tokens`, `server-enrollment-challenges`)
for independent down-migrations.

### `servers` columns + partial uniqueness

```sql
ALTER TABLE servers ADD COLUMN deleted_at    TIMESTAMPTZ;          -- soft-delete / archive
ALTER TABLE servers ADD COLUMN registered_at TIMESTAMPTZ;          -- set on successful enrollment

-- host must stay unique among live servers, but an archived row must not block
-- recreating a server at the same host.
ALTER TABLE servers DROP CONSTRAINT servers_host_key;
CREATE UNIQUE INDEX servers_host_live ON servers (host) WHERE deleted_at IS NULL;

CREATE INDEX servers_live ON servers (deleted_at) WHERE deleted_at IS NULL;
```

`down.sql` reverses (restore the plain unique constraint, drop columns/indexes).
Restoring the plain unique constraint in `down` can fail if duplicate live hosts
exist; acceptable for a dev-only down.

### `server_enrollment_tokens` table

```sql
CREATE TABLE server_enrollment_tokens (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id   UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL UNIQUE,        -- SHA-256 of the plaintext token
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ                   -- set atomically with a successful bind
);
CREATE INDEX server_enrollment_tokens_server ON server_enrollment_tokens (server_id);
```

"active" = `consumed_at IS NULL AND expires_at > now()`. Reissue inserts a new
row and **marks prior un-consumed tokens `consumed_at = now()`** (definitively
dead — not merely nudging `expires_at`, which races a concurrent presentation),
all in one transaction, so exactly one token is active afterward.

### `server_enrollment_challenges` table (the PoP nonce)

```sql
CREATE TABLE server_enrollment_challenges (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id   UUID NOT NULL REFERENCES servers (id) ON DELETE CASCADE,
    token_hash  BYTEA NOT NULL,               -- which token this challenge is for
    public_key  BYTEA NOT NULL,               -- the SPKI presented at `begin`
    nonce       BYTEA NOT NULL,               -- 32 CSPRNG bytes, the challenge
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL,         -- short, ~5 minutes
    used_at     TIMESTAMPTZ                   -- single-use: set when taken at `complete`
);
CREATE INDEX server_enrollment_challenges_lookup ON server_enrollment_challenges (server_id, nonce);
```

A challenge is one-shot and not a reusable secret, so `nonce` is stored/compared
as-is (no slow hashing). Old/expired/used challenges can be swept opportunistically.

## Backend: `crates/database`

### `servers.rs`

- **Add `Server::create(conn, NewServer) -> Result<Server>`** — operator-driven
  insert. `NewServer` carries: `name`, `host`, `kind`, `rank`, `group_id`,
  `public_name`, `cloud`, `geolocation`, `is_monitored`, `alert_when_down_for`,
  `notes`, `tags`, and an optional pre-bound `device_id` (Tailscale case).
  `registered_at` stays NULL.
- **Add `Server::soft_delete(conn, id)`** — one transaction, `SELECT … FOR
  UPDATE` on the server row: set `deleted_at = now()`, capture `device_id`, set
  `device_id = NULL` and `registered_at = NULL`; if there was a device,
  `Device::untrust` it **and deactivate its keys** (`is_active = false`).
  Idempotent on already-archived rows.
- **Add `Server::restore(conn, id)`** — clear `deleted_at` (does **not** rebind a
  device; the box must re-enroll). Guard against host collision with a live
  server (conflict error).
- **Filter `deleted_at IS NULL` everywhere live servers are listed/looked up.**
  Audit and update each of: `get_all` (`servers.rs:81`), `list_by_kind`
  (`:106`), `all_pingable` (`:159`), `get_by_host` (`:178`),
  `get_by_device_id` (`:284`), `list_ungrouped` (`:341`),
  `ServerGroup::list_servers` (`server_groups.rs:167`), and any
  status/incident roll-ups that enumerate servers. `get_by_id`/`get_by_ids`
  should still resolve archived rows (detail/restore needs them) but callers
  that drive monitoring must exclude archived — verify each call site.
- **Refactor out of the deleted `upsert_from_ticket`** into reusable helpers:
  `canonicalize_host(url) -> UrlField` (`servers.rs:211`) and
  `detect_cloud(hosting: &str) -> Option<bool>` (`servers.rs:244`), called by the
  register path.
- **Delete `Server::upsert_from_ticket`** and its `CanopyTicket` import (then the
  `CanopyTicket` type itself in `commons-types`, per decision 1).

### `server_groups.rs`

- **Fix the empty-group guard to count archived servers.** `ServerGroup::delete`
  (`:146`) counts `servers WHERE group_id = $1` with no `deleted_at` filter,
  while `list_servers` (`:167`) will now filter archived out. Without aligning
  them, a group containing only archived servers looks empty in the UI but
  `delete` refuses with "still has N servers" — a dead-end with no visible
  cause. Decision: archived servers **retain** `group_id` (history stays whole),
  so the delete guard must count them too, and the UI must surface archived
  members as blockers. Do not null `group_id` on archive.

### `server_enrollment_tokens.rs` (new model module)

- `struct ServerEnrollmentToken { … }` mirroring the table; re-export from
  `lib.rs`.
- `mint(conn, server_id, ttl) -> Result<(ServerEnrollmentToken, String)>` —
  generate 32 bytes from a CSPRNG (`OsRng`/`getrandom`, **not** a seedable RNG),
  token string = base64url, store SHA-256 hash, `expires_at = now() + ttl`; in
  the same tx mark prior un-consumed tokens `consumed_at = now()`. Returns the
  plaintext for the ticket payload (caller must not persist or log it).
- `find_active(conn, server_id, plaintext) -> Result<ServerEnrollmentToken>` —
  hash, look up by `server_id` + `token_hash` with `consumed_at IS NULL AND
  expires_at > now()`. Used by `begin` to validate **without** consuming.
- `consume(conn, server_id, token_hash) -> Result<()>` — atomic `UPDATE … SET
  consumed_at = now() WHERE server_id = $1 AND token_hash = $2 AND consumed_at IS
  NULL AND expires_at > now() RETURNING …`. Called **inside** the `complete`
  bind transaction; the `RETURNING` guard makes concurrent completes race-safe
  (one wins). Compare only via the SQL `WHERE` on the full hash — never an
  in-memory plaintext/prefix compare. (Unsalted SHA-256 is correct here because
  the token is 256-bit CSPRNG output; do **not** "upgrade" to HMAC/argon — the
  entropy carries it. Document this so a later reviewer doesn't change it.)
- `active_for(conn, server_id) -> Result<Option<ServerEnrollmentToken>>` — expiry
  only, for the admin UI; never reveals the secret.

### `server_enrollment_challenges.rs` (new model module)

- `create(conn, server_id, token_hash, public_key, ttl) -> Result<Vec<u8>>` —
  generate a 32-byte CSPRNG nonce, insert, return the nonce. Used by `begin`.
- `take(conn, server_id, nonce, public_key) -> Result<ServerEnrollmentChallenge>`
  — atomic single-use `UPDATE … SET used_at = now() WHERE server_id = $1 AND
  nonce = $2 AND public_key = $3 AND used_at IS NULL AND expires_at > now()
  RETURNING …`. Returns the row (incl. `token_hash`) so `complete` knows which
  token to consume. Failure (no match / expired / used / key mismatch) → generic
  error.

### `devices.rs`

- Reuse `from_key` (`:98`), `create` (`:112`), `create_with_tailscale` (`:147`),
  `trust` (`:608`), `untrust` (`:625`). **Do not** use `merge_into` (`:259`) from
  the register path.
- **Add `Device::add_key(conn, device_id, key)`** — insert an active key, but
  only if the device has **no existing active mTLS key**; error otherwise (the
  register path uses this only for the Tailscale-precreated, key-less case).
- **Add `Device::deactivate_keys(conn, device_id)`** — set all keys
  `is_active = false`; used by `Server::soft_delete`.
- **Add a lookup** for "is this key an active credential on a device bound to a
  *live* server?" (for the `complete` rejection in *Security model* item 4) — a
  small query joining `device_keys`/`servers` on `is_active` and
  `deleted_at IS NULL`.
- **Change `mtls::resolve` to never auto-create** (in `commons-servers`, below).

## Backend: `crates/public-server` — the register endpoints

Two endpoints under `/servers/register` (`crates/public-server/src/servers.rs`),
rate-limited per `server_id` + source.

**Shared identity step:** both extract the presented cert's SPKI bytes
**directly** — factor the cert parse out of `device_auth/mtls.rs:21-27` into a
helper returning the raw SPKI **without** creating or resolving a `Device`.
Register owns device resolution; it must not go through the auto-creating
extractor (which is itself being changed to not auto-create).

### `POST /servers/register/begin`

- Body: `BeginArgs { server_id, token }`.
- Load server; if missing/archived → **generic** failure (item 5). Validate token
  via `find_active` (does not consume) → generic failure if inactive.
- `ServerEnrollmentChallenge::create(server_id, token_hash, spki, ~5min)`.
- Return `{ nonce, channel_binding_required: bool }`. **Token not consumed.**
  `channel_binding_required` is `true` iff `CANOPY_ENROLL_EKM_HEADER` is set, so
  the client knows to include the TLS exporter in its signature.

### `POST /servers/register/complete`

- Body: `CompleteArgs { server_id, nonce, signature }`.
- `ServerEnrollmentChallenge::take(server_id, nonce, spki)` (single-use) → generic
  failure on any mismatch.
- **Verify `signature` over the transcript `nonce ‖ server_id ‖ spki [‖ ekm]`
  against the presented SPKI** (proof-of-possession). When
  `CANOPY_ENROLL_EKM_HEADER` is set, read the EKM from that header (set by the
  proxy) and append it to the expected transcript; reject if the header is
  absent. Bad signature → generic failure + alert; the challenge is already
  spent.
- One transaction (`SELECT … FOR UPDATE` on the server; re-check
  `deleted_at IS NULL` here — TOCTOU guard against a concurrent archive):
  1. Reject if `spki` is an active key on a device bound to a *different live*
     server (item 4).
  2. Resolve target device:
     - `server.device_id` set (Tailscale) → `Device::add_key(target, spki)`
       (errors if it already has an active mTLS key); audit the addition.
     - else → `from_key(spki)`: if it resolves a **free** (unbound/Untrusted)
       device, use it (re-activate the key); else `Device::create`. Set
       `server.device_id = target`. **No `merge_into`.**
  3. `Device::trust(target, Server)`.
  4. `ServerEnrollmentToken::consume(server_id, token_hash)` — the burn, atomic
     with the bind.
  5. `server.registered_at = now()`.
- Return `RegisterResponse { server_id, device_id }`. (No `central_public_key` —
  dropped; nothing consumes it and shipping an unverified "trust anchor" is a
  footgun. Re-add only with a real consumer + verification story.)

**Remove the device-driven public-server mutation surface.** Operator-first
creation makes `create` (`servers.rs:84`), `edit` (`:115`), and `remove`
(`:152`) redundant, so **delete all three** (and trim now-unused `NewServer` /
`PartialServer` / `ServerDevice` / `AdminDevice` plumbing in this file). Keep
`list` (`:49`) — that's the public mobile list of central servers, unrelated.
bestool no longer self-creates or self-edits server records (noted in the bestool
spec). The separately-landed IDOR fix on `edit` is thus superseded by its
removal; it was correct to land immediately as interim defence.

## Backend: `crates/commons-servers` — stop auto-creating devices

- **`device_auth/mtls.rs::resolve`**: on `from_key` miss, return no device
  (auth fails) instead of `Device::create` (`:32`). After this, an mTLS key
  enters the DB only via `register/complete`. Add the raw-SPKI extraction helper
  here for the register endpoints to share.
- Leave the tailnet path's first-contact behaviour as-is (tailnet membership is a
  trust gate). Verify no flow relies on mTLS first-contact auto-create now that
  enrollment is operator-first.

## Backend: `crates/private-server/src/fns/servers.rs`

- **Add `create`** (`POST /api/servers/create`, `TailscaleAdmin`) — body mirrors
  `NewServer` plus optional `tailscale_identifier`. If given, resolve it (reuse
  `resolve_tailnet_identifier`/`attach_tailscale_device`, `fns/servers.rs:667` &
  `fns/devices.rs:736`) and `Device::create_with_tailscale` before inserting the
  server with that `device_id`. Returns the new server id.
- **Add `delete`** (`TailscaleAdmin`) → `Server::soft_delete`.
- **Add `restore`** (`TailscaleAdmin`) → `Server::restore`.
- **Add `mint_enrollment`** (`TailscaleAdmin`) — `ServerEnrollmentToken::mint`,
  assemble the payload JSON (`api_url` from `PUBLIC_URL`, `server_id`, plaintext
  token; **no `group_id`, no `ca`**), generate a 4-word passphrase (chbs, EFF
  large list), `algae-cli` `encrypt_stream` the payload under
  `Passphrase::new(SecretString::from(passphrase))`, base64 the ciphertext, and
  return `{ ticket, passphrase, expires_at }` (`EnrollmentTicket`). The response
  carries the secret (passphrase + decryptable ticket), so it must be
  admin-gated, non-cacheable, and never logged.
- **Add `enrollment_status`** (or fold into `get_detail`) — `registered_at` plus
  `active_for` (expiry only) so the UI shows pending vs registered. This is the
  *only* place enrollment failure reasons are exposed (the public endpoint is
  opaque, item 5).
- **Remove `import_ticket`** (`:575`) and drop it from `routes()` (`:221`).
- **Keep `attach_tailscale_device`** for post-hoc editing; no longer primary.
- Surface archived servers in `get_detail`/`get_info` with an archived flag;
  exclude from `list_some`/`list_ungrouped`.

Run `just gen-openapi` after these handler changes; commit `openapi.json` +
`api-types.ts` alongside.

## ERRORS.md + `AppError`

The **public** register endpoints return one opaque problem type
(`EnrollmentFailed`, generic detail) for every pre-completion failure — unknown
server, archived, invalid/expired/consumed token, bad nonce, bad signature
(item 5). The specific reason is logged server-side only. Never include the
token in any message/`detail` (item 6).

Internal/admin-facing variants (for logs and `enrollment_status`, not the device
wire): token invalid/expired/consumed, challenge invalid/expired/used,
signature-verification-failed, `ServerArchived`. Add ERRORS.md headings matching
each problem type. The group-non-empty delete keeps its existing problem type.

## Frontend: `private-web`

### New: create-server flow

- Route `/servers/new` and `/groups/:id/servers/new` → a `ServerCreate` page
  reusing the field set from `ServerEdit.tsx` (`EditForm`), minus the raw
  `device_id` input, **plus** a Tailscale identity picker that previews via
  `resolve_tailnet_identifier`. From a group, the group is preselected.
- On submit → `servers.create` → navigate to the **setup** view.

### New: setup / instructions view

After create (and reachable from a not-yet-registered server's detail page):

- Calls `servers.mint_enrollment` and shows install bestool + the
  `bestool canopy register` command, with the encrypted **ticket** in an
  always-visible code block (copy button) and the **passphrase** shown
  prominently and separately (its own box + copy button), noting it must be
  shared over a separate channel — the ticket is useless without it.
- "token expires <relative time>" and a **Reissue** button (re-mints, issuing a
  new ticket AND a new passphrase; old token is invalidated).
- A live status that distinguishes **not yet used → used but not completed →
  registered** (poll `enrollment_status`), so a token consumed without a
  completed bind is visible rather than looking like success.

### Group create → server create

- After `server_groups.create`, redirect to `/groups/:id/servers/new`. Back
  leaves the (legal) empty group.
- Add **Add server** buttons on `GroupDetail.tsx` and the groups list.

### Server detail / delete / archive

- `ServerDetail.tsx`: **Delete (archive)** with confirm → `servers.delete`.
- Banner + setup instructions while `registered_at` is null.
- Archived servers: **archived** state + **Restore** action; keep history.

### Demote "Device" from a first-class concept

- Remove `ImportTicketButton`/`ImportTicketDialog` from `DevicesSearch.tsx` and
  delete `private-web/src/lib/canopyTicket.ts` + `parseCanopyTicket` usage.
- Fold device info on `ServerDetail` into a **collapsed-by-default** Advanced /
  identity section (mTLS key(s), Tailscale identity, connection history). Keep
  `/devices` for power users (trust/untrust/merge), but it is no longer where
  servers are born.
- Group delete: keep the guard; show "move or delete its servers first" (and
  surface archived members as blockers) on the 4xx.

## Tests

Backend (`commons_tests::db` and `::server`):

- `Server::create` ungrouped/grouped; `host` uniqueness only among live rows
  (archived row doesn't block recreating same host).
- `soft_delete` archives, releases + untrusts the device, **deactivates its
  keys**, hides the row from live listings, keeps it in `get_by_id`; `restore`
  brings it back and 409s on a live-host clash.
- `ServerGroup::delete` refuses a group whose only members are archived (guard
  counts archived).
- Token: `mint` returns plaintext + stores only the hash; reissue marks priors
  consumed (exactly one active after); `consume` succeeds once then errors.
- Challenge: `take` is single-use, rejects expired/used/key-mismatch.
- mTLS boundary: an unknown key no longer auto-creates a device (auth fails; no
  row inserted).
- Register endpoints (public-server HTTP test with `mtls-certificate` header):
  - happy path, no pre-bound device → begin returns a nonce; complete with a
    valid signature creates+binds+promotes, `registered_at` set, token consumed;
  - happy path, Tailscale-precreated device → key added to *that* device (no
    second device); refused if it already has an active mTLS key;
  - **bad signature** at complete → generic failure, token still un-consumed,
    challenge spent;
  - **PoP negative**: presenting a victim's SPKI without its private key cannot
    complete;
  - **channel binding**: with `CANOPY_ENROLL_EKM_HEADER` set, `begin` reports
    `channel_binding_required: true`, and `complete` rejects a signature that
    omits the EKM or a request missing the header; with it unset, plain PoP
    completes;
  - key already bound to a *different live* server → rejected; `merge_into` never
    invoked from register;
  - soft-deleted-then-recreated server, same box → re-enrolls only via full
    token + PoP (no silent key-match promotion);
  - all failure modes (unknown/archived server, invalid/expired/consumed token,
    bad nonce) return the **same opaque** response;
  - the token never appears in any error body.
- Removing `import_ticket`: delete `crates/private-server/tests/import_ticket.rs`
  and `crates/database/tests/upsert_from_ticket.rs`.

Frontend e2e (Playwright, `private-web/e2e`):

- create group → lands on create-server-in-group; back leaves empty group;
- create server → setup view shows the encrypted ticket + passphrase + reissue;
- archive a server hides it from lists and shows restore.

## Implementation status

Shipped (backend + frontend + tests): migrations; archival + token/challenge
models; the two-step PoP register endpoints with env-gated channel binding,
no-merge and reject-key-bound-elsewhere guards; no-auto-create at the mTLS
boundary; private-server create/delete/restore/mint_enrollment/enrollment_status;
removal of the public create/edit/remove surface, `import_ticket`, and the
`CanopyTicket` type; passphrase-encrypted enrollment tickets (algae/age-scrypt
under a 4-word chbs passphrase, returned as `{ ticket, passphrase }`); the full
React flow (create → setup ticket+passphrase → archive/restore,
collapsed device section, group→server flow, Import-Ticket UI removed);
ERRORS.md; regenerated OpenAPI + TS types. Backend tests cover the PoP happy
path, bad-signature-keeps-token, opaque errors, Tailscale-precreated add-key,
reject-key-bound-to-another-live-server, archival release/deactivate/hide +
host reuse, and token reissue/single-use.

**Rate-limiting + alerting on `/register/*`** (security model item 3) — DONE.
In-process fixed-window limiter (per source IP 60/min, per server 20/min;
`crates/public-server/src/ratelimit.rs`) returning 429; `tracing::warn!`
alerting (target `enrollment`) on rate-limit trips, PoP signature failures, and
key-already-bound-to-another-live-server attempts. Note: the limiter is
single-process (each replica has its own window) — fine as an abuse backstop,
not a distributed quota. Token-consumed-without-bind alerting is moot here:
`consume` runs inside the bind transaction, so a failed bind rolls the burn
back (nothing to alert on).

**Deferred (not yet done — do not consider the plan complete):**

- **Frontend e2e (Playwright)** tests for the create→setup→archive flow.
- **`register/complete`** re-checks `deleted_at` but not under `SELECT … FOR
  UPDATE`; tighten the archive-vs-register TOCTOU with a row lock.
- A `mint_enrollment`-side test asserting the token never appears in any
  response/error body; a channel-binding (`CANOPY_ENROLL_EKM_HEADER`) test.
- `attach_tailscale_device`'s "already attached" check uses
  `get_by_device_id` (includes archived); switch to `live_by_device_id`.

## Open items / follow-ups (surface, don't drop)

- **Channel-binding rollout:** the feature is in (env-gated on
  `CANOPY_ENROLL_EKM_HEADER`), but whether our proxy can compute and forward a
  TLS exporter value needs investigation before it can be switched on in any
  environment. Until then it stays off and app-layer PoP runs alone.
- **Token TTL configurability:** 7 days is the fixed default for now. Not made
  env-configurable; if it becomes adjustable later it will most likely be a
  per-mint UI control rather than an env var.
- **Hardware-bound device keys (bestool side, not this plan):** the strongest
  defence against a copied key is to make the key non-exfiltratable — bestool is
  expected to bind the device key to a TPM/secure element so it can't be moved to
  another machine without explicit operator action. Captured in the bestool spec
  as a recommendation; no Canopy-side change needed (Canopy still just verifies
  the signature against the presented SPKI).
