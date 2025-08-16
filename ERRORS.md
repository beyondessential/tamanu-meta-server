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

## Other

An unclassified error.
