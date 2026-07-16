# `AppError` variant coverage — analysis & recommendations — 2026-07-16

Companion to [`logic-audit-2026-07.md`](./logic-audit-2026-07.md). That audit
repeatedly found handlers returning HTTP 500 for conditions that are not server
faults (findings **L11, L15, L16, L17**, the auth-shape issue **H9**, and the
cross-cutting pattern *"`AppError::custom` where a typed variant exists"*). This
report steps back from the individual bugs and asks the structural question:
**what error variants is `AppError` missing, such that `AppError::custom` stops
being the path of least resistance?**

No code changes here — this is a design proposal for discussion.

## The problem

`AppError` (`crates/commons-errors/src/lib.rs`) already models most of the
domain well: it has typed variants for auth, enrollment, rate limiting,
conflicts, upstream failures, and a `BadRequest(String)` whose own doc-comment
states the intent — *"Maps to 400 so callers don't have to chase a generic
500."* But it also exposes a catch-all:

```rust
#[error("{0}")]
Custom(String),           // → 500, problem type /errors/other
pub fn custom(err: impl ToString) -> Self { Self::Custom(err.to_string()) }
```

`AppError::custom` is ergonomic — it takes any `ToString`, so it drops into a
`.map_err(...)` or `.ok_or_else(...)` with no ceremony — and that is exactly
why it has spread to **~40 call sites**, roughly **30 of them reachable on an
HTTP request path**. Every one of those resolves to:

- **HTTP status 500** (the `_ =>` fallback arm in `to_http_status`), and
- **problem type `/errors/other`** (the `Self::Custom(_) => "other"` arm).

The consequences, all observed in the audit:

1. **Client errors masquerade as server faults.** A 400/404/403 condition
   returned as 500 tells the caller "retry, it's our fault" when it never will
   succeed, and pollutes 5xx dashboards/alerting with user typos.
2. **The documented OpenAPI contract is violated.** Several handlers annotate
   `(status = 404, …)` / `(status = 400, …)` but return 500 via `custom`, so the
   checked-in `openapi.json` and the generated TS client lie about the wire
   contract (L15, L16, L17).
