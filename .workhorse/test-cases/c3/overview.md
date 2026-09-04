# Extract OpenAPI part as separate published crate

Scenarios verifying the published API client. The publish step itself belongs to
card L3, so nothing here exercises the registry.

## Generation

- [x] Generating the client reads only the repository, with no running canopy and no network (verifies spec: APIC)
- [x] Regenerating from an unchanged document leaves the committed source unchanged (verifies spec: APIC)
- [x] `just check-generated` fails when the document or the client is stale, naming the files that differ (verifies spec: APIC)
- [x] Generation fails rather than emitting an untyped JSON body for an operation it cannot type (verifies spec: APIC)
- [x] Generation fails when a schema declared to hold a credential secret is missing from the document
- [ ] Generation fails when a schema accepting further keys has nowhere to carry them

## Typed surface

- [x] Every operation in the document has a method taking and returning generated types (verifies spec: APIC)
- [x] A method name comes from its path, with the verb distinguishing a path served by several verbs (verifies spec: APIC)
- [x] A response that is a map with a declared value type is a map of that type, not untyped JSON (verifies spec: APIC)
- [x] A response that is an array of a declared type is a vector of that type (verifies spec: APIC)
- [x] A schema carrying arbitrary further keys alongside declared fields both sends and reads those keys (verifies spec: APIC)
- [x] Further keys are flattened onto the object rather than nested under a field (verifies spec: APIC)

## Generated types

- [x] A credential secret is readable through its value but absent from debug output (verifies spec: APIC)
- [x] A credential secret serialises as its plain value, so the wire is unchanged (verifies spec: APIC)
- [x] A field the document describes as a timestamp is a timestamp rather than text (verifies spec: APIC)
- [x] A struct is constructed without naming every field, so a schema gaining a field leaves call sites working (verifies spec: APIC)
- [ ] Adding an optional property to a schema is not a semver-breaking change to the crate (verifies spec: API)

## Calling canopy

- [x] A request reaches the transport with a path-only target, leaving host and scheme to it (verifies spec: APIC)
- [x] A path parameter is substituted into the target (verifies spec: APIC)
- [x] An unsuccessful status surfaces as an error carrying that status and the body (verifies spec: APIC)
- [x] A failure to obtain any response is distinct from a response reporting failure (verifies spec: APIC)
- [x] A success body that is not the declared JSON is a decode failure, not an HTTP error (verifies spec: APIC)
- [x] A request body at or above the compression threshold is gzipped, with the encoding header set to match
- [x] A request body below the threshold is sent uncompressed
- [x] The crate builds with no HTTP client in its dependency tree (verifies spec: APIC)
