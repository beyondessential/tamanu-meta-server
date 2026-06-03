# API Errors

## Environment

Issued with an environment variable is not present or in the wrong format.

This should never be exposed over the API.

## Header

Issued when an HTTP Header is missing or malformed.

## Version Parse

Issued when a version or version range in URLs or API bodies is not parseable.

## Database

Database and query errors.

## Render

HTML template errors.

## IO

I/O errors, typically issued when handling too-large bodies.

## Resource not found

Issued when a database resource (such as a version, server, or other entity) cannot be found.

## No matching versions

Issued when a version range is valid, but does not match any of the available versions.

## Unusable range

Issued when a version range is syntactically valid, but not usable to obtain concrete versions.

## Timesync

Issued for the /timesync endpoint.

## Auth: missing certificate

Issued when a client certificate is required but not provided.

## Auth: invalid certificate

Issued when the provided client certificate is malformed, expired, revoked, or otherwise invalid.

## Auth: certificate not found

Issued when the provided certificate is well-formed but does not match any known device identity.

## Auth: insufficient permissions

Issued when the authenticated device is valid but lacks the necessary role.

## Auth: failed

Issued when authentication fails for unspecified reasons.

## Device has no server

Issued when a device tries to submit an event but is not registered against
any server. Devices must be linked to a server (`servers.device_id`) before
they can report issues.

## Source manual forbidden

Issued when an event submission to the public API uses `source = "manual"`.
That source is reserved for operator-submitted events via the private API.

## Auth: tailnet directory unavailable

Issued when the private server can't reach the Tailscale control plane to
resolve a caller's tailnet IP to a node identity. The path to recover is
operator-side: check the OAuth credentials and the `TAILSCALE_TAILNET`
config, and confirm the device-list refresh loop is making progress.

## Auth: tailnet node not permitted

Issued when a tailnet caller's resolved node identity is missing the tag
required by `TAILSCALE_REQUIRED_TAG`. The node is on the tailnet, but not
one that's allowed to authenticate as a device.

## Tagged device not allowed

Issued when a tagged-device caller (no `Tailscale-User-Login` header,
source IP in the Tailscale CGNAT or ULA ranges) hits any private-server
route outside `/public/...`. Those routes are for human admins and
internal callers only.

## Device tailscale node already claimed

Issued by the admin attach-tailscale flow when the requested node id
is already attached to a different device row. Resolve with the merge
flow if those two rows represent the same physical machine.

## Device merge conflict

Issued by the admin device-merge flow when source and target both
hold tailscale identity, or both are attached to a server. The
operator must clear one side first (detach tailscale or null out
`servers.device_id`) before retrying the merge.

## Bad request

Issued when a client sends syntactically or semantically invalid
input. Body content explains the specific issue (e.g. malformed
ticket, unsupported ticket version, unparseable URL).

## Conflict

Issued when the requested change conflicts with existing state in
a way the operator can resolve (e.g. importing a ticket whose
canonical URL is already claimed by a different server id). Body
content explains the conflict.

## Auth: tailnet identity missing

Issued on the private-server's `/public/...` mount when the
extractor can't resolve the caller to a tailnet identity. Causes,
in order of likelihood:

- `CLIENT_IP_SOURCE` doesn't match what the upstream proxy emits,
  so `ClientIp` resolves to the wrong address.
- The caller's IP isn't in Tailscale's CGNAT v4 (`100.64.0.0/10`)
  or ULA v6 (`fd7a:115c:a1e0::/48`) ranges — i.e. they aren't
  reaching us through the Tailscale ingress.
- The IP *is* in tailnet space, but the directory cache doesn't
  know about it: the node may have just joined the tailnet and a
  refresh hasn't happened yet, or the directory background task
  is failing.

## Enrollment failed

Returned by the public-server enrollment endpoints
(`/servers/register/begin`, `/servers/register/complete`) for *every*
pre-completion failure: unknown or archived server, invalid/expired/consumed
enrollment token, invalid/expired/already-used challenge nonce, a key already
bound to a different live server, or a bad proof-of-possession signature.

The response is deliberately uniform (HTTP 403, no distinguishing detail) so
the endpoint can't be used as an existence/lifecycle oracle for a probed
`server_id`. The specific reason is logged server-side. If you're enrolling a
server and hit this, re-mint the token from the admin UI (it may have expired
or already been used) and confirm the box is presenting the same certificate
across `begin` and `complete`.

## Rate limited

Returned (HTTP 429) when a caller exceeds the enrollment endpoints'
in-process rate limit — currently per source IP and per target server over a
one-minute window. Enrollment is a rare, human-paced operation, so the budgets
are generous for legitimate use; hitting this means an unusual volume of
`/servers/register/*` traffic (a token-guesser, a griefer burning challenges,
or a misbehaving client retrying in a tight loop). Trips are logged under the
`enrollment` target for alerting. Back off and retry after the window.

## Other

An unclassified error.
