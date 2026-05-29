# Spec: `bestool canopy register` (hand-off to a bestool agent)

This is a self-contained spec for implementing a new bestool subcommand that
enrolls a machine as a Canopy server using Canopy's new **operator-first**
enrollment flow. It is written for an agent working in the **bestool** repo; it
has no dependencies on Canopy internals beyond the HTTP contract described here.

## Background: what changed in Canopy

Canopy is moving from "device-first / ticket-pull" to "operator-first /
token-push" server enrollment.

- **Old flow (being removed):** bestool generated a `CanopyTicket` (via
  `bestool t meta-ticket`) carrying the machine's own public key; an operator
  pasted that ticket into Canopy to create the server.
- **New flow:** an operator creates the server *in Canopy first*, and Canopy
  hands them a base64 **enrollment blob**. The operator runs
  `bestool canopy register <base64>` on the machine, which claims the
  pre-created server over mTLS by presenting a single-use token.

Net effect for bestool:

1. **Add** `bestool canopy register <base64>`.
2. **Remove** `bestool t meta-ticket` (the `CanopyTicket` producer) entirely —
   it has zero use once the Canopy change lands, so there's no deprecation
   window. Delete the command and any now-dead `CanopyTicket`-generation code it
   was the sole user of. Do not remove other `t` subcommands.

## The enrollment blob

The argument to `register` is a base64url (accept all base64 variants:
standard, no-pad, url-safe, url-safe-no-pad — mirror Canopy's lenient decoding)
encoding of this JSON:

```jsonc
{
  "v": "enroll-1",                              // version tag; reject anything else
  "api_url": "https://<canopy public server>",  // device-facing API origin
  "server_id": "<uuid>",
  "group_id": "<uuid>",                          // informational; display, don't require server-side
  "token": "<base64url of 32 random bytes>"      // the single-use enrollment secret
}
```

- Validate `v == "enroll-1"`; fail clearly otherwise.
- `token` is a bearer secret — never log it.
- There is no CA in the blob: `api_url` is served with a webPKI (Let's Encrypt)
  certificate, so verify the server's TLS against the system root store. Do not
  pin a CA.

## What `register` does

1. **Parse** the blob; validate version and required fields
   (`api_url`, `server_id`, `token`).
2. **Establish the machine's mTLS identity.** Use the machine's existing
   bestool/Canopy client key+certificate if one is already provisioned; if not,
   generate a keypair and a self-signed client certificate (ECDSA, matching what
   Canopy expects — Canopy identifies devices by the certificate's
   SubjectPublicKeyInfo bytes, not by a CA chain, so self-signed is fine).
   Persist this identity in bestool's usual config/state location so subsequent
   Canopy calls reuse it.
3. **Call the register endpoint** (below) over HTTPS, presenting the client
   certificate (mTLS). Verify the server's TLS certificate against system roots
   (webPKI); no CA pinning.
4. **Persist the result**: store `server_id`, the returned `device_id`, and
   `api_url` so the agent knows it is enrolled and where to report. If the
   response includes `central_public_key`, persist it as the server-trust
   anchor.
5. **Report** success to the operator (server id, device id). On failure, print
   the Canopy error (problem-type + detail) verbatim and exit non-zero.

Make `register` **idempotent-friendly**: re-running with an already-consumed
token should surface Canopy's "token consumed" error cleanly (the machine is
likely already enrolled); detect the "already enrolled with this identity" case
and treat it as success where possible.

## HTTP contract

### `POST {api_url}/servers/register`

- **Transport:** HTTPS with mTLS — present the machine client certificate.
  Canopy's proxy accepts arbitrary client certs; the certificate's public key is
  the device identity.
- **Request body (JSON):**
  ```jsonc
  { "server_id": "<uuid>", "token": "<token>" }
  ```
  (`group_id` may be included; Canopy ignores/cross-checks it.)
- **Success `200`:**
  ```jsonc
  {
    "server_id": "<uuid>",
    "device_id": "<uuid>",
    "central_public_key": "<PEM, optional>"
  }
  ```
- **Errors** (RFC-7807-style problem JSON, as elsewhere in Canopy):
  - `401/403` invalid token, `410/403` expired token, `409` consumed token,
    `404` unknown server, `409` archived server.
  - Surface `title`/`detail` to the operator; map to a non-zero exit.

## CLI shape

```
bestool canopy register <BLOB_BASE64>
```

- Single positional arg: the base64 blob (as copied from Canopy's setup screen).
- Consider a `--config <path>` to override where the mTLS identity/state is
  stored, consistent with other bestool subcommands.
- Place under a `canopy` subcommand group (new if it doesn't exist).

## Out of scope / notes

- Status/metric reporting after enrollment is unchanged — this command only
  performs enrollment. If bestool currently self-creates the server record via
  Canopy's public `POST /servers` after coming online, that path is now
  redundant (the operator creates the server). Flag to the Canopy side whether
  bestool should stop calling it; do not remove Canopy's endpoint from here.
- Token lifetime is 7 days, single-use, reissuable from Canopy — bestool doesn't
  manage token lifecycle, it just presents whatever is in the blob.
