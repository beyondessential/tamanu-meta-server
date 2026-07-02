# Public MCP access via bearer tokens

Expose the read-only MCP fleet-query interface (spec `MCP`) on the public
(internet-facing) server, gated by bearer tokens, so agents outside the
tailnet — specifically Claude's admin Connections feature, which attaches a
`Bearer` credential to MCP calls through its credential proxy — can query the
fleet. The tailnet mount at `/api/mcp` on the private server is unchanged.

## Decisions (locked in)

- **Static bearer tokens**, not OAuth. Claude's credential proxy documents
  exactly this shape for MCP servers ("a Bearer token with Allowed websites
  set to the MCP host"); its OAuth credential types are not documented to work
  for MCP connections, and the classic connector path rejects
  machine-to-machine grants outright. Every available scheme parks a
  long-lived secret in Claude's credential store, so the mitigations live on
  our side: hashed at rest, revocable, rotatable, last-use visible.
- **Managed from the admin UI** (Settings), backed by admin-gated
  private-server endpoints. Mint shows the token once; list shows name,
  creator, created/expires/last-used; revoke is immediate.
- **Fixed 1-year expiry**, not configurable at mint. A fleet-wide alert is
  raised 15 days before a token expires so rotation happens on schedule
  instead of as an outage.

## Architecture

`CanopyMcp` currently lives in `crates/private-server/src/mcp.rs` and only
uses `AppState.db`. The private server depends on the public server (it nests
the device routes at `/public`), so the reuse has to go the other way:

- **New crate `crates/mcp`** (package `mcp`): the `CanopyMcp` tool router and
  `service()` constructor, holding a `database::Db` instead of the private
  `AppState`. Deps: `rmcp`, `database`, `commons-errors`, `commons-types`,
  serde/tracing. The `require_tailnet_user` middleware stays in
  private-server (it is about the tailnet surface, not the MCP itself).
- **Private server**: mounts `mcp::service(db)` at `/api/mcp` exactly as
  before, behind `require_tailnet_user` + `reject_tagged_devices`.
- **Public server**: new `pub fn mcp_router() -> Router<AppState>` mounting
  `mcp::service(db)` at `/mcp` behind the new bearer gate. Deliberately NOT
  inside `public_server::routes()`, so it does not leak onto the private
  server's `/public` nest nor into the device OpenAPI spec; `main.rs` and the
  `commons-tests` server harness compose it in.

No ingress change: the nginx ingress already uses
`auth-tls-verify-client: optional_no_ca`, so requests without a client cert
reach the binary and auth is enforced per-route.

## Token storage — `mcp_tokens`

Migration via `just migration add_mcp_tokens`:

```sql
CREATE TABLE mcp_tokens (
  id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name          TEXT NOT NULL,
  token_hash    BYTEA NOT NULL UNIQUE,
  created_by    TEXT NOT NULL,           -- tailnet login of the minting admin
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  expires_at    TIMESTAMPTZ NOT NULL,    -- always created_at + 1 year, set in code
  revoked_at    TIMESTAMPTZ,
  last_used_at  TIMESTAMPTZ
);
```

Model `database::mcp_tokens::McpToken` mirrors `server_enrollment_tokens`:
256-bit CSPRNG token, plaintext `canopy_mcp_<base64url>` handed out once,
only the unsalted SHA-256 digest persisted (same rationale comment: the token
is full-entropy, don't "upgrade" to HMAC/argon), whole-digest equality in the
SQL `WHERE`. Functions: `mint(name, created_by)` (expiry fixed at +1 year in
code), `find_active(plaintext)` (`revoked_at IS NULL AND expires_at > now()`),
`revoke(id)`, `list()`, `touch_last_used(id)` (throttled: skip the write if
`last_used_at` is fresher than a minute), `expiring_within(days)` for the
alert sweep.

The 1-year term is a constant in the model, not an argument — mint cannot be
talked into a longer-lived token.

## Public bearer gate

Middleware on the `/mcp` mount:

1. Parse `Authorization: Bearer <token>`; missing/malformed → 401
   problem-details with `WWW-Authenticate: Bearer` (new `AppError` variant,
   documented in ERRORS.md).
2. Hash and look up an active token row; miss → same 401 (no distinction
   between unknown, revoked, and expired to a caller).
3. Failed attempts are rate-limited per client IP with the existing
   `RateLimiter` in public-server state (same backstop the enrollment
   endpoints use).
4. Success: `tracing::info!(token = %row.name, "mcp request")` for
   attribution (parity with the tailnet mount logging the user login), and
   `touch_last_used`.

The token itself is never logged. `/.well-known/oauth-*` must keep 404ing on
the public surface too, so MCP clients don't try to negotiate OAuth.

## Admin surface

- `crates/private-server/src/fns/mcp_tokens.rs`: `list`, `mint`, `revoke`,
  all `TailscaleAdmin`-gated, mounted at `/api/mcp_tokens`. `mint` returns
  the plaintext once. `just gen-openapi` after.
- React: a new Settings tab (MCP access) listing tokens with
  name/creator/created/expires/last-used/revoked, a mint dialog (name field →
  show-once token with copy button), and revoke with confirm. Expiring-soon
  (≤15 days) rows get a visible warning state.
- Playwright spec in `private-web/e2e/` covering mint → token shown once →
  row listed → revoke; seed helper extended as needed.

## Expiry alert

Issues attach to exactly one server or group (`issues_scope_exactly_one`
CHECK), and incidents/Slack are strictly per-group — a scope-less "fleet"
issue is unrepresentable. The established idiom for a fleet-wide condition is
fan-out per group, as the backup preflight sweep does for canopy's own
identity problems.

So: `sweep_token_expiry` in `database::mcp_tokens`, called from the
`monitor.rs` minute loop (like `sweep_key_expiry`). When any non-revoked
token is within 15 days of `expires_at`, raise a group-scoped issue on every
group via `raise_group_event` with a single stable ref (`mcp-token-expiry`),
severity Error (the incident-opening floor — Warning would never page),
message naming the expiring token(s) and their expiry dates; when none are
expiring, emit the `active: false` recovery. Revoking or replacing the token
resolves the alert on the next tick.

## Specs and docs

- Amend `.workhorse/specs/private-server/mcp.md` "Access and identity": add
  the second access path — token-authenticated agents on the public surface,
  token lifecycle (admin-minted, named, 1-year fixed expiry, revocable,
  attributable), expiry alerting. The "never on the device-facing surface"
  sentence becomes "never authenticated by device identity" (the mount shares
  the device-facing binary but not its auth).
- ERRORS.md entry for the new auth error variant(s).
- PR description carries the operator setup guide (mint token in Settings →
  Claude admin: plugin with `.mcp.json` pointing at `https://<host>/mcp` +
  Bearer credential with Allowed websites set to the host).

## Done when

- `/api/mcp` behaves exactly as before on the tailnet surface.
- `/mcp` on the public binary: 401 without/with bad token, tool calls succeed
  with a minted token, revoked/expired tokens are refused, failures are
  rate-limited.
- Admin can mint/list/revoke in the UI (Playwright-covered), token plaintext
  appears exactly once.
- A token within 15 days of expiry raises the fleet-wide alert; revocation
  resolves it.
- Spec, ERRORS.md, openapi artefacts updated; per-package tests green.
