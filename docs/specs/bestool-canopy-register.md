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
  `bestool canopy register` on the machine (feeding it the blob), which claims
  the pre-created server over mTLS via a **challenge/response that proves the
  machine holds the private key** behind the certificate it presents.

Net effect for bestool:

1. **Add** `bestool canopy register` (blob read from stdin/file — see CLI shape).
2. **Remove** `bestool t meta-ticket` (the `CanopyTicket` producer) entirely —
   it has zero use once the Canopy change lands, so there's no deprecation
   window. Delete the command and any now-dead `CanopyTicket`-generation code it
   was the sole user of. Do not remove other `t` subcommands.

## The enrollment blob

The blob is a base64url (accept all base64 variants: standard, no-pad, url-safe,
url-safe-no-pad — mirror Canopy's lenient decoding) encoding of this JSON:

```jsonc
{
  "v": "enroll-1",                              // version tag; reject anything else
  "api_url": "https://<canopy public server>",  // device-facing API origin
  "server_id": "<uuid>",
  "token": "<base64url of 32 random bytes>"      // the single-use enrollment secret
}
```

- Validate `v == "enroll-1"`; fail clearly otherwise.
- `token` is a bearer secret — **never log it**, and read the blob from stdin or
  a file, never an argv positional (see CLI shape), so it doesn't land in shell
  history or `ps`/`/proc/<pid>/cmdline`.
- There is no `group_id` and no CA in the blob. `api_url` is served with a webPKI
  (Let's Encrypt) certificate, so verify the server's TLS against the system root
  store; do not pin a CA.

## What `register` does

1. **Read + parse** the blob (from stdin/file); validate version and required
   fields (`api_url`, `server_id`, `token`).
2. **Establish the machine's mTLS identity.** Use the machine's existing
   bestool/Canopy client key+certificate if one is already provisioned; if not,
   generate a keypair and a self-signed client certificate (ECDSA — Canopy
   identifies devices by the certificate's SubjectPublicKeyInfo (SPKI) bytes, not
   by a CA chain, so self-signed is fine). Persist this identity in bestool's
   usual config/state location so subsequent Canopy calls reuse it. **You must
   retain the private key** — enrollment now requires signing a challenge with
   it.
3. **Run the two-step enrollment handshake** (see HTTP contract), presenting the
   client certificate (mTLS) on both calls and verifying the server's TLS against
   system roots:
   - `begin` → receive a `nonce`.
   - Sign the transcript with the machine's private key, `complete` → bound.
4. **Persist the result**: store `server_id`, the returned `device_id`, and
   `api_url` so the agent knows it is enrolled and where to report.
5. **Report** success to the operator (server id, device id). On failure, print
   Canopy's error and exit non-zero. Note Canopy's register errors are
   intentionally **opaque** ("enrollment failed") and do not distinguish unknown
   server / bad token / bad signature — don't try to branch on the reason.

Make `register` **idempotent-friendly**: if the machine is already enrolled with
this identity (the token has been consumed), detect that and treat it as success
where possible rather than erroring.

## HTTP contract

Both calls are HTTPS with mTLS (present the machine client certificate) to
`api_url`. The endpoint is rate-limited per server.

### Step 1 — `POST {api_url}/servers/register/begin`

- **Body:** `{ "server_id": "<uuid>", "token": "<token>" }`
- **Success `200`:** `{ "nonce": "<base64 of 32 bytes>" }`
- The token is **not** consumed here; the challenge nonce is short-lived
  (~minutes).

### Step 2 — `POST {api_url}/servers/register/complete`

- **Signature:** sign the transcript `nonce ‖ server_id ‖ SPKI` (the exact
  concatenation/encoding will be pinned by Canopy — coordinate the byte layout;
  e.g. raw nonce bytes ‖ server_id UUID bytes ‖ DER SPKI bytes) with the
  machine's private key, using the algorithm matching the cert key (ECDSA).
- **Body:** `{ "server_id": "<uuid>", "nonce": "<from begin>", "signature": "<base64>" }`
- **Success `200`:** `{ "server_id": "<uuid>", "device_id": "<uuid>" }`
- **Errors:** RFC-7807-style problem JSON with a single opaque "enrollment
  failed" for all failure modes (unknown/archived server, invalid/expired/
  consumed token, bad/expired/used nonce, bad signature). Surface `title`/`detail`
  to the operator and exit non-zero.

Canopy verifies the signature against the SPKI of the cert presented on the
`complete` call (which must match the one presented at `begin`) — this is the
proof-of-possession. Only on success does Canopy consume the token and bind the
device.

## CLI shape

```
bestool canopy register            # reads the blob from stdin
bestool canopy register --blob-file <path>
```

- Read the blob from **stdin by default** (or `--blob-file`); do **not** accept it
  as an argv positional (keeps the secret out of process listings and history).
- Consider `--config <path>` to override where the mTLS identity/state is stored,
  consistent with other bestool subcommands.
- Place under a `canopy` subcommand group (new if it doesn't exist).

## Out of scope / notes

- Status/metric reporting after enrollment is unchanged — this command only
  performs enrollment. If bestool currently self-creates the server record via
  Canopy's public `POST /servers` after coming online, that path is now redundant
  (the operator creates the server). Flag to the Canopy side whether bestool
  should stop calling it; do not remove Canopy's endpoint from here.
- Token lifetime is 7 days, single-use, reissuable from Canopy — bestool doesn't
  manage token lifecycle, it just presents whatever is in the blob.
- Canopy no longer returns a `central_public_key` (it was unused). If a real
  server-trust anchor is added later, this spec will be updated with its
  verification story.