3. **`/errors/other` is not machine-matchable.** The whole point of the RFC 7807
   `type` field (documented on `ProblemDetailsSchema`: *"Stable … so callers can
   match on it"*) is defeated when a dozen unrelated conditions share one slug.
4. **No compiler pressure toward the right status.** Because `custom` accepts
   anything and silently yields 500, nothing flags a miscategorised error at
   the call site — the mistake is invisible until someone reads the response.

## Inventory: what the HTTP-reachable `custom` sites *mean*

Classifying every HTTP-path `AppError::custom` by the status it *should* carry:

| Intended semantics | Correct status | Representative call sites | Today |
|---|---|---|---|
| Malformed / missing client input | **400** | `public/versions.rs:551` (bad artifact UUID), `public/artifacts.rs:120` (bad version range), `public/timesync.rs:17` (payload size), `public/events.rs:57` + `private/issues.rs:577,778` + `private/incidents.rs:507` (empty ref/note), `database/devices.rs:280` (merge same id) | 500 |
| Resource not found | **404** | `public/versions.rs:557` (artifact), `private/bestool.rs:186` (snippet), `private/devices.rs:938` (tailnet node) | 500 |
| Caller authenticated but not authorised | **403** | `public/statuses.rs:214` (create statuses), `public/statuses.rs:337` (read severities) | 500 |
| State/precondition violation | **409** | `private/versions.rs:492` (can't demote non-latest published version) | 500 |
| SQL playground: the operator's own query failed | **422** | `private/sql.rs:153,159,171` (`format_db_error` on the user query/txn), `:170` (60s timeout) | 500 |
| Feature not configured for this deployment | **503** | `private/sql.rs:129` (`RO_DATABASE_URL` unset) | 500 |
| Proxied upstream dependency failed | **502** | `public/versions.rs:566` (artifact download proxy), all of `commons-servers/tailnet_directory.rs` (~13 Tailscale API request/decode sites) | 500 |
| Genuine internal fault | **500 (correct)** | crypto (`device_auth/keygen.rs`), CSPRNG + config-parse (`mcp_tokens.rs`, `server_enrollment_*.rs`), HTTP-client build, serde-serialize of internal data, missing `PUBLIC_URL`, key-encryption, query-history insert (`private/sql.rs:141,146,176`, `private/servers.rs:1053,1064,1079`, `private/devices.rs:633,650`) | 500 |

Only the last row is behaving correctly today — and even there, `/errors/other`
is a poor type slug for a genuine fault.

Two call sites are **non-HTTP** (`jobs/bin/chrome_versions.rs`,
`database/bin/seed.rs`): these run in CLI binaries where the status mapping is
never consulted, so `Custom` is harmless and needs no change. (The `de::Error::custom`
calls in `healthcheck_severities.rs` are serde deserializer errors, unrelated
to `AppError`.)

## Recommended new variants

The design goal is that **for every HTTP outcome a handler can produce, there is
a typed variant whose name makes the wrong status obvious at the call site**,
leaving `Custom` for non-HTTP contexts only. Proposed additions to the enum:

### 1. `NotFound(String)` → 404, slug `resource-not-found`
```rust
/// A specific resource was looked up and not found. Use for `Option`
/// lookups that don't go through diesel's `NotFound` (which already maps
/// to 404). Reuses the `resource-not-found` type slug so clients match one
/// slug for "not found" regardless of how the miss was produced.
#[error("not found: {0}")]
NotFound(String),
```
Covers `bestool.rs:186`, `devices.rs:938`, `public/versions.rs:557`. Fixes L15,
L16 (the not-found half). Reusing the existing `resource-not-found` slug keeps
the two 404 sources coherent for callers.

### 2. `Forbidden(String)` → 403, slug `forbidden`
```rust
/// Caller is authenticated but not authorised for this specific resource.
/// Distinct from `AuthInsufficientPermissions` (which is role-based) — this
/// is per-object (e.g. a device acting on a server it isn't attached to).
#[error("forbidden: {0}")]
Forbidden(String),
```
Covers `public/statuses.rs:214,337`, where a device acting on another server's
resources currently gets 500 instead of 403.

### 3. `Unprocessable(String)` → 422, slug `unprocessable`
```rust
/// Request is well-formed but violates a state/business rule (as opposed
/// to `BadRequest`, which is malformed input). 422 rather than 409 when
/// there is no conflicting resource, just an invalid transition.
#[error("unprocessable: {0}")]
Unprocessable(String),
```
Covers the version demote-guard (`private/versions.rs:492`) and the SQL
playground's *user-query* failures (`sql.rs:153,159,170,171`) — those are an
expected outcome of a SQL console, not a server fault, and 422 lets the UI show
"your query failed" distinctly from "the server broke". (If the team prefers,
the demote guard alone reads equally well as `Conflict` → 409, which already
exists; the SQL cases are the ones that genuinely need a new variant.)

### 4. `Unavailable(String)` → 503, slug `unavailable`
```rust
/// A feature or dependency is not available in this deployment/right now —
/// e.g. an optional feature whose config is unset, or a transient capacity
/// limit. Signals "try later / not enabled here", not "your request was bad".
#[error("unavailable: {0}")]
Unavailable(String),
```
Covers `sql.rs:129` (`RO_DATABASE_URL` unset). Generalises the one-off
`AuthTailnetDirectoryUnavailable` pattern.

### 5. `Internal(String)` → 500, slug `internal`
```rust
/// A genuine server-side fault (crypto failure, CSPRNG, serialisation of
/// server-controlled data, a misconfiguration, an invariant violation). The
/// message is logged; a caller-safe summary is returned. Prefer this over
/// `Custom` so the type slug is `internal`, not `other`, and so 500s are a
/// deliberate choice rather than a fallthrough.
#[error("internal error: {0}")]
Internal(String),
```
Covers every "genuine internal fault" row above. This is the key move: it lets
`Custom` be **removed from HTTP code entirely** (or retained only for the two
CLI binaries), so that a `500` is always something a developer typed on
purpose. `to_http_status` keeps its `_ => INTERNAL_SERVER_ERROR` fallback as a
backstop, but no HTTP handler should rely on it.

### Existing variants to prefer (no change needed)
- **`BadRequest(String)` → 400** for the malformed-input row (already exists,
  just underused — fixes L17 and the `events.rs`/`timesync.rs`/`artifacts.rs`
  sites).
- **`Upstream(String)` → 502** for the proxy/Tailscale-directory row (already
  exists with exactly the right doc-comment about logging the sensitive detail
  server-side). The `tailnet_directory.rs` and artifact-download sites should
  use it instead of `custom`.
- **`Conflict(String)` → 409** as an alternative home for the demote guard.

## Call-site migration map

| File:line (today `custom`) | Recommended variant |
|---|---|
| `public/versions.rs:551`, `public/artifacts.rs:120`, `public/timesync.rs:17`, `public/events.rs:57`, `private/issues.rs:577,778`, `private/incidents.rs:507`, `database/devices.rs:280` | `BadRequest` |
| `public/versions.rs:557`, `private/bestool.rs:186`, `private/devices.rs:938` | `NotFound` |
| `public/statuses.rs:214,337` | `Forbidden` |
| `private/versions.rs:492` | `Unprocessable` (or `Conflict`) |
| `private/sql.rs:153,159,170,171` | `Unprocessable` |
| `private/sql.rs:129` | `Unavailable` |
| `public/versions.rs:566`, `commons-servers/tailnet_directory.rs:*` | `Upstream` |
| `device_auth/keygen.rs:*`, `mcp_tokens.rs:*`, `server_enrollment_*.rs:*`, `private/sql.rs:141,146,176`, `private/servers.rs:1053,1064,1079`, `private/devices.rs:633,650`, `*/statuses.rs` HTTP-client build | `Internal` |
| `jobs/bin/chrome_versions.rs:*`, `database/bin/seed.rs:109` | leave as `Custom` (non-HTTP) |

## Wiring each new variant

For every added variant, three places in `crates/commons-errors/src/lib.rs`
must stay in lockstep (the audit's L11 is precisely a variant that was added
without a `to_http_status` arm, so it fell through to 500):

1. `to_http_status` — add the explicit status arm.
2. `to_problem_details` — add the type-slug arm (the `match` is exhaustive, so
   the compiler enforces this one).
3. **`ERRORS.md`** — add a heading matching the new problem type (per the repo
   rule: *"Update ERRORS.md when adding new error types, the heading must match
   the error problem type"*).

Note an asymmetry worth fixing while here: `to_problem_details`'s slug `match`
is exhaustive (compiler-checked), but `to_http_status` ends in `_ =>
INTERNAL_SERVER_ERROR`, so a new variant silently defaults to 500 there. **L11
is exactly this bug** (`VersionParse` and `Header` have no status arm).
Consider making `to_http_status` exhaustive too — remove the wildcard and list
every variant — so adding a variant forces a deliberate status choice at
compile time. That single change would have prevented L11 and would prevent the
next one.

## Guidance: when `custom` is still acceptable

- **CLI binaries / non-Axum contexts** (`jobs`, `bin/seed`, migrations tooling):
  the `IntoResponse` mapping is never invoked, so `Custom` is fine — though
  `Internal` reads as clearly there too.
- **Never in an Axum handler or an extractor**: if it can reach `IntoResponse`,
  it must use a typed variant. A lint idea: once `Custom` is out of HTTP code,
  gate it behind a `#[doc(hidden)]` / `#[deprecated(note = "use a typed variant
  in HTTP paths")]` marker so new HTTP-path uses stand out in review.

## Relationship to the logic audit

Adopting variants 1–5 mechanically resolves the following audit findings and
downgrades the recurring pattern from "bug per site" to "handled by type":

- **L11** — `VersionParse`/`Header` → 500: fixed by adding their status arms
  (and, structurally, by making `to_http_status` exhaustive).
- **L15** — `get_snippet` 500 instead of 404: `NotFound`.
- **L16** — `attach_tailscale` 500/400 instead of 404: `NotFound` (and aligns
  the two divergent attach handlers on one variant).
- **L17** — empty ref/note validation 500 instead of 400: `BadRequest`.
- Cross-cutting *"`AppError::custom` where a typed variant exists"* — retired by
  removing `Custom` from HTTP paths.

`H9`/`M15`/`M16` (auth annotation without enforcement) are a separate
class — an extractor/authz gap, not an error-mapping gap — and are not fixed by
new variants, though `Forbidden`/an auth variant is the correct thing those
handlers should return once they *do* enforce.
