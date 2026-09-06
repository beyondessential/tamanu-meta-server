---
id: APIC
---

# Public API client crate

Canopy publishes `bes-canopy-api`, a Rust client for its public API, generated from the OpenAPI document Canopy holds for that API.
A consumer reaching Canopy from Rust depends on the published crate rather than describing the API itself, so the wire types it works with are the ones Canopy declares.

## Generated in the Canopy repository

The document the crate is generated from is held in Canopy's own source, so generation reads only the repository.

The generated source is committed alongside that document, so a change to the client's surface appears in the change that causes it and is reviewable there.
Regenerating from an unchanged document produces an unchanged crate.

## One version for the document and the crate

The document and the crate carry the same version, because the crate is derived from the document and its surface moves only when the document or the generator moves.
The document declares the version and the crate takes it, so the two cannot drift apart.

The version describes the crate's surface.
A change in what the generator emits raises it as a change to the document would, even where the API the document describes is untouched.

The version is not an input to generation.
A release generates the crate, judges the change against the published crate as [API](api-compatibility.md) defines, and then records the resulting version, so the version is settled after the change it describes is final.

A compatible change raises the minor or patch version.
A break raises the major version, and that raise is where the coordination [API](api-compatibility.md) requires is recorded.

## Every operation is typed

Each operation in the document has a method on the client, taking and returning types generated from the document's schemas.
Method names are derived from the operation's path, with the verb distinguishing the methods of a path served by more than one verb, so a consumer's call sites depend on the path rather than on the order operations were generated in.

An operation is reached through its generated types rather than through an untyped JSON body.
A schema the generator cannot express is a defect in the document or in the generator, resolved there rather than by degrading that operation to untyped JSON.

A schema that is a typed object carrying arbitrary further keys generates a struct with its declared fields and a map holding the rest, so a consumer both sends and reads those further keys.
A schema that is a map with a declared value type generates a map of that type.

## The consumer supplies the transport

The crate leaves how a request reaches Canopy to its consumer, and depends on no particular HTTP client.
Every generated method works over whichever transport the consumer supplies.

A transport receives a request whose target is a path, and resolves the host, the scheme, and the authentication itself.
It returns Canopy's response as given, unsuccessful statuses included, because endpoints give particular statuses a meaning only the client can read.
A failure to obtain any response is reported as distinct from a response that reports failure.

The client turns an unsuccessful status into an error carrying that status and the body, so a consumer can branch on the status an endpoint documents.

## What the generated types carry

A field holding a credential secret is readable but does not appear in debug output, so a consumer logging a response does not disclose it.
A field the document describes as a timestamp is typed as a timestamp rather than as text.

A schema gaining a field leaves construction of that schema's type working for a consumer that does not set it.

The crate records the document it was generated from, so a document that changed without the version moving with it can be told from one that did not.
