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

## Other

An unclassified error.
