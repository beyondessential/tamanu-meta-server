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

## Other

An unclassified error.
